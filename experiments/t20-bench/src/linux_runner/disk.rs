//! Native development disk pipeline; no full-state recovery fallback or qualification claim.
use super::*;
use crate::engine::disk::{
    DiskBatch, DiskProfileLimits, admit_development_disk, commit_batch, fixture_state_counts,
    visit_development_batches,
};
use uste_graph::{GraphDiskLiveState, GraphDiskReadLimits, GraphStateRootMergeLimits};
use uste_storage::{IndexGetLimits, IndexPredecessorLimits};
use uste_txn::{
    AuthenticatedIndexRecovery, AuthorizedDiskReader, CoordinatorMetadataRebaseLimits,
    CoordinatorRecoveryLimits, DiskCommitCoordinator,
};

type Disk = DiskCommitCoordinator<
    GraphDiskLiveState,
    LinuxFileSystem,
    RecoveryEnvelope,
    OsEntropy,
    OsEntropy,
>;

mod query;
mod sampling;
pub use query::query_correctness;
pub use sampling::sample_worker;

struct DiskSession {
    filesystem: LinuxFileSystem,
    coordinator: Disk,
    policy: PolicyKernel,
    principal: AuthenticatedPrincipal,
    frontier: u64,
    setup_elapsed: std::time::Duration,
}

/// These commands intentionally retain the existing development ceiling. Their limits and
/// measurements are not substituted for the accepted exact-size BM-01/BM-06 campaigns.
pub fn run(
    root: &Path,
    password_file: &Path,
    profile: Bm01Profile,
    phase: &str,
) -> Result<String, LinuxRunnerError> {
    prepare_session(root, password_file, profile, phase).map(|(_, report)| report)
}

fn prepare_session(
    root: &Path,
    password_file: &Path,
    profile: Bm01Profile,
    phase: &str,
) -> Result<(DiskSession, String), LinuxRunnerError> {
    prepare_session_with_observer(root, password_file, profile, phase, &mut |_| Ok(()))
}

/// Explicit process-loss harness. It never returns after the selected durable prefix marker.
pub fn create_crash_probe(
    root: &Path,
    password_file: &Path,
    profile: Bm01Profile,
    pause_after_revision: u64,
) -> Result<String, LinuxRunnerError> {
    use std::io::Write as _;
    let final_revision = materialization_revision_count(profile);
    if pause_after_revision == 0 || pause_after_revision >= final_revision {
        return Err(error("USTE_BM01_CRASH_PROBE_REVISION"));
    }
    let mut observer = |revision| {
        if revision == pause_after_revision {
            let mut output = std::io::stdout().lock();
            writeln!(output, "{}", crash_probe_marker(revision, final_revision))
                .and_then(|()| output.flush())
                .map_err(|_| error("USTE_BM01_CRASH_PROBE_SIGNAL"))?;
            loop {
                std::thread::park();
            }
        }
        Ok(())
    };
    let _ = prepare_session_with_observer(root, password_file, profile, "create", &mut observer)?;
    Err(error("USTE_BM01_CRASH_PROBE_MISSED"))
}

