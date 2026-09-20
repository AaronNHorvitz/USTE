//! Native packed phases with bounded profile-bound bootstrap and paired-prefix recovery.
use super::*;
use crate::engine::{
    disk::{DiskBatch, visit_disk_batches},
    packed::{self as engine, limits::Limits},
};
use disk::io::ObservedFileSystem;
use uste_graph::recover_packed_graph_origin;
use uste_txn::{AuthenticatedIndexRecovery, AuthorizedPackedReader, CoordinatorRecoveryLimits};
mod bootstrap;
mod crypto_work;
pub mod history;
mod query;
mod sampling;
pub use query::query_correctness;
pub use sampling::sample_worker;
type Fs = ObservedFileSystem<LinuxFileSystem>;
type Packed = engine::PackedEngine<Fs, RecoveryEnvelope, OsEntropy, OsEntropy>;
type Recovery = engine::RecoveryEngine<Fs, RecoveryEnvelope, OsEntropy, OsEntropy>;
const DATABASE: &str = "bm01-linux-packed-engine";
const STAGING_CACHE_BYTES: usize = 64 * 1024 * 1024;
const STAGING_CACHE_SCOPE: &str = "fresh-per-private-tree-batch";
const PROOF_CACHE_BYTES: usize = 64 * 1024 * 1024;
const PROOF_CACHE_SCOPE: &str = "fresh-per-preparation";

fn buffered_limits(mut limits: Limits) -> Limits {
    limits.preparation.proof_cache_bytes = Some(PROOF_CACHE_BYTES);
    limits.origin.suffix.proof_cache_bytes = Some(PROOF_CACHE_BYTES);
    limits.origin.genesis.stage.staging_cache_bytes = Some(STAGING_CACHE_BYTES);
    limits.origin.suffix.graph.staging_cache_bytes = Some(STAGING_CACHE_BYTES);
    limits.origin.suffix.metadata.staging.staging_cache_bytes = Some(STAGING_CACHE_BYTES);
    limits.publication.stage.staging_cache_bytes = Some(STAGING_CACHE_BYTES);
    limits
}

