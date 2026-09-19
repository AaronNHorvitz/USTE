//! Separate-process BM-06 development phases, not a reserved-host qualification campaign.
use super::*;
use crate::{
    engine::recovery::{recovery_policy, verify_history},
    recovery_materialization::{Bm06Profile, VERSIONS},
};

const MAX_NATIVE_RECORDS: u64 = 2;

pub fn run(
    root: &Path,
    password_file: &Path,
    profile: Bm06Profile,
    phase: &str,
) -> Result<String, LinuxRunnerError> {
    run_observed(root, password_file, profile, phase, &mut |_| Ok(()))
}

pub fn create_crash_probe(
    root: &Path,
    password_file: &Path,
    profile: Bm06Profile,
    pause: u64,
) -> Result<String, LinuxRunnerError> {
    if profile.records() > MAX_NATIVE_RECORDS {
        return Err(error("USTE_BM06_DEVELOPMENT_LIMIT"));
    }
    if pause == 0 || pause > profile.checkpoint_revision() {
        return Err(error("USTE_BM06_PROBE_REVISION"));
    }
    run_observed(root, password_file, profile, "create", &mut |revision| {
        if revision == pause {
            use std::io::Write;
            let mut output = std::io::stdout().lock();
            writeln!(
                output,
                "{{\"schema\":\"bm06-durable-prefix-v1\",\"frontier\":{revision}}}"
            )
            .and_then(|()| output.flush())
            .map_err(|_| error("USTE_BM06_PROBE_SIGNAL"))?;
            loop {
                std::thread::park();
            }
        }
        Ok(())
    })
}