fn prepare_session_with_observer(
    root: &Path,
    password_file: &Path,
    profile: Bm01Profile,
    phase: &str,
    observer: &mut impl FnMut(u64) -> Result<(), LinuxRunnerError>,
) -> Result<(DiskSession, String), LinuxRunnerError> {
    if profile.entities() > crate::engine::MAX_DEVELOPMENT_ENTITIES {
        return Err(error("USTE_BM01_DISK_DEVELOPMENT_LIMIT"));
    }
    if !matches!(phase, "create" | "resume" | "open") {
        return Err(error("USTE_BM01_DISK_PHASE"));
    }
    let limits = DiskProfileLimits::new(profile).map_err(|_| error("USTE_BM01_LIMITS"))?;
    let started = Instant::now();
    let mut fs = open_filesystem(root)?;
    let mut adapter = PortableRecoveryAdapter::new(credential::read_password(password_file)?);
    let name =
        EntryName::new("bm01-linux-disk-engine").map_err(|_| error("USTE_BM01_DATABASE_NAME"))?;
    if phase == "create" {
        let vault = KeyVault::create(scope().database(), &mut adapter, OsEntropy)
            .map_err(|_| error("USTE_BM01_KEY_CREATE"))?;
        let raw = CommitCoordinator::create(
            &mut fs,
            scope(),
            retention()?,
            name.clone(),
            vault,
            OsEntropy,
            GraphState::new(scope()),
        )
        .map_err(|_| error("USTE_BM01_DATABASE_CREATE"))?;
        bootstrap(raw, &mut fs, observer)?;
    }
    let (recovery, report, frontier) = AuthenticatedIndexRecovery::open_with_frontier_transaction(
        &mut fs,
        &name,
        scope(),
        OsEntropy,
        OsEntropy,
        &mut adapter,
    )
    .map_err(|_| error("USTE_BM01_DATABASE_OPEN"))?;
    let recovered_revision = report.frontier.map_or(0, |revision| revision.get());
    if recovered_revision > materialization_revision_count(profile) {
        return Err(error("USTE_BM01_FRONTIER_MISMATCH"));
    }
    // Only a verified zero/one-revision prefix may reconstruct a tiny policy-only GraphState.
    // All larger prefixes must use disk admission, even if their derived roots are missing.
    let (recovery, frontier) = if phase == "resume" && recovered_revision <= 1 {
        let raw = recovery
            .into_bounded_coordinator(
                &mut fs,
                GraphState::new(scope()),
                retention()?,
                CoordinatorRecoveryLimits::new(1, 0).map_err(|_| error("USTE_BM01_LIMITS"))?,
                1_048_576,
            )
            .map_err(|_| error("USTE_BM01_BOOTSTRAP_RECOVERY"))?;
        bootstrap(raw, &mut fs, observer)?;
        let (recovery, _, frontier) = AuthenticatedIndexRecovery::open_with_frontier_transaction(
            &mut fs,
            &name,
            scope(),
            OsEntropy,
            OsEntropy,
            &mut adapter,
        )
        .map_err(|_| error("USTE_BM01_DATABASE_OPEN"))?;
        (recovery, frontier)
    } else {
        (recovery, frontier)
    };
    let frontier = frontier.ok_or_else(|| error("USTE_BM01_FRONTIER_MISSING"))?;
    let outcome = frontier.outcome();
    let (mut disk, admission) = admit_development_disk(&mut fs, recovery, frontier, limits)
        .map_err(|_| error("USTE_BM01_DISK_ADMISSION"))?;
    if phase == "open"
        && (disk
            .state()
            .map_err(|_| error("USTE_BM01_DISK_STATE"))?
            .is_pending()
            || disk.overlay_counts() != (0, 0)
            || disk.rebase_required())
    {
        return Err(error("USTE_BM01_DISK_REPAIR_REQUIRED"));
    }
    if phase != "open" {
        let (read, merge) = (limits.merge_read, limits.merge);
        if disk
            .state()
            .map_err(|_| error("USTE_BM01_DISK_STATE"))?
            .is_pending()
        {
            uste_graph::publish_graph_disk_coordinator_base(
                &mut disk,
                &mut fs,
                outcome,
                GraphStateRootMergeLimits::uniform(merge, 64 * 1024)
                    .map_err(|_| error("USTE_BM01_LIMITS"))?,
            )
            .map_err(|_| error("USTE_BM01_DISK_ROOT_REPAIR"))?;
        }
        disk.rebase_metadata(
            &mut fs,
            CoordinatorMetadataRebaseLimits { merge, reuse: read },
        )
        .map_err(|_| error("USTE_BM01_DISK_METADATA_REPAIR"))?;
    }
    let mut policy =
        kernel(benchmark_policy(scope()).map_err(|_| error("USTE_BM01_POLICY_PROFILE"))?)
            .map_err(|_| error("USTE_BM01_POLICY_PROFILE"))?;
    let principal = authenticate(&policy)?;
    if outcome.revision.get() >= 2 {
        require_binding(&disk, &mut fs, &policy, &principal, profile)?;
    }
    if phase != "open" {
        let mut clock = SystemClock::new();
        visit_development_batches(profile, |sequence, operations| {
            commit_batch(
                &mut disk,
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
            )?;
            observer(sequence).map_err(|error| error.code().to_owned())
        })
        .map_err(|_| error("USTE_BM01_DISK_MATERIALIZE"))?;
    }
    let frontier = disk
        .checkpoint_anchor()
        .map_err(|_| error("USTE_BM01_DISK_STATE"))?
        .ok_or_else(|| error("USTE_BM01_FRONTIER_MISSING"))?
        .0
        .get();
    require_expected_frontier(profile, frontier)?;
    require_binding(&disk, &mut fs, &policy, &principal, profile)?;
    let final_counts = disk
        .state()
        .map_err(|_| error("USTE_BM01_DISK_STATE"))?
        .current_base()
        .ok_or_else(|| error("USTE_BM01_DISK_REPAIR_REQUIRED"))?
        .state_counts();
    if final_counts != fixture_state_counts(profile) {
        return Err(error("USTE_BM01_FIXTURE_CARDINALITY"));
    }
    let admission_json = serde_json::json!({
        "measurement_scope": "cold-graph-semantic-admission-only",
        "complete_authenticated_io": false,
        "graph_revision": admission.graph_revision,
        "metadata_revision": admission.metadata_revision,
        "state_counts": admission.state_counts,
        "scan_runs": admission.graph.scan.runs,
        "scan_entries": admission.graph.scan.entries,
        "scan_logical_bytes": admission.graph.scan.logical_bytes,
        "scan_pages_read": admission.graph.scan.pages_read,
        "exact_lookups": admission.graph.exact_lookups,
        "predecessor_lookups": admission.graph.predecessor_lookups,
        "semantic_reference_visits": admission.graph.semantic_reference_visits,
        "lookup_page_visits_including_cache_hits": admission.graph.lookup_page_visits,
        "lookup_result_bytes": admission.graph.lookup_result_bytes,
        "peak_history_group_logical_bytes": admission.graph.peak_history_group_logical_bytes,
    });
    let report = format!(
        concat!(
            "{{\"schema\":\"bm01-linux-disk-development-v1\",",
            "\"engine_benchmark\":false,\"qualification\":\"nonqualifying-development-profile\",",
            "\"filesystem_profile\":\"linux-x86_64-btrfs\",\"phase\":\"{}\",",
            "\"full_memory_graph_state\":false,\"full_memory_coordinator_metadata\":false,",
            "\"storage_metadata_memory_resident\":true,\"entities\":{},\"relationships\":{},",
            "\"frontier\":{},\"recovered_revision\":{},\"elapsed_milliseconds\":{},",
            "\"repaired_certificate_tail_bytes\":{},\"ignored_uncommitted_journal_bytes\":{},",
            "\"cold_admission\":{},\"final_state_counts\":{}}}"
        ),
        phase,
        profile.entities(),
        profile.relationships(),
        frontier,
        recovered_revision,
        started.elapsed().as_millis(),
        report.repaired_certificate_tail_bytes,
        report.ignored_uncommitted_journal_bytes,
        admission_json,
        serde_json::json!(final_counts)
    );
    Ok((
        DiskSession {
            filesystem: fs,
            coordinator: disk,
            policy,
            principal,
            frontier,
            setup_elapsed: started.elapsed(),
        },
        report,
    ))
}