struct Session {
    filesystem: Fs,
    coordinator: Packed,
    policy: PolicyKernel,
    principal: AuthenticatedPrincipal,
    limits: Limits,
    report: serde_json::Value,
}
fn error(code: &'static str) -> LinuxRunnerError {
    LinuxRunnerError::new(code)
}
fn open_recovery(
    fs: &mut Fs,
    adapter: &mut PortableRecoveryAdapter,
    limits: Limits,
) -> Result<(Recovery, RecoveryReport), LinuxRunnerError> {
    let (recovery, report, _) = AuthenticatedIndexRecovery::open_with_disk_blob_metadata(
        fs,
        &EntryName::new(DATABASE).map_err(|_| error("USTE_BM01_DATABASE_NAME"))?,
        scope(),
        OsEntropy,
        OsEntropy,
        adapter,
        limits.legacy.blob_recovery,
        &mut uste_storage::PageCache::new(64 * 1024 * 1024)
            .map_err(|_| error("USTE_BM01_LIMITS"))?,
    )
    .map_err(|_| error("USTE_BM01_PACKED_OPEN"))?;
    Ok((recovery, report))
}
// Authenticate the revision-two profile marker before explicit derived reconstruction. This
// establishes fixture selection, not permission to alter authoritative history.
fn source_binding(
    fs: &mut Fs,
    recovery: &mut Recovery,
    profile: Bm01Profile,
    limits: Limits,
) -> Result<(), LinuxRunnerError> {
    let revision = uste_types::CommitRevision::new(2).map_err(|_| error("USTE_BM01_LIMITS"))?;
    let mut cursor = recovery
        .open_transaction_cursor(revision, revision, 1, limits.legacy.prefix_bytes)
        .map_err(|_| error("USTE_BM01_PACKED_BINDING"))?;
    let transaction = recovery
        .next_recovered_transaction(fs, &mut cursor)
        .map_err(|_| error("USTE_BM01_PACKED_BINDING"))?
        .ok_or_else(|| error("USTE_BM01_PACKED_BINDING"))?;
    if recovery
        .next_recovered_transaction(fs, &mut cursor)
        .map_err(|_| error("USTE_BM01_PACKED_BINDING"))?
        .is_some()
    {
        return Err(error("USTE_BM01_PACKED_BINDING"));
    }
    recovery
        .finish_transaction_cursor(cursor)
        .map_err(|_| error("USTE_BM01_PACKED_BINDING"))?;
    let tx = uste_graph::decode_transaction(transaction.canonical_request())
        .map_err(|_| error("USTE_BM01_PACKED_BINDING"))?;
    let Some(Operation::Create {
        expected: Expected::Absent,
        record: NewRecord::Evidence(evidence),
    }) = tx.operations().first()
    else {
        return Err(error("USTE_BM01_PACKED_BINDING"));
    };
    if tx.scope() != scope()
        || tx.policy_mutation().is_some()
        || transaction.blob_inventory().is_some()
        || evidence.id != evidence_ref(scope())
        || evidence.digest != engine_mapping_digest(profile)
        || evidence.locator.as_str() != "bm01-uste-graph-v1"
    {
        return Err(error("USTE_BM01_PACKED_BINDING"));
    }
    Ok(())
}
fn require_binding(session: &mut Session, profile: Bm01Profile) -> Result<(), LinuxRunnerError> {
    let mut limits = session
        .limits
        .read()
        .map_err(|_| error("USTE_BM01_LIMITS"))?;
    limits.expansion = None;
    let reader = AuthorizedPackedReader::new(&session.coordinator, &session.policy, limits)
        .map_err(|_| error("USTE_BM01_PACKED_AUTHORIZATION"))?;
    let output = reader
        .read(
            &mut session.filesystem,
            &session.principal,
            &GraphReadRequest::Record {
                id: evidence_ref(scope()),
            },
            &NeverCancel,
        )
        .map_err(|_| error("USTE_BM01_PACKED_BINDING"))?;
    let GraphReadOutput::Record(Some(record)) = output else {
        return Err(error("USTE_BM01_PACKED_BINDING"));
    };
    let Record::Evidence(evidence) = *record else {
        return Err(error("USTE_BM01_PACKED_BINDING"));
    };
    if evidence.id != evidence_ref(scope())
        || evidence.version != RecordVersion::FIRST
        || evidence.created_revision.get() != 2
        || evidence.digest != engine_mapping_digest(profile)
        || evidence.locator.as_str() != "bm01-uste-graph-v1"
    {
        return Err(error("USTE_BM01_PACKED_BINDING"));
    }
    Ok(())
}
pub fn run(
    root: &Path,
    password_file: &Path,
    profile: Bm01Profile,
    phase: &str,
) -> Result<String, LinuxRunnerError> {
    prepare(root, password_file, profile, phase).map(|session| session.report.to_string())
}
fn prepare(
    root: &Path,
    password_file: &Path,
    profile: Bm01Profile,
    phase: &str,
) -> Result<Session, LinuxRunnerError> {
    prepare_observed(root, password_file, profile, phase, &mut |_| Ok(()))
}

/// Development control; the test supervisor must own and reap the parked process.
pub fn create_crash_probe(
    root: &Path,
    password_file: &Path,
    profile: Bm01Profile,
    pause: u64,
) -> Result<String, LinuxRunnerError> {
    disk::validate_native_profile(profile)?;
    if pause > materialization_revision_count(profile) {
        return Err(error("USTE_BM01_PACKED_PROBE_REVISION"));
    }
    prepare_observed(root, password_file, profile, "create", &mut |revision| {
        if revision == pause {
            use std::io::Write;
            let mut output = std::io::stdout().lock();
            writeln!(
                output,
                "{{\"schema\":\"bm01-packed-durable-prefix-v1\",\"frontier\":{revision}}}"
            )
            .and_then(|()| output.flush())
            .map_err(|_| error("USTE_BM01_PACKED_PROBE_SIGNAL"))?;
            loop {
                std::thread::park();
            }
        }
        Ok(())
    })
    .map(|session| session.report.to_string())
}