fn run_observed(
    root: &Path,
    password_file: &Path,
    profile: Bm06Profile,
    phase: &str,
    observer: &mut impl FnMut(u64) -> Result<(), LinuxRunnerError>,
) -> Result<String, LinuxRunnerError> {
    if profile.records() > MAX_NATIVE_RECORDS {
        return Err(error("USTE_BM06_DEVELOPMENT_LIMIT"));
    }
    if !matches!(
        phase,
        "create" | "resume" | "tail" | "recover" | "open" | "tail-crash-probe"
    ) {
        return Err(error("USTE_BM06_PHASE"));
    }
    let start = Instant::now();
    let limits = DiskProfileLimits::recovery(profile).map_err(|_| error("USTE_BM06_LIMITS"))?;
    let mut fs = io::ObservedFileSystem::new(open_filesystem(root)?);
    let mut adapter = PortableRecoveryAdapter::new(credential::read_password(password_file)?);
    let name = EntryName::new("bm06-linux-disk-engine").map_err(|_| error("USTE_BM06_NAME"))?;
    let policy = recovery_policy().map_err(|_| error("USTE_BM06_POLICY"))?;
    let mut clock = SystemClock::new();
    if phase == "create" {
        let vault = KeyVault::create(scope().database(), &mut adapter, OsEntropy)
            .map_err(|_| error("USTE_BM06_KEY_CREATE"))?;
        let raw = CommitCoordinator::create(
            &mut fs,
            scope(),
            retention()?,
            name.clone(),
            vault,
            OsEntropy,
            GraphState::new(scope()),
        )
        .map_err(|_| error("USTE_BM06_CREATE"))?;
        bootstrap(raw, &mut fs, profile, &mut clock, observer)?;
    }
    let (recovery, storage_report, frontier) =
        AuthenticatedIndexRecovery::open_with_disk_blob_metadata(
            &mut fs,
            &name,
            scope(),
            OsEntropy,
            OsEntropy,
            &mut adapter,
            limits.blob_recovery,
            &mut uste_storage::PageCache::new(64 * 1024 * 1024)
                .map_err(|_| error("USTE_BM06_LIMITS"))?,
        )
        .map_err(|_| error("USTE_BM06_OPEN"))?;
    let (recovery, frontier) = if phase == "resume"
        && storage_report
            .frontier
            .is_none_or(|revision| revision.get() <= 1)
    {
        let raw = recovery
            .into_bounded_coordinator(
                &mut fs,
                GraphState::new(scope()),
                retention()?,
                CoordinatorRecoveryLimits::new(1, 0).map_err(|_| error("USTE_BM06_LIMITS"))?,
                1_048_576,
            )
            .map_err(|_| error("USTE_BM06_BOOTSTRAP_RECOVERY"))?;
        bootstrap(raw, &mut fs, profile, &mut clock, observer)?;
        let (recovery, _, frontier) = AuthenticatedIndexRecovery::open_with_disk_blob_metadata(
            &mut fs,
            &name,
            scope(),
            OsEntropy,
            OsEntropy,
            &mut adapter,
            limits.blob_recovery,
            &mut uste_storage::PageCache::new(64 * 1024 * 1024)
                .map_err(|_| error("USTE_BM06_LIMITS"))?,
        )
        .map_err(|_| error("USTE_BM06_OPEN"))?;
        (recovery, frontier)
    } else {
        (recovery, frontier)
    };
    let frontier = frontier.ok_or_else(|| error("USTE_BM06_FRONTIER"))?;
    let recovered_revision = frontier.revision().get();
    let expected = match phase {
        "create" => 1,
        "resume" if recovered_revision <= profile.frontier() => recovered_revision,
        "tail" | "tail-crash-probe" => profile.checkpoint_revision(),
        _ => profile.frontier(),
    };
    if frontier.revision().get() != expected {
        return Err(error("USTE_BM06_FRONTIER"));
    }
    let (mut disk, admission) = admit_development_disk(
        &mut fs,
        recovery,
        frontier,
        limits,
        matches!(phase, "recover" | "resume"),
    )
    .map_err(|_| error("USTE_BM06_ADMISSION"))?;
    let mut kernel = kernel(policy).map_err(|_| error("USTE_BM06_POLICY"))?;
    let principal = authenticate(&kernel)?;
    if matches!(phase, "create" | "resume") {
        require_bootstrap_binding(&disk, &mut fs, &kernel, &principal, profile, &mut clock)?;
        if recovered_revision > 1 {
            verify_history(
                &disk,
                &mut fs,
                &kernel,
                &principal,
                profile,
                recovered_revision - 1,
            )
            .map_err(|_| error("USTE_BM06_HISTORY_ORACLE"))?;
        }
        disk.rebase_metadata(
            &mut fs,
            CoordinatorMetadataRebaseLimits {
                merge: limits.merge,
                reuse: limits.merge_read,
            },
        )
        .map_err(|_| error("USTE_BM06_METADATA_REPAIR"))?;
        for sequence in recovered_revision + 1..=profile.checkpoint_revision() {
            commit_batch(
                &mut disk,
                &mut fs,
                &mut kernel,
                &principal,
                batch(profile, sequence)?,
                &mut clock,
                limits,
            )
            .map_err(|_| error("USTE_BM06_MATERIALIZE"))?;
            observer(sequence)?;
        }
    } else if phase == "recover" {
        disk.rebase_metadata(
            &mut fs,
            CoordinatorMetadataRebaseLimits {
                merge: limits.merge,
                reuse: limits.merge_read,
            },
        )
        .map_err(|_| error("USTE_BM06_METADATA_REPAIR"))?;
        // Exact retry must resolve the certificate's outcome without another revision.
        commit_batch(
            &mut disk,
            &mut fs,
            &mut kernel,
            &principal,
            batch(profile, profile.frontier())?,
            &mut clock,
            limits,
        )
        .map_err(|_| error("USTE_BM06_EXACT_RETRY"))?;
    }
    let recovery_or_construction_ms = start.elapsed().as_millis();
    let before_verification_io = fs.snapshot()?;
    let verification_start = Instant::now();
    let versions = if matches!(phase, "create" | "tail" | "tail-crash-probe")
        || (phase == "resume" && recovered_revision < profile.frontier())
    {
        VERSIONS - 1
    } else {
        VERSIONS
    };
    let verified = verify_history(&disk, &mut fs, &kernel, &principal, profile, versions)
        .map_err(|_| error("USTE_BM06_HISTORY_ORACLE"))?;
    let verification_ms = verification_start.elapsed().as_millis();
    let verification_io = fs.snapshot()?.delta(before_verification_io)?.json()?;
    if matches!(phase, "tail" | "tail-crash-probe") {
        commit_unpublished_tail(
            &mut disk,
            &mut fs,
            &mut kernel,
            &principal,
            profile,
            &mut clock,
        )?;
        if phase == "tail-crash-probe" {
            use std::io::Write;
            let mut output = std::io::stdout().lock();
            writeln!(
                output,
                "{{\"schema\":\"bm06-durable-tail-v1\",\"frontier\":{},\"base_revision\":{}}}",
                profile.frontier(),
                profile.checkpoint_revision()
            )
            .and_then(|()| output.flush())
            .map_err(|_| error("USTE_BM06_PROBE_SIGNAL"))?;
            loop {
                std::thread::park();
            }
        }
    }
    let final_revision = disk
        .checkpoint_anchor()
        .map_err(|_| error("USTE_BM06_FRONTIER"))?
        .ok_or_else(|| error("USTE_BM06_FRONTIER"))?
        .0
        .get();
    Ok(serde_json::json!({
        "schema": "bm06-linux-development-v1", "phase": phase, "engine_benchmark": false,
        "qualification": "nonqualifying-native-development", "qualifying_recovery_trials": 0,
        "development_record_limit": MAX_NATIVE_RECORDS, "records": profile.records(),
        "filesystem_profile": "linux-x86_64-btrfs", "recovery_profile": "portable-argon2id-v1",
        "initial_graph_revision": admission.graph_revision, "initial_metadata_revision": admission.metadata_revision,
        "frontier": final_revision, "verified_history_versions": verified,
        "repaired_certificate_tail_bytes": storage_report.repaired_certificate_tail_bytes,
        "ignored_uncommitted_journal_bytes": storage_report.ignored_uncommitted_journal_bytes,
        "verified_history_revision": 1 + versions * profile.batches_per_version(),
        "verified_payload_bytes": verified * crate::recovery_materialization::PAYLOAD_BYTES as u64,
        "open_repair_or_construction_milliseconds": recovery_or_construction_ms,
        "history_verification_milliseconds": verification_ms,
        "total_elapsed_milliseconds": start.elapsed().as_millis(),
        "filesystem_adapter_io": fs.snapshot()?.json()?, "history_verification_adapter_io": verification_io,
        "complete_authenticated_io": false, "physical_device_io": false,
        "kernel_filesystem_device_cache": "uncontrolled", "uste_cache_budget_bytes": 64 * 1024 * 1024,
        "full_memory_graph_state": false, "full_memory_coordinator_metadata": false,
        "storage_metadata_memory_resident": disk.certificate_anchor_residency().0 || disk.blob_metadata_residency().0,
    }).to_string())
}