fn bootstrap(
    mut raw: LinuxRaw,
    fs: &mut LinuxFileSystem,
    observer: &mut impl FnMut(u64) -> Result<(), LinuxRunnerError>,
) -> Result<(), LinuxRunnerError> {
    install_policy(&mut raw, fs)?;
    observer(1)?;
    let snapshot = raw
        .read_view()
        .map_err(|_| error("USTE_BM01_READ_VIEW"))?
        .state()
        .clone();
    uste_graph::publish_graph_state_root(&mut raw, fs, &snapshot)
        .map_err(|_| error("USTE_BM01_BOOTSTRAP_ROOT"))?;
    uste_txn::publish_coordinator_metadata_root(&mut raw, fs)
        .map_err(|_| error("USTE_BM01_BOOTSTRAP_ROOT"))?;
    uste_txn::publish_coordinator_transaction_index(&mut raw, fs)
        .map_err(|_| error("USTE_BM01_BOOTSTRAP_ROOT"))?;
    Ok(())
}

fn install_policy(raw: &mut LinuxRaw, fs: &mut LinuxFileSystem) -> Result<(), LinuxRunnerError> {
    // Recovery must be an exact retry, never a fresh policy append to an unrelated prefix.
    if let Some((revision, _)) = raw
        .checkpoint_anchor()
        .map_err(|_| error("USTE_BM01_DISK_STATE"))?
    {
        let mut outcomes = raw.checkpoint_outcomes();
        let Some((principal, key, outcome)) = outcomes.next() else {
            return Err(error("USTE_BM01_BOOTSTRAP_PROFILE"));
        };
        if revision.get() != 1
            || outcome.revision != revision
            || principal != PRINCIPAL
            || key != identity(1, IdempotencyKey::from_bytes)
            || outcome.transaction_id != identity(1, TransactionId::from_bytes)
            || outcomes.next().is_some()
        {
            return Err(error("USTE_BM01_BOOTSTRAP_PROFILE"));
        }
    }
    let policy = benchmark_policy(scope()).map_err(|_| error("USTE_BM01_POLICY_PROFILE"))?;
    let install = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        Vec::new(),
        uste_graph::DurablePolicyMutation::Install { policy },
    ))
    .map_err(|_| error("USTE_BM01_POLICY_ENCODE"))?;
    let outcome = raw
        .commit(
            fs,
            TransactionRequest {
                principal: PRINCIPAL,
                idempotency_key: identity(1, IdempotencyKey::from_bytes),
                transaction_id: identity(1, TransactionId::from_bytes),
                canonical_request: &install,
                blob_inventory: None,
            },
            &mut SystemClock::new(),
            &NeverCancel,
        )
        .map_err(|_| error("USTE_BM01_POLICY_COMMIT"))?;
    if outcome.revision.get() != 1 {
        return Err(error("USTE_BM01_REVISION_MISMATCH"));
    }
    Ok(())
}

