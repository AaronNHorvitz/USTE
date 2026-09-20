//! Native small BM-06 checkpoint/tail pipeline; no larger-than-memory qualification.
use super::*;
use crate::recovery_materialization::{Bm06Profile, VERSIONS};
use uste_txn::AuthorizedPackedWriter;
const HISTORY_DATABASE: &str = "bm06-linux-packed-engine";
const MAX_RECORDS: u64 = 2;
#[cfg(test)]
mod tests;

fn bootstrap_id<T>(profile: Bm06Profile, construct: impl FnOnce([u8; 16]) -> T) -> T {
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(b"BM06PK1\0");
    bytes[8..].copy_from_slice(&profile.records().to_be_bytes());
    construct(bytes)
}
fn batch(profile: Bm06Profile, sequence: u64) -> Result<DiskBatch, LinuxRunnerError> {
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(b"BM06PD1\0");
    bytes[8..].copy_from_slice(&sequence.to_be_bytes());
    Ok(DiskBatch {
        sequence,
        idempotency_key: IdempotencyKey::from_bytes(bytes),
        transaction_id: TransactionId::from_bytes(bytes),
        operations: profile
            .batch(scope(), sequence)
            .map_err(|_| error("USTE_BM06_PACKED_BATCH"))?,
    })
}
fn policy_bytes() -> Result<Vec<u8>, LinuxRunnerError> {
    encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        Vec::new(),
        uste_graph::DurablePolicyMutation::Install {
            policy: crate::engine::recovery::recovery_policy()
                .map_err(|_| error("USTE_BM06_POLICY"))?,
        },
    ))
    .map_err(|_| error("USTE_BM06_POLICY"))
}
fn open(
    fs: &mut Fs,
    adapter: &mut PortableRecoveryAdapter,
    limits: Limits,
) -> Result<(Recovery, RecoveryReport), LinuxRunnerError> {
    let (recovery, report, _) = AuthenticatedIndexRecovery::open_with_disk_blob_metadata(
        fs,
        &EntryName::new(HISTORY_DATABASE).map_err(|_| error("USTE_BM06_NAME"))?,
        scope(),
        OsEntropy,
        OsEntropy,
        adapter,
        limits.legacy.blob_recovery,
        &mut uste_storage::PageCache::new(64 * 1024 * 1024)
            .map_err(|_| error("USTE_BM06_LIMITS"))?,
    )
    .map_err(|_| error("USTE_BM06_PACKED_OPEN"))?;
    Ok((recovery, report))
}
fn binding(
    fs: &mut Fs,
    recovery: &mut Recovery,
    profile: Bm06Profile,
) -> Result<(), LinuxRunnerError> {
    let first = uste_types::CommitRevision::FIRST;
    let mut cursor = recovery
        .open_transaction_cursor(first, first, 1, 1_048_576)
        .map_err(|_| error("USTE_BM06_PACKED_BINDING"))?;
    let transaction = recovery
        .next_recovered_transaction(fs, &mut cursor)
        .map_err(|_| error("USTE_BM06_PACKED_BINDING"))?
        .ok_or_else(|| error("USTE_BM06_PACKED_BINDING"))?;
    if recovery
        .next_recovered_transaction(fs, &mut cursor)
        .map_err(|_| error("USTE_BM06_PACKED_BINDING"))?
        .is_some()
    {
        return Err(error("USTE_BM06_PACKED_BINDING"));
    }
    recovery
        .finish_transaction_cursor(cursor)
        .map_err(|_| error("USTE_BM06_PACKED_BINDING"))?;
    if transaction.outcome().transaction_id != bootstrap_id(profile, TransactionId::from_bytes)
        || transaction.blob_inventory().is_some()
        || transaction.canonical_request() != policy_bytes()?
    {
        return Err(error("USTE_BM06_PACKED_BINDING"));
    }
    Ok(())
}
// The native cap keeps one batch per generation. This is not a general qualifying prefix formula.
fn counts(profile: Bm06Profile, revision: u64) -> Result<[u64; 8], LinuxRunnerError> {
    if profile.records() > MAX_RECORDS || revision == 0 || revision > profile.frontier() {
        return Err(error("USTE_BM06_PACKED_FRONTIER"));
    }
    Ok([
        if revision == 1 { 0 } else { profile.records() },
        profile.records() * (revision - 1),
        0,
        0,
        0,
        0,
        1,
        1,
    ])
}