fn batch(profile: Bm06Profile, sequence: u64) -> Result<DiskBatch, LinuxRunnerError> {
    Ok(DiskBatch {
        sequence,
        idempotency_key: identity(sequence, IdempotencyKey::from_bytes),
        transaction_id: identity(sequence, TransactionId::from_bytes),
        operations: profile
            .batch(scope(), sequence)
            .map_err(|_| error("USTE_BM06_BATCH"))?,
    })
}

fn bootstrap_identity<T>(profile: Bm06Profile, construct: impl FnOnce([u8; 16]) -> T) -> T {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(b"BM06BT1\0");
    bytes[8..].copy_from_slice(&profile.records().to_be_bytes());
    construct(bytes)
}

fn bootstrap(
    mut raw: DiskRaw,
    fs: &mut DiskFileSystem,
    profile: Bm06Profile,
    clock: &mut SystemClock,
    observer: &mut impl FnMut(u64) -> Result<(), LinuxRunnerError>,
) -> Result<(), LinuxRunnerError> {
    let key = bootstrap_identity(profile, IdempotencyKey::from_bytes);
    let transaction = bootstrap_identity(profile, TransactionId::from_bytes);
    if let Some((revision, _)) = raw
        .checkpoint_anchor()
        .map_err(|_| error("USTE_BM06_BOOTSTRAP_PROFILE"))?
    {
        let mut outcomes = raw.checkpoint_outcomes();
        let Some((principal, stored_key, outcome)) = outcomes.next() else {
            return Err(error("USTE_BM06_BOOTSTRAP_PROFILE"));
        };
        if revision.get() != 1
            || outcome.revision != revision
            || principal != PRINCIPAL
            || stored_key != key
            || outcome.transaction_id != transaction
            || outcomes.next().is_some()
        {
            return Err(error("USTE_BM06_BOOTSTRAP_PROFILE"));
        }
    }
    let policy = recovery_policy().map_err(|_| error("USTE_BM06_POLICY"))?;
    let encoded = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        Vec::new(),
        uste_graph::DurablePolicyMutation::Install { policy },
    ))
    .map_err(|_| error("USTE_BM06_POLICY"))?;
    let outcome = raw
        .commit(
            fs,
            TransactionRequest {
                principal: PRINCIPAL,
                idempotency_key: key,
                transaction_id: transaction,
                canonical_request: &encoded,
                blob_inventory: None,
            },
            clock,
            &NeverCancel,
        )
        .map_err(|_| error("USTE_BM06_BOOTSTRAP"))?;
    if outcome.revision.get() != 1 {
        return Err(error("USTE_BM06_BOOTSTRAP_PROFILE"));
    }
    observer(1)?; // Certificate acknowledged; derived bootstrap roots may still be absent.
    let snapshot = raw
        .read_view()
        .map_err(|_| error("USTE_BM06_BOOTSTRAP"))?
        .state()
        .clone();
    uste_graph::publish_graph_state_root(&mut raw, fs, &snapshot)
        .map_err(|_| error("USTE_BM06_BOOTSTRAP_ROOT"))?;
    uste_txn::publish_coordinator_metadata_root(&mut raw, fs)
        .map_err(|_| error("USTE_BM06_BOOTSTRAP_ROOT"))?;
    uste_txn::publish_coordinator_transaction_index(&mut raw, fs)
        .map_err(|_| error("USTE_BM06_BOOTSTRAP_ROOT"))?;
    Ok(())
}

