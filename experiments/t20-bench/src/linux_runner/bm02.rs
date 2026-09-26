//! BM-02 development runner: durable single-operation commits, corrections, interleaved reads and
//! batched ingestion through the authorized journal coordinator on the Linux/Btrfs adapter.
//!
//! This is a development runner, never qualification. It writes through the ordinary authorized
//! durable commit path with the graph reducer (the journal remains the sole commit authority); it
//! does not exercise the packed disk-index writer used by BM-01. Reports are content-free and
//! state every boundary; Decision 0007's targets are echoed but not evaluated.

use std::{collections::BTreeMap, path::Path, time::Instant};

use uste_crypto::{KeyVault, OsEntropy, PortableRecoveryAdapter};
use uste_graph::{
    Expected, GraphReadOutput, GraphReadRequest, GraphState, GraphTransaction,
    MAX_TRANSACTION_OPERATIONS, NewEntity, NewRecord, Operation, Record, RecordVersion,
    encode_transaction,
};
use uste_policy::AuthenticatedPrincipal;
use uste_storage::EntryName;
use uste_storage::linux::LinuxFileSystem;
use uste_txn::{
    AuthorizedCoordinator, AuthorizedTransactionRequest, CommitCoordinator, MAX_REQUEST_BYTES,
    NeverCancel, TransactionRequest,
};
use uste_types::{IdempotencyKey, RecordId, RecordRef, TransactionId, Value};

use super::{
    LinuxCoordinator, LinuxRunnerError, SystemClock, authenticate, credential, hex,
    open_filesystem, percentile, process_rss, retention,
};
use crate::engine::{PRINCIPAL, benchmark_policy, kernel, scope, text};

const DATABASE_NAME: &str = "bm02-linux-development";
/// Development ceilings; the qualifying BM-02 workload is defined separately and is not run here.
pub const MAX_BM02_SINGLE_COMMITS: u32 = 2_000;
pub const MAX_BM02_BATCHES: u32 = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bm02Plan {
    pub single_commits: u32,
    pub batches: u32,
    pub batch_events: u32,
}

impl Bm02Plan {
    pub fn new(single_commits: u32, batches: u32, batch_events: u32) -> Result<Self, String> {
        if single_commits == 0
            || single_commits > MAX_BM02_SINGLE_COMMITS
            || batches == 0
            || batches > MAX_BM02_BATCHES
            || batch_events == 0
            || usize::try_from(batch_events).map_err(|_| "batch size")? > MAX_TRANSACTION_OPERATIONS
        {
            return Err(format!(
                "BM-02 development plan must have 1..={MAX_BM02_SINGLE_COMMITS} single commits, \
                 1..={MAX_BM02_BATCHES} batches and 1..={MAX_TRANSACTION_OPERATIONS} events per batch"
            ));
        }
        Ok(Self {
            single_commits,
            batches,
            batch_events,
        })
    }
}

fn error(code: &'static str) -> LinuxRunnerError {
    LinuxRunnerError::new(code)
}

fn bm02_record(ordinal: u64) -> RecordRef {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(b"BM02DEV\0");
    bytes[8..].copy_from_slice(&ordinal.to_be_bytes());
    RecordRef::new(
        scope().database(),
        scope().namespace(),
        RecordId::from_bytes(bytes),
    )
}

fn operation_identity(sequence: u64) -> ([u8; 16], [u8; 16]) {
    let mut key = [0_u8; 16];
    key[..8].copy_from_slice(b"BM02KEY\0");
    key[8..].copy_from_slice(&sequence.to_be_bytes());
    let mut transaction = [0_u8; 16];
    transaction[..8].copy_from_slice(b"BM02TXN\0");
    transaction[8..].copy_from_slice(&sequence.to_be_bytes());
    (key, transaction)
}

struct Writer {
    filesystem: LinuxFileSystem,
    coordinator: LinuxCoordinator,
    principal: AuthenticatedPrincipal,
    clock: SystemClock,
    sequence: u64,
    committed_bytes: u64,
}