pub fn run(
    root: &Path,
    password: &Path,
    profile: Bm06Profile,
    phase: &str,
) -> Result<String, LinuxRunnerError> {
    if profile.records() > MAX_RECORDS {
        return Err(error("USTE_BM06_PACKED_DEVELOPMENT_LIMIT"));
    }
    if !matches!(phase, "create" | "open" | "tail" | "recover" | "rebuild") {
        return Err(error("USTE_BM06_PACKED_PHASE"));
    }
    let started = Instant::now();
    let mut fs = ObservedFileSystem::new(open_filesystem(root)?);
    let mut adapter = PortableRecoveryAdapter::new(credential::read_password(password)?);
    let limits = Limits::recovery(profile).map_err(|_| error("USTE_BM06_LIMITS"))?;
    if phase == "create" {
        let vault = KeyVault::create(scope().database(), &mut adapter, OsEntropy)
            .map_err(|_| error("USTE_BM06_KEY_CREATE"))?;
        let mut raw = CommitCoordinator::create(
            &mut fs,
            scope(),
            retention()?,
            EntryName::new(HISTORY_DATABASE).map_err(|_| error("USTE_BM06_NAME"))?,
            vault,
            OsEntropy,
            GraphState::new(scope()),
        )
        .map_err(|_| error("USTE_BM06_PACKED_CREATE"))?;
        raw.commit(
            &mut fs,
            TransactionRequest {
                principal: PRINCIPAL,
                idempotency_key: bootstrap_id(profile, IdempotencyKey::from_bytes),
                transaction_id: bootstrap_id(profile, TransactionId::from_bytes),
                canonical_request: &policy_bytes()?,
                blob_inventory: None,
            },
            &mut SystemClock::new(),
            &NeverCancel,
        )
        .map_err(|_| error("USTE_BM06_PACKED_BOOTSTRAP"))?;
        drop(raw);
    }
    let (mut recovery, recovered) = open(&mut fs, &mut adapter, limits)?;
    binding(&mut fs, &mut recovery, profile)?;
    let mut kernel =
        kernel(crate::engine::recovery::recovery_policy().map_err(|_| error("USTE_BM06_POLICY"))?)
            .map_err(|_| error("USTE_BM06_POLICY"))?;
    let principal = authenticate(&kernel)?;
    if phase == "create" {
        if recovered.frontier.map(|r| r.get()) != Some(1) {
            return Err(error("USTE_BM06_PACKED_FRONTIER"));
        }
        let (mut live, _) = recover_packed_graph_origin(
            recovery,
            &mut fs,
            retention()?,
            CoordinatorRecoveryLimits::new(1, 0).map_err(|_| error("USTE_BM06_LIMITS"))?,
            limits.origin,
        )
        .map_err(|_| error("USTE_BM06_PACKED_BOOTSTRAP"))?;
        for sequence in 2..=profile.checkpoint_revision() {
            engine::commit_batch(
                &mut live,
                &mut fs,
                &mut kernel,
                &principal,
                batch(profile, sequence)?,
                &mut SystemClock::new(),
                limits,
            )
            .map_err(|_| error("USTE_BM06_PACKED_MATERIALIZE"))?;
        }
        drop(live);
        recovery = open(&mut fs, &mut adapter, limits)?.0;
    }
    let frontier = recovery
        .authenticated_frontier_anchor()
        .ok_or_else(|| error("USTE_BM06_PACKED_FRONTIER"))?
        .0
        .get();
    if (phase == "recover" && frontier != profile.frontier())
        || (phase == "tail" && frontier != profile.checkpoint_revision())
        || (frontier != profile.frontier() && frontier != profile.checkpoint_revision())
    {
        return Err(error("USTE_BM06_PACKED_FRONTIER"));
    }
    let mut origin_groups = None;
    if phase == "rebuild" {
        let (live, report) = recover_packed_graph_origin(
            recovery,
            &mut fs,
            retention()?,
            CoordinatorRecoveryLimits::new(0, 0).map_err(|_| error("USTE_BM06_LIMITS"))?,
            limits.origin,
        )
        .map_err(|_| error("USTE_BM06_PACKED_REBUILD"))?;
        if report.suffix.journal.groups != frontier - 1 || live.overlay_counts() != (0, 0) {
            return Err(error("USTE_BM06_PACKED_REBUILD"));
        }
        origin_groups = Some(report.suffix.journal.groups);
        drop(live);
        recovery = open(&mut fs, &mut adapter, limits)?.0;
    }
    let base = if phase == "recover" {
        engine::prefix::latest_revision(&mut fs, &recovery, limits)
            .map_err(|_| error("USTE_BM06_PACKED_PREFIX"))?
    } else {
        uste_types::CommitRevision::new(frontier).map_err(|_| error("USTE_BM06_PACKED_FRONTIER"))?
    };
    let (mut live, _, suffix) = engine::admit_at(
        &mut fs,
        recovery,
        limits,
        Some(base),
        counts(profile, base.get())?,
    )
    .map_err(|_| error("USTE_BM06_PACKED_ADMISSION"))?;
    let versions = if frontier == profile.frontier() {
        VERSIONS
    } else {
        VERSIONS - 1
    };
    let verified =
        engine::recovery::verify_history(&live, &mut fs, &kernel, &principal, profile, versions)
            .map_err(|_| error("USTE_BM06_PACKED_HISTORY"))?;
    if phase == "recover" {
        engine::commit_batch(
            &mut live,
            &mut fs,
            &mut kernel,
            &principal,
            batch(profile, profile.frontier())?,
            &mut SystemClock::new(),
            limits,
        )
        .map_err(|_| error("USTE_BM06_PACKED_RETRY"))?;
        if live.overlay_counts() != (0, 0) {
            return Err(error("USTE_BM06_PACKED_RETRY"));
        }
    }
    if phase == "tail" {
        let request = batch(profile, profile.frontier())?;
        let bytes = encode_transaction(&GraphTransaction::new(scope(), request.operations))
            .map_err(|_| error("USTE_BM06_PACKED_BATCH"))?;
        let mut publication = limits.publication;
        publication.stage.maximum_batches = 1;
        let mut writer =
            AuthorizedPackedWriter::new(&mut live, &mut kernel, limits.preparation, publication)
                .map_err(|_| error("USTE_BM06_PACKED_WRITER"))?;
        match writer.commit(
            &mut fs,
            &principal,
            AuthorizedTransactionRequest {
                idempotency_key: request.idempotency_key,
                transaction_id: request.transaction_id,
                canonical_request: &bytes,
                blob_inventory: None,
            },
            &mut SystemClock::new(),
            &NeverCancel,
        ) {
            Err(uste_txn::AuthorizedDiskWriteError::CommittedPublication {
                outcome,
                error:
                    uste_txn::TransactionError::Storage(
                        uste_storage::journal::StorageError::ResourceLimit,
                    ),
            }) if outcome.revision.get() == profile.frontier() => (),
            _ => return Err(error("USTE_BM06_PACKED_EXPECTED_TAIL")),
        }
    }
    drop(live);
    // A tail intentionally has no terminal triple and therefore no terminal digest claim.
    let digest = if phase == "tail" {
        None
    } else {
        let recovery = open(&mut fs, &mut adapter, limits)?.0;
        let mut selected = limits;
        selected.counts = counts(profile, frontier)?;
        Some(hex(&engine::admit(&mut fs, recovery, selected)
            .map_err(|_| error("USTE_BM06_PACKED_ADMISSION"))?
            .1))
    };
    let (rss, peak) = process_rss()?;
    Ok(serde_json::json!({
        "schema": "bm06-linux-packed-development-v1", "phase": phase, "engine_benchmark": false,
        "qualification": "nonqualifying-development-profile", "qualifying_recovery_trials": 0,
        "filesystem_profile": "linux-x86_64-btrfs", "storage_metadata_mode": "disk-certificate-and-blob-recovery",
        "full_memory_graph_state": false, "full_memory_coordinator_metadata": false,
        "complete_authenticated_io": false, "kernel_filesystem_device_cache": "uncontrolled",
        "records": profile.records(), "frontier": if phase == "tail" { profile.frontier() } else { frontier },
        "selected_base_revision": base.get(), "suffix_groups": suffix, "origin_suffix_groups": origin_groups,
        "verified_history_versions": verified, "history_verified_through_revision": frontier, "v1_state_digest": digest,
        "derived_terminal_pending": phase == "tail", "incomplete_prefix_resume_implemented": false,
        "elapsed_milliseconds": started.elapsed().as_millis(), "current_rss_kib": rss, "process_peak_rss_kib": peak,
        "repaired_certificate_tail_bytes": recovered.repaired_certificate_tail_bytes,
        "ignored_uncommitted_journal_bytes": recovered.ignored_uncommitted_journal_bytes,
        "adapter_io": fs.snapshot()?.json()?, "development_record_limit": MAX_RECORDS,
    }).to_string())
}