fn prepare_observed(
    root: &Path,
    password_file: &Path,
    profile: Bm01Profile,
    phase: &str,
    observer: &mut impl FnMut(u64) -> Result<(), LinuxRunnerError>,
) -> Result<Session, LinuxRunnerError> {
    disk::validate_native_profile(profile)?;
    if !matches!(phase, "create" | "open" | "rebuild" | "resume") {
        return Err(error("USTE_BM01_PACKED_PHASE"));
    }
    let limits = buffered_limits(Limits::new(profile).map_err(|_| error("USTE_BM01_LIMITS"))?);
    let started = Instant::now();
    let mut fs = ObservedFileSystem::new(open_filesystem(root)?);
    let mut adapter = PortableRecoveryAdapter::new(credential::read_password(password_file)?);
    if phase == "create" {
        let vault = KeyVault::create(scope().database(), &mut adapter, OsEntropy)
            .map_err(|_| error("USTE_BM01_KEY_CREATE"))?;
        let mut raw = CommitCoordinator::create(
            &mut fs,
            scope(),
            retention()?,
            EntryName::new(DATABASE).map_err(|_| error("USTE_BM01_DATABASE_NAME"))?,
            vault,
            OsEntropy,
            GraphState::new(scope()),
        )
        .map_err(|_| error("USTE_BM01_DATABASE_CREATE"))?;
        observer(0)?;
        bootstrap::install(&mut raw, &mut fs, profile)?;
        observer(1)?;
        drop(raw);
    }
    let (mut recovery, recovered) = open_recovery(&mut fs, &mut adapter, limits)?;
    let recovered_frontier = recovered.frontier.map_or(0, |revision| revision.get());
    let bootstrap_resume = phase == "resume" && recovered_frontier <= 1;
    if bootstrap_resume {
        recovery = bootstrap::resume(recovery, &mut fs, &mut adapter, profile, limits)?;
    }
    let mut policy =
        kernel(benchmark_policy(scope()).map_err(|_| error("USTE_BM01_POLICY_PROFILE"))?)
            .map_err(|_| error("USTE_BM01_POLICY_PROFILE"))?;
    let principal = authenticate(&policy)?;
    let expected = materialization_revision_count(profile);
    let mut origin_groups = None;
    let mut resume_base_revision = None;
    let mut resume_suffix_groups = None;
    if phase == "create" || phase == "resume" {
        let mut live = if phase == "create" || bootstrap_resume {
            if recovery
                .authenticated_frontier_anchor()
                .map(|(revision, _)| revision.get())
                != Some(1)
            {
                return Err(error("USTE_BM01_PACKED_BOOTSTRAP"));
            }
            if bootstrap_resume {
                resume_base_revision = Some(1);
                resume_suffix_groups = Some(0);
            }
            recover_packed_graph_origin(
                recovery,
                &mut fs,
                retention()?,
                CoordinatorRecoveryLimits::new(1, 0).map_err(|_| error("USTE_BM01_LIMITS"))?,
                limits.origin,
            )
            .map_err(|_| error("USTE_BM01_PACKED_BOOTSTRAP"))?
            .0
        } else {
            if !matches!(recovered.frontier.map(|r| r.get()), Some(value) if value >= 2 && value <= expected)
            {
                return Err(error("USTE_BM01_PACKED_RESUME_PREFIX"));
            }
            source_binding(&mut fs, &mut recovery, profile, limits)?;
            let (live, base, groups) = engine::prefix::recover_latest(&mut fs, recovery, profile)
                .map_err(|_| error("USTE_BM01_PACKED_PREFIX_ADMISSION"))?;
            resume_base_revision = Some(base);
            resume_suffix_groups = Some(groups);
            live
        };
        let mut clock = SystemClock::new();
        visit_disk_batches(profile, |sequence, operations| {
            engine::commit_batch_observed(
                &mut live,
                &mut fs,
                &mut policy,
                &principal,
                DiskBatch {
                    sequence,
                    idempotency_key: identity(sequence, IdempotencyKey::from_bytes),
                    transaction_id: identity(sequence, TransactionId::from_bytes),
                    operations,
                },
                &mut clock,
                limits,
                &mut |revision| observer(revision).map_err(|error| error.code().to_string()),
            )?;
            Ok(())
        })
        .map_err(|_| error("USTE_BM01_PACKED_MATERIALIZE"))?;
        drop(live);
        recovery = open_recovery(&mut fs, &mut adapter, limits)?.0;
    }
    let frontier = recovery
        .authenticated_frontier_anchor()
        .ok_or_else(|| error("USTE_BM01_FRONTIER_MISMATCH"))?
        .0
        .get();
    if frontier > expected || (phase != "rebuild" && frontier != expected) {
        return Err(error("USTE_BM01_FRONTIER_MISMATCH"));
    }
    if phase == "rebuild" {
        if frontier == 1 {
            bootstrap::source_binding(&mut fs, &mut recovery, profile)?;
        } else {
            source_binding(&mut fs, &mut recovery, profile, limits)?;
        }
        let (live, report) = recover_packed_graph_origin(
            recovery,
            &mut fs,
            retention()?,
            CoordinatorRecoveryLimits::new(0, 0).map_err(|_| error("USTE_BM01_LIMITS"))?,
            limits.origin,
        )
        .map_err(|_| error("USTE_BM01_PACKED_REBUILD"))?;
        if report.suffix.journal.groups != frontier - 1 || live.overlay_counts() != (0, 0) {
            return Err(error("USTE_BM01_PACKED_REBUILD"));
        }
        origin_groups = Some(report.suffix.journal.groups);
        drop(live);
        recovery = open_recovery(&mut fs, &mut adapter, limits)?.0;
    }
    let mut selected = limits;
    selected.counts = engine::prefix::counts(
        profile,
        uste_types::CommitRevision::new(frontier)
            .map_err(|_| error("USTE_BM01_FRONTIER_MISMATCH"))?,
    )
    .map_err(|_| error("USTE_BM01_LIMITS"))?;
    let (live, digest) = engine::admit(&mut fs, recovery, selected)
        .map_err(|_| error("USTE_BM01_PACKED_ADMISSION"))?;
    let mut session = Session {
        filesystem: fs,
        coordinator: live,
        policy,
        principal,
        limits,
        report: serde_json::Value::Null,
    };
    if frontier >= 2 {
        require_binding(&mut session, profile)?;
    }
    let (rss, peak) = process_rss()?;
    let terminal_vault_work = crypto_work::CryptoWork::from(
        session
            .coordinator
            .vault_decrypt_report()
            .map_err(|_| error("USTE_BM01_CRYPTO_COUNTER"))?,
    );
    session.report = serde_json::json!({
        "schema": "bm01-linux-packed-development-v1", "engine_benchmark": false,
        "qualification": "nonqualifying-development-profile", "phase": phase,
        "filesystem_profile": "linux-x86_64-btrfs", "storage_metadata_mode": "disk-certificate-and-blob-recovery",
        "full_memory_graph_state": false, "full_memory_coordinator_metadata": false,
        "graph_admission_cache_bytes": engine::GRAPH_ADMISSION_CACHE_BYTES,
        "staging_cache_bytes": STAGING_CACHE_BYTES,
        "staging_cache_scope": STAGING_CACHE_SCOPE,
        "graph_admission_cache_scope": engine::GRAPH_ADMISSION_CACHE_SCOPE,
        "coordinator_admission_buffered": true,
        "coordinator_admission_cache_bytes": engine::COORDINATOR_ADMISSION_CACHE_BYTES,
        "coordinator_admission_cache_scope": engine::COORDINATOR_ADMISSION_CACHE_SCOPE,
        "entities": profile.entities(), "relationships": profile.relationships(), "frontier": frontier,
        "complete_fixture": frontier == expected,
        "v1_state_digest": hex(&digest), "origin_suffix_groups": origin_groups,
        "elapsed_milliseconds": started.elapsed().as_millis(), "current_rss_kib": rss, "process_peak_rss_kib": peak,
        "repaired_certificate_tail_bytes": recovered.repaired_certificate_tail_bytes,
        "ignored_uncommitted_journal_bytes": recovered.ignored_uncommitted_journal_bytes,
        "kernel_filesystem_device_cache": "uncontrolled", "complete_authenticated_io": false,
        "setup_adapter_io": session.filesystem.snapshot()?.json()?,
        "terminal_vault_work": terminal_vault_work.json(),
        "terminal_vault_work_scope": "last-cold-open-owner-only",
        "development_entity_limit": disk::MAX_NATIVE_DEVELOPMENT_ENTITIES,
        "data_bearing_prefix_resume_implemented": true,
        "policy_only_prefix_resume_implemented": true, "legacy_unbound_policy_resume_supported": false,
        "bounded_bootstrap_resume": bootstrap_resume, "recovered_frontier": recovered_frontier,
        "resume_base_revision": resume_base_revision, "resume_suffix_groups": resume_suffix_groups,
    });
    session.report["proof_cache_bytes"] = serde_json::json!(PROOF_CACHE_BYTES);
    session.report["proof_cache_scope"] = serde_json::json!(PROOF_CACHE_SCOPE);
    Ok(session)
}

#[cfg(test)]
mod tests;
