//! Native small BM-06 checkpoint/tail pipeline; no larger-than-memory qualification.
use super::*;
use crate::recovery_materialization::Bm06Profile;
const HISTORY_DATABASE: &str = "bm06-linux-packed-engine";
const MAX_RECORDS: u64 = 8192;
mod bootstrap;
mod continuation;
mod measurement;
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
    limits: Limits,
) -> Result<(), LinuxRunnerError> {
    let first = uste_types::CommitRevision::FIRST;
    let mut cursor = recovery
        .open_transaction_cursor(first, first, 1, binding_budget(limits)?)
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
fn binding_budget(limits: Limits) -> Result<u64, LinuxRunnerError> {
    // The small policy group still authenticates its certificate through the current frontier.
    // Charge that bounded distance separately from the policy group's original 1 MiB allowance.
    limits
        .origin
        .suffix
        .graph
        .certificates
        .maximum_encoded_bytes()
        .checked_add(1_048_576)
        .ok_or_else(|| error("USTE_BM06_LIMITS"))
}
// Native admission stays separately capped; prefix arithmetic also supports partial generations.
fn counts(profile: Bm06Profile, revision: u64) -> Result<[u64; 8], LinuxRunnerError> {
    if profile.records() > MAX_RECORDS || revision == 0 || revision > profile.frontier() {
        return Err(error("USTE_BM06_PACKED_FRONTIER"));
    }
    profile
        .prefix_state_counts(revision)
        .map_err(|_| error("USTE_BM06_PACKED_FRONTIER"))
}

pub fn run(
    root: &Path,
    password: &Path,
    profile: Bm06Profile,
    phase: &str,
) -> Result<String, LinuxRunnerError> {
    run_observed(root, password, profile, phase, &mut |_| Ok(()))
}

fn park_after_marker(revision: u64) -> Result<(), LinuxRunnerError> {
    use std::io::Write;
    let mut output = std::io::stdout().lock();
    writeln!(
        output,
        "{{\"schema\":\"bm06-packed-durable-prefix-v1\",\"frontier\":{revision}}}"
    )
    .and_then(|()| output.flush())
    .map_err(|_| error("USTE_BM06_PACKED_PROBE_SIGNAL"))?;
    loop {
        std::thread::park();
    }
}

/// Test-only owned-child control, including empty and policy-only bootstrap frontiers.
pub fn create_crash_probe(
    root: &Path,
    password: &Path,
    profile: Bm06Profile,
    pause: u64,
) -> Result<String, LinuxRunnerError> {
    if profile.records() > MAX_RECORDS {
        return Err(error("USTE_BM06_PACKED_DEVELOPMENT_LIMIT"));
    }
    if pause > profile.checkpoint_revision() {
        return Err(error("USTE_BM06_PACKED_PROBE_REVISION"));
    }
    run_observed(root, password, profile, "create", &mut |revision| {
        if revision == pause {
            park_after_marker(revision)?;
        }
        Ok(())
    })
}

/// Owned-child control at graph publication before intermediate metadata rebase, or terminal acknowledgement.
pub fn tail_prefix_crash_probe(
    root: &Path,
    password: &Path,
    profile: Bm06Profile,
    pause: u64,
) -> Result<String, LinuxRunnerError> {
    if profile.records() > MAX_RECORDS {
        return Err(error("USTE_BM06_PACKED_DEVELOPMENT_LIMIT"));
    }
    if pause <= profile.checkpoint_revision() || pause > profile.frontier() {
        return Err(error("USTE_BM06_PACKED_PROBE_REVISION"));
    }
    run_observed(root, password, profile, "tail", &mut |revision| {
        if revision == pause {
            park_after_marker(revision)?;
        }
        Ok(())
    })
}

fn run_observed(
    root: &Path,
    password: &Path,
    profile: Bm06Profile,
    phase: &str,
    observer: &mut impl FnMut(u64) -> Result<(), LinuxRunnerError>,
) -> Result<String, LinuxRunnerError> {
    run_bounded_observed(root, password, profile, phase, None, observer)
}

/// Finish an explicit complete-generation construction prefix, never the final update generation.
pub fn construct_prefix(
    root: &Path,
    password: &Path,
    profile: Bm06Profile,
    target: u64,
    create: bool,
) -> Result<String, LinuxRunnerError> {
    run_bounded_observed(
        root,
        password,
        profile,
        if create {
            "create-prefix"
        } else {
            "resume-prefix"
        },
        Some(target),
        &mut |_| Ok(()),
    )
}

fn run_bounded_observed(
    root: &Path,
    password: &Path,
    profile: Bm06Profile,
    phase: &str,
    target: Option<u64>,
    observer: &mut impl FnMut(u64) -> Result<(), LinuxRunnerError>,
) -> Result<String, LinuxRunnerError> {
    if profile.records() > MAX_RECORDS {
        return Err(error("USTE_BM06_PACKED_DEVELOPMENT_LIMIT"));
    }
    if !matches!(
        phase,
        "create"
            | "create-prefix"
            | "resume-prefix"
            | "open"
            | "tail"
            | "recover"
            | "recover-checkpoint"
            | "rebuild"
            | "resume"
            | "tail-crash-probe"
    ) {
        return Err(error("USTE_BM06_PACKED_PHASE"));
    }
    if matches!(phase, "create-prefix" | "resume-prefix") != target.is_some() {
        return Err(error("USTE_BM06_PACKED_PREFIX_TARGET"));
    }
    if target.is_some_and(|target| {
        target == 0
            || target > profile.checkpoint_revision()
            || !(target - 1).is_multiple_of(profile.batches_per_version())
    }) {
        return Err(error("USTE_BM06_PACKED_PREFIX_TARGET"));
    }
    let is_create = matches!(phase, "create" | "create-prefix");
    let is_resume = matches!(phase, "resume" | "resume-prefix");
    let is_tail = matches!(phase, "tail" | "tail-crash-probe");
    let is_checkpoint_recovery = phase == "recover-checkpoint";
    let is_recovery = phase == "recover" || is_checkpoint_recovery;
    let started = Instant::now();
    let mut fs = ObservedFileSystem::new(open_filesystem(root)?);
    let mut adapter = PortableRecoveryAdapter::new(credential::read_password(password)?);
    let limits = buffered_limits(Limits::recovery(profile).map_err(|_| error("USTE_BM06_LIMITS"))?);
    if is_create {
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
        observer(0)?;
        bootstrap::install(&mut raw, &mut fs, profile)?;
        observer(1)?;
        drop(raw);
    }
    let (mut recovery, recovered) = open(&mut fs, &mut adapter, limits)?;
    let recovered_frontier = recovered.frontier.map_or(0, |revision| revision.get());
    let bootstrap_resume = is_resume && recovered_frontier <= 1;
    if bootstrap_resume {
        recovery = bootstrap::resume(recovery, &mut fs, &mut adapter, profile, limits)?;
    }
    binding(&mut fs, &mut recovery, profile, limits)?;
    let mut kernel =
        kernel(crate::engine::recovery::recovery_policy().map_err(|_| error("USTE_BM06_POLICY"))?)
            .map_err(|_| error("USTE_BM06_POLICY"))?;
    let principal = authenticate(&kernel)?;
    let mut resume_base_revision = None;
    let mut resume_suffix_groups = None;
    let mut construction_nonce_session = None;
    if is_create || is_resume {
        if is_create && recovered_frontier != 1 {
            return Err(error("USTE_BM06_PACKED_FRONTIER"));
        }
        let (live, base, groups) = continuation::complete(
            recovery,
            &mut fs,
            profile,
            limits,
            &mut kernel,
            &principal,
            target,
            observer,
        )?;
        if is_resume {
            resume_base_revision = Some(base);
            resume_suffix_groups = Some(groups);
        }
        let nonces = live
            .vault_nonce_report()
            .map_err(|_| error("USTE_BM06_NONCE_REPORT"))?;
        construction_nonce_session = Some(serde_json::json!({
            "measurement_scope": "construction-owner-only",
            "includes_bootstrap_owner": false, "includes_other_vaults": false,
            "includes_key_adapter": false, "memory_bytes_measured": false,
            "writer_rotation_implemented": false,
            "issued_nonces": nonces.issued_nonces,
            "nonce_limit": nonces.nonce_limit, "remaining_nonces": nonces.remaining_nonces,
        }));
        drop(live);
        recovery = open(&mut fs, &mut adapter, limits)?.0;
    }
    let frontier = recovery
        .authenticated_frontier_anchor()
        .ok_or_else(|| error("USTE_BM06_PACKED_FRONTIER"))?
        .0
        .get();
    counts(profile, frontier)?;
    if (is_recovery && frontier != profile.frontier())
        || (is_tail && frontier != profile.checkpoint_revision())
        || (phase != "rebuild"
            && frontier != profile.frontier()
            && frontier != profile.checkpoint_revision()
            && Some(frontier) != target)
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
    let base = if is_checkpoint_recovery {
        uste_types::CommitRevision::new(profile.checkpoint_revision())
            .map_err(|_| error("USTE_BM06_PACKED_FRONTIER"))?
    } else if is_recovery {
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
    if is_checkpoint_recovery && suffix != profile.batches_per_version() {
        return Err(error("USTE_BM06_PACKED_CHECKPOINT_TAIL"));
    }
    let admission_elapsed = started.elapsed();
    let admission_io = fs.snapshot()?;
    let verified = engine::recovery::verify_prefix_history(
        &live, &mut fs, &kernel, &principal, profile, frontier,
    )
    .map_err(|_| error("USTE_BM06_PACKED_HISTORY"))?;
    let verification_elapsed = started.elapsed();
    let verification_io = fs.snapshot()?;
    if is_recovery {
        for sequence in profile.checkpoint_revision() + 1..=profile.frontier() {
            engine::commit_batch(
                &mut live,
                &mut fs,
                &mut kernel,
                &principal,
                batch(profile, sequence)?,
                &mut SystemClock::new(),
                limits,
            )
            .map_err(|_| error("USTE_BM06_PACKED_RETRY"))?;
        }
        if live.overlay_counts() != (0, 0) {
            return Err(error("USTE_BM06_PACKED_RETRY"));
        }
    }
    if is_tail {
        engine::recovery::certify_generation_tail_observed(
            &mut live,
            &mut fs,
            &mut kernel,
            &principal,
            profile,
            crate::recovery_materialization::VERSIONS - 1,
            limits,
            &mut SystemClock::new(),
            &mut |sequence| batch(profile, sequence).map_err(|error| error.code().to_string()),
            &mut |revision| observer(revision).map_err(|error| error.code().to_string()),
        )
        .map_err(|_| error("USTE_BM06_PACKED_EXPECTED_TAIL"))?;
        if phase == "tail-crash-probe" {
            park_after_marker(profile.frontier())?;
        }
    }
    drop(live);
    let post_verification_elapsed = started.elapsed();
    let post_verification_io = fs.snapshot()?;
    // A tail intentionally has no terminal triple and therefore no terminal digest claim.
    let digest = if is_tail {
        None
    } else {
        let recovery = open(&mut fs, &mut adapter, limits)?.0;
        let mut selected = limits;
        selected.counts = counts(profile, frontier)?;
        Some(hex(&engine::admit(&mut fs, recovery, selected)
            .map_err(|_| error("USTE_BM06_PACKED_ADMISSION"))?
            .1))
    };
    let terminal_elapsed = started.elapsed();
    let terminal_io = fs.snapshot()?;
    let phase_work = measurement::report(
        [
            admission_elapsed,
            verification_elapsed,
            post_verification_elapsed,
            terminal_elapsed,
        ],
        [
            admission_io,
            verification_io,
            post_verification_io,
            terminal_io,
        ],
    )?;
    let (rss, peak) = process_rss()?;
    let mut report = serde_json::json!({
        "schema": "bm06-linux-packed-development-v1", "phase": phase, "engine_benchmark": false,
        "qualification": "nonqualifying-development-profile", "qualifying_recovery_trials": 0,
        "filesystem_profile": "linux-x86_64-btrfs", "storage_metadata_mode": "disk-certificate-and-blob-recovery",
        "full_memory_graph_state": false, "full_memory_coordinator_metadata": false,
        "graph_admission_cache_bytes": engine::GRAPH_ADMISSION_CACHE_BYTES,
        "graph_admission_cache_scope": engine::GRAPH_ADMISSION_CACHE_SCOPE,
        "coordinator_admission_buffered": true,
        "coordinator_admission_cache_bytes": engine::COORDINATOR_ADMISSION_CACHE_BYTES,
        "coordinator_admission_cache_scope": engine::COORDINATOR_ADMISSION_CACHE_SCOPE,
        "complete_authenticated_io": false, "kernel_filesystem_device_cache": "uncontrolled",
        "records": profile.records(), "frontier": if is_tail { profile.frontier() } else { frontier },
        "construction_target_revision": target,
        "construction_nonce_session": construction_nonce_session,
        "phase_work": phase_work,
        "selected_base_revision": base.get(), "suffix_groups": suffix, "origin_suffix_groups": origin_groups,
        "checkpoint_tail_replay": is_checkpoint_recovery,
        "verified_history_versions": verified, "history_verified_through_revision": frontier, "v1_state_digest": digest,
        "derived_terminal_pending": is_tail, "incomplete_prefix_resume_implemented": true,
        "recovered_frontier": recovered_frontier, "bounded_bootstrap_resume": bootstrap_resume,
        "resume_base_revision": resume_base_revision, "resume_suffix_groups": resume_suffix_groups,
        "elapsed_milliseconds": started.elapsed().as_millis(), "current_rss_kib": rss, "process_peak_rss_kib": peak,
        "repaired_certificate_tail_bytes": recovered.repaired_certificate_tail_bytes,
        "ignored_uncommitted_journal_bytes": recovered.ignored_uncommitted_journal_bytes,
        "adapter_io": fs.snapshot()?.json()?, "development_record_limit": MAX_RECORDS,
    });
    report["staging_cache_bytes"] = serde_json::json!(STAGING_CACHE_BYTES);
    report["staging_cache_scope"] = serde_json::json!(STAGING_CACHE_SCOPE);
    report["proof_cache_bytes"] = serde_json::json!(PROOF_CACHE_BYTES);
    report["proof_cache_scope"] = serde_json::json!(PROOF_CACHE_SCOPE);
    Ok(report.to_string())
}