impl Writer {
    /// Commit one transaction and return the elapsed time to its durable receipt.
    fn commit(&mut self, operations: Vec<Operation>) -> Result<u128, LinuxRunnerError> {
        let bytes = encode_transaction(&GraphTransaction::new(scope(), operations))
            .map_err(|_| error("USTE_BM02_TRANSACTION_ENCODE"))?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(error("USTE_BM02_REQUEST_BOUNDS"));
        }
        let (key, transaction) = operation_identity(self.sequence);
        let started = Instant::now();
        let outcome = self
            .coordinator
            .commit(
                &mut self.filesystem,
                &self.principal,
                AuthorizedTransactionRequest {
                    idempotency_key: IdempotencyKey::from_bytes(key),
                    transaction_id: TransactionId::from_bytes(transaction),
                    canonical_request: &bytes,
                    blob_inventory: None,
                },
                &mut self.clock,
                &NeverCancel,
            )
            .map_err(|_| error("USTE_BM02_TRANSACTION_COMMIT"))?;
        let elapsed = started.elapsed().as_nanos();
        if outcome.revision.get() != self.sequence {
            return Err(error("USTE_BM02_REVISION_MISMATCH"));
        }
        self.sequence += 1;
        self.committed_bytes += u64::try_from(bytes.len()).map_err(|_| error("USTE_BM02_BYTES"))?;
        Ok(elapsed)
    }

    /// Read one record through a fresh authorized view and return its version and latency.
    fn read(&self, id: RecordRef) -> Result<(RecordVersion, u128), LinuxRunnerError> {
        let started = Instant::now();
        let view = self
            .coordinator
            .read_view(&self.principal)
            .map_err(|_| error("USTE_BM02_READ_VIEW"))?;
        let output = self
            .coordinator
            .read(&self.principal, &view, &GraphReadRequest::Record { id })
            .map_err(|_| error("USTE_BM02_READ"))?;
        let elapsed = started.elapsed().as_nanos();
        match output {
            GraphReadOutput::Record(Some(record)) => match *record {
                Record::Entity(entity) if entity.id == id => Ok((entity.version, elapsed)),
                _ => Err(error("USTE_BM02_READ_MISMATCH")),
            },
            _ => Err(error("USTE_BM02_READ_MISMATCH")),
        }
    }
}

fn create_entity(ordinal: u64) -> Result<Operation, LinuxRunnerError> {
    Ok(Operation::Create {
        expected: Expected::Absent,
        record: NewRecord::Entity(NewEntity {
            id: bm02_record(ordinal),
            entity_type: text("bm02-entity-v1").map_err(|_| error("USTE_BM02_TEXT"))?,
            schema_version: 1,
            properties: Value::Null,
        }),
    })
}

/// Units per second over `nanoseconds`; precision loss is irrelevant at report resolution.
#[allow(clippy::cast_precision_loss)]
fn rate(units: u64, nanoseconds: u128) -> f64 {
    if nanoseconds == 0 {
        0.0
    } else {
        units as f64 * 1e9 / nanoseconds as f64
    }
}

fn summary(mut samples: Vec<u128>) -> serde_json::Value {
    samples.sort_unstable();
    serde_json::json!({
        "count": samples.len(),
        "p50_nanoseconds": percentile(&samples, 50),
        "p95_nanoseconds": percentile(&samples, 95),
        "p99_nanoseconds": percentile(&samples, 99),
        "maximum_nanoseconds": samples.last().copied().unwrap_or(0),
    })
}