fn require_binding(
    disk: &Disk,
    fs: &mut LinuxFileSystem,
    policy: &PolicyKernel,
    principal: &AuthenticatedPrincipal,
    profile: Bm01Profile,
) -> Result<(), LinuxRunnerError> {
    let reader = AuthorizedDiskReader::new(
        disk,
        policy,
        GraphDiskReadLimits {
            current: IndexGetLimits::new(64, 16 * 1024).map_err(|_| error("USTE_BM01_LIMITS"))?,
            historical: IndexPredecessorLimits::new(64, 16 * 1024)
                .map_err(|_| error("USTE_BM01_LIMITS"))?,
            expansion: None,
        },
    )
    .map_err(|_| error("USTE_BM01_AUTHORIZED_OPEN"))?;
    let result = reader
        .read(
            fs,
            principal,
            &GraphReadRequest::Record {
                id: evidence_ref(scope()),
            },
            &NeverCancel,
        )
        .map_err(|_| error("USTE_BM01_PROFILE_BINDING"))?;
    let GraphReadOutput::Record(Some(record)) = result else {
        return Err(error("USTE_BM01_PROFILE_BINDING"));
    };
    let Record::Evidence(evidence) = *record else {
        return Err(error("USTE_BM01_PROFILE_BINDING"));
    };
    if evidence.id != evidence_ref(scope())
        || evidence.version != RecordVersion::FIRST
        || evidence.created_revision.get() != 2
        || evidence.locator.as_str() != "bm01-uste-graph-v1"
        || evidence.digest != engine_mapping_digest(profile)
    {
        return Err(error("USTE_BM01_PROFILE_BINDING"));
    }
    Ok(())
}

fn error(code: &'static str) -> LinuxRunnerError {
    LinuxRunnerError::new(code)
}

#[cfg(test)]
#[path = "disk_tests.rs"]
mod tests;
