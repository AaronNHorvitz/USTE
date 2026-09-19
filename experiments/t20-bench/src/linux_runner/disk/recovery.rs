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
    if profile.records() > MAX_NATIVE_RECORDS {
        return Err(error("USTE_BM06_DEVELOPMENT_LIMIT"));
    }
    if !matches!(
        phase,
        "create" | "tail" | "recover" | "open" | "tail-crash-probe"
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
        let mut raw = CommitCoordinator::create(
            &mut fs,
            scope(),
            retention()?,
            name.clone(),
            vault,
            OsEntropy,
            GraphState::new(scope()),
        )
        .map_err(|_| error("USTE_BM06_CREATE"))?;
        let install = encode_transaction(&GraphTransaction::with_policy_mutation(
            scope(),
            Vec::new(),
            uste_graph::DurablePolicyMutation::Install {
                policy: policy.clone(),
            },
        ))
        .map_err(|_| error("USTE_BM06_POLICY"))?;
        raw.commit(
            &mut fs,
            TransactionRequest {
                principal: PRINCIPAL,
                idempotency_key: identity(1, IdempotencyKey::from_bytes),
                transaction_id: identity(1, TransactionId::from_bytes),
                canonical_request: &install,
                blob_inventory: None,
            },
            &mut clock,
            &NeverCancel,
        )
        .map_err(|_| error("USTE_BM06_BOOTSTRAP"))?;
        let snapshot = raw
            .read_view()
            .map_err(|_| error("USTE_BM06_BOOTSTRAP"))?
            .state()
            .clone();
        uste_graph::publish_graph_state_root(&mut raw, &mut fs, &snapshot)
            .map_err(|_| error("USTE_BM06_BOOTSTRAP_ROOT"))?;
        uste_txn::publish_coordinator_metadata_root(&mut raw, &mut fs)
            .map_err(|_| error("USTE_BM06_BOOTSTRAP_ROOT"))?;
        uste_txn::publish_coordinator_transaction_index(&mut raw, &mut fs)
            .map_err(|_| error("USTE_BM06_BOOTSTRAP_ROOT"))?;
        drop(snapshot);
        drop(raw);
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
    let frontier = frontier.ok_or_else(|| error("USTE_BM06_FRONTIER"))?;
    let expected = match phase {
        "create" => 1,
        "tail" | "tail-crash-probe" => profile.checkpoint_revision(),
        _ => profile.frontier(),
    };
    if frontier.revision().get() != expected {
        return Err(error("USTE_BM06_FRONTIER"));
    }
    let (mut disk, admission) =
        admit_development_disk(&mut fs, recovery, frontier, limits, phase == "recover")
            .map_err(|_| error("USTE_BM06_ADMISSION"))?;
    let mut kernel = kernel(policy).map_err(|_| error("USTE_BM06_POLICY"))?;
    let principal = authenticate(&kernel)?;
    if phase == "create" {
        for sequence in 2..=profile.checkpoint_revision() {
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
    let versions = if matches!(phase, "create" | "tail" | "tail-crash-probe") {
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