/// Create a fresh development store under `root` and run `plan` against it.
pub fn development(
    root: &Path,
    password_file: &Path,
    plan: Bm02Plan,
) -> Result<String, LinuxRunnerError> {
    let setup_started = Instant::now();
    let mut filesystem = open_filesystem(root)?;
    let mut adapter = PortableRecoveryAdapter::new(credential::read_password(password_file)?);
    let vault = KeyVault::create(scope().database(), &mut adapter, OsEntropy)
        .map_err(|_| error("USTE_BM02_KEY_CREATE"))?;
    let mut raw = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        retention()?,
        EntryName::new(DATABASE_NAME).map_err(|_| error("USTE_BM02_DATABASE_NAME"))?,
        vault,
        OsEntropy,
        GraphState::new(scope()),
    )
    .map_err(|_| error("USTE_BM02_DATABASE_CREATE"))?;
    let policy = benchmark_policy(scope()).map_err(|_| error("USTE_BM02_POLICY_PROFILE"))?;
    let install = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        Vec::new(),
        uste_graph::DurablePolicyMutation::Install {
            policy: policy.clone(),
        },
    ))
    .map_err(|_| error("USTE_BM02_POLICY_ENCODE"))?;
    let mut clock = SystemClock::new();
    let (key, transaction) = operation_identity(1);
    raw.commit(
        &mut filesystem,
        TransactionRequest {
            principal: PRINCIPAL,
            idempotency_key: IdempotencyKey::from_bytes(key),
            transaction_id: TransactionId::from_bytes(transaction),
            canonical_request: &install,
            blob_inventory: None,
        },
        &mut clock,
        &NeverCancel,
    )
    .map_err(|_| error("USTE_BM02_POLICY_COMMIT"))?;
    let policy_kernel = kernel(policy).map_err(|_| error("USTE_BM02_POLICY_PROFILE"))?;
    let principal = authenticate(&policy_kernel)?;
    let coordinator = AuthorizedCoordinator::new(raw, policy_kernel)
        .map_err(|_| error("USTE_BM02_AUTHORIZED_OPEN"))?;
    let mut writer = Writer {
        filesystem,
        coordinator,
        principal,
        clock,
        sequence: 2,
        committed_bytes: 0,
    };
    let setup_elapsed = setup_started.elapsed();

    // Single-operation durable commits: three creates then one correction of the most recently
    // created entity, repeated; every commit is followed by an authorized point read.
    let mut next_ordinal = 0_u64;
    let mut versions = BTreeMap::<u64, RecordVersion>::new();
    let mut commit_samples = Vec::new();
    let mut create_samples = Vec::new();
    let mut correction_samples = Vec::new();
    let mut read_samples = Vec::new();
    let single_started = Instant::now();
    for index in 0..plan.single_commits {
        let correction = index % 4 == 3;
        let ordinal = if correction {
            next_ordinal - 1
        } else {
            next_ordinal += 1;
            next_ordinal - 1
        };
        let operation = if correction {
            let current = *versions
                .get(&ordinal)
                .ok_or_else(|| error("USTE_BM02_CORRECTION_TARGET"))?;
            Operation::ReplaceEntity {
                target: bm02_record(ordinal),
                expected: Expected::Version(current),
                properties: Value::Unsigned(u128::from(index)),
            }
        } else {
            create_entity(ordinal)?
        };
        let elapsed = writer.commit(vec![operation])?;
        commit_samples.push(elapsed);
        if correction {
            correction_samples.push(elapsed);
        } else {
            create_samples.push(elapsed);
        }
        let (version, read) = writer.read(bm02_record(ordinal))?;
        let expected = match versions.get(&ordinal) {
            None => RecordVersion::FIRST,
            Some(previous) => {
                RecordVersion::new(previous.get() + 1).map_err(|_| error("USTE_BM02_VERSION"))?
            }
        };
        if version != expected {
            return Err(error("USTE_BM02_READ_MISMATCH"));
        }
        versions.insert(ordinal, version);
        read_samples.push(read);
    }
    let single_elapsed = single_started.elapsed();

    // Batched ingestion: each transaction creates `batch_events` new entities.
    let mut batch_samples = Vec::new();
    let batch_started = Instant::now();
    for _ in 0..plan.batches {
        let operations = (0..plan.batch_events)
            .map(|_| {
                next_ordinal += 1;
                create_entity(next_ordinal - 1)
            })
            .collect::<Result<Vec<_>, _>>()?;
        batch_samples.push(writer.commit(operations)?);
    }
    let batch_wall = batch_started.elapsed().as_nanos();
    let batch_events = u64::from(plan.batches) * u64::from(plan.batch_events);
    let single_commit_nanoseconds = commit_samples.iter().sum::<u128>();
    let batch_commit_nanoseconds = batch_samples.iter().sum::<u128>();
    let (current_rss_kib, peak_rss_kib) = process_rss()?;
    let mut digest = blake3::Hasher::new_derive_key("USTE BM-02 development-v1");
    digest.update(&writer.sequence.to_be_bytes());
    digest.update(&next_ordinal.to_be_bytes());
    for (ordinal, version) in &versions {
        digest.update(&ordinal.to_be_bytes());
        digest.update(&version.get().to_be_bytes());
    }
    Ok(serde_json::json!({
        "schema": "bm02-linux-development-v1",
        "engine_benchmark": false,
        "qualification": "nonqualifying-development",
        "budget_evaluation": "not-performed",
        "write_path": "authorized-journal-coordinator-with-in-memory-graph-reducer",
        "packed_disk_index_writer": false,
        "filesystem_profile": "linux-x86_64-btrfs",
        "kernel_filesystem_device_cache": "uncontrolled",
        "durable_receipt": "commit returns after journal publication",
        "commit_time": "canonical request submitted to durable receipt; client encoding excluded",
        "phase_wall": "includes operation construction and client encoding",
        "reader_mode": "same-thread-interleaved-after-each-single-commit",
        "not_measured": ["concurrent-readers", "queue-depth", "backpressure", "blob-ingestion"],
        "decision_0007_targets": {
            "single_commit_p99_milliseconds": 50,
            "batch_events_per_second": 2000,
        },
        "plan": {
            "single_commits": plan.single_commits,
            "correction_every": 4,
            "batches": plan.batches,
            "batch_events": plan.batch_events,
        },
        "setup_milliseconds": setup_elapsed.as_millis(),
        "single_phase_milliseconds": single_elapsed.as_millis(),
        "single_commit_latency": summary(commit_samples),
        "create_commit_latency": summary(create_samples),
        "correction_commit_latency": summary(correction_samples),
        "interleaved_read_latency": summary(read_samples),
        "batch_commit_latency": summary(batch_samples),
        "batch_events": batch_events,
        "batch_events_per_second_commit_time": rate(batch_events, batch_commit_nanoseconds),
        "batch_events_per_second_phase_wall": rate(batch_events, batch_wall),
        "single_transactions_per_second_commit_time": rate(
            u64::from(plan.single_commits),
            single_commit_nanoseconds
        ),
        "committed_request_bytes_per_second_commit_time": rate(
            writer.committed_bytes,
            single_commit_nanoseconds + batch_commit_nanoseconds
        ),
        "final_revision": writer.sequence - 1,
        "committed_request_bytes": writer.committed_bytes,
        "entities_created": next_ordinal,
        "outcome_digest": hex(digest.finalize().as_bytes()),
        "current_rss_kib": current_rss_kib,
        "process_peak_rss_kib": peak_rss_kib,
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_refuses_empty_and_oversized_development_runs() {
        assert!(Bm02Plan::new(1, 1, 1).is_ok());
        let maximum_events = u32::try_from(MAX_TRANSACTION_OPERATIONS).unwrap();
        assert!(Bm02Plan::new(MAX_BM02_SINGLE_COMMITS, MAX_BM02_BATCHES, maximum_events).is_ok());
        for (single, batches, events) in [
            (0, 1, 1),
            (1, 0, 1),
            (1, 1, 0),
            (MAX_BM02_SINGLE_COMMITS + 1, 1, 1),
            (1, MAX_BM02_BATCHES + 1, 1),
            (1, 1, maximum_events + 1),
        ] {
            assert!(Bm02Plan::new(single, batches, events).is_err());
        }
    }

    #[test]
    fn identities_are_distinct_per_sequence() {
        assert_ne!(operation_identity(1), operation_identity(2));
        assert_ne!(bm02_record(0), bm02_record(1));
        let (key, transaction) = operation_identity(7);
        assert_ne!(key, transaction);
    }
}