fn require_bootstrap_binding(
    disk: &Disk,
    fs: &mut DiskFileSystem,
    kernel: &PolicyKernel,
    principal: &AuthenticatedPrincipal,
    profile: Bm06Profile,
    clock: &mut SystemClock,
) -> Result<(), LinuxRunnerError> {
    // Trusted fixture maintenance, gated by current authority and exact durable policy before
    // its raw coordinator metadata lookup. This does not add a consumer metadata capability.
    kernel
        .authorize(
            principal,
            uste_policy::Action::ManageSchema,
            uste_policy::Target::Namespace(scope()),
        )
        .map_err(|_| error("USTE_BM06_AUTHORIZATION"))?;
    uste_txn::AuthorizedDiskMetadata::new(disk, kernel).map_err(|_| error("USTE_BM06_POLICY"))?;
    let now = clock
        .observe()
        .map_err(|_| error("USTE_BM06_CLOCK"))?
        .wall_utc;
    let outcome = disk
        .outcome(
            fs,
            PRINCIPAL,
            bootstrap_identity(profile, IdempotencyKey::from_bytes),
            now,
            IndexGetLimits::new(64, 136).map_err(|_| error("USTE_BM06_LIMITS"))?,
            &mut uste_storage::PageCache::new(64 * 1024).map_err(|_| error("USTE_BM06_LIMITS"))?,
        )
        .map_err(|_| error("USTE_BM06_BOOTSTRAP_PROFILE"))?
        .ok_or_else(|| error("USTE_BM06_BOOTSTRAP_PROFILE"))?;
    if outcome.revision.get() != 1
        || outcome.transaction_id != bootstrap_identity(profile, TransactionId::from_bytes)
    {
        return Err(error("USTE_BM06_BOOTSTRAP_PROFILE"));
    }
    Ok(())
}

fn commit_unpublished_tail(
    disk: &mut Disk,
    fs: &mut DiskFileSystem,
    kernel: &mut PolicyKernel,
    principal: &AuthenticatedPrincipal,
    profile: Bm06Profile,
    clock: &mut SystemClock,
) -> Result<(), LinuxRunnerError> {
    let limits = DiskProfileLimits::recovery(profile).map_err(|_| error("USTE_BM06_LIMITS"))?;
    let request = batch(profile, profile.frontier())?;
    let encoded = encode_transaction(&GraphTransaction::new(scope(), request.operations))
        .map_err(|_| error("USTE_BM06_BATCH"))?;
    let tiny =
        uste_storage::IndexRunReadLimits::new(1, 1, 1).map_err(|_| error("USTE_BM06_LIMITS"))?;
    let merge = uste_storage::IndexRunMergeLimits::new(tiny, 1, 1, 1, 1)
        .map_err(|_| error("USTE_BM06_LIMITS"))?;
    let mut writer = uste_txn::AuthorizedDiskWriter::new_with_cache_budget(
        disk,
        kernel,
        uste_graph::GraphDiskWritePreparationLimits {
            proof: limits.preparation,
            delta: uste_graph::GraphStateDeltaLimits::new(1_000_000, 64 * 1024 * 1024)
                .map_err(|_| error("USTE_BM06_LIMITS"))?,
        },
        uste_graph::GraphStateRootMergeLimits::uniform(merge, limits.history_group_bytes)
            .map_err(|_| error("USTE_BM06_LIMITS"))?,
        64 * 1024 * 1024,
    )
    .map_err(|_| error("USTE_BM06_WRITER"))?;
    match writer.commit(
        fs,
        principal,
        AuthorizedTransactionRequest {
            idempotency_key: request.idempotency_key,
            transaction_id: request.transaction_id,
            canonical_request: &encoded,
            blob_inventory: None,
        },
        clock,
        &NeverCancel,
    ) {
        Err(uste_txn::AuthorizedDiskWriteError::CommittedPublication {
            outcome,
            error:
                uste_txn::TransactionError::Storage(uste_storage::journal::StorageError::ResourceLimit),
        }) if outcome.revision.get() == profile.frontier() => Ok(()),
        _ => Err(error("USTE_BM06_EXPECTED_CERTIFIED_TAIL")),
    }
}
