//! Small disk-state BM-06 equivalence check. The filesystem and credential adapter are test models.
use super::*;
use crate::recovery_materialization::{Bm06Profile, PAYLOAD_BYTES, VERSIONS};
use disk::{DiskBatch, DiskProfileLimits, admit_development_disk, commit_batch};
use uste_graph::{GraphDiskLiveState, GraphDiskReadLimits, Record};
use uste_storage::{IndexGetLimits, IndexPredecessorLimits, PageCache};
use uste_txn::{AuthenticatedIndexRecovery, AuthorizedDiskReader, DiskCommitCoordinator};

pub const MAX_DEVELOPMENT_RECORDS: u64 = 2;
type Disk = DiskCommitCoordinator<
    GraphDiskLiveState,
    MemoryFileSystem,
    TestEnvelope,
    CounterEntropy,
    CounterEntropy,
>;

#[derive(Debug)]
pub struct RecoveryDevelopmentVerification {
    pub records: u64,
    pub verified_versions: u64,
    pub verified_payload_bytes: u64,
    pub base_revision: u64,
    pub recovered_revision: u64,
    pub storage_metadata_memory_resident: bool,
}

pub(crate) fn recovery_policy() -> Result<NamespacePolicy, String> {
    let quotas = QuotaLimits::new(16 * 1024 * 1024, 0, 0, 0, 1024 * 1024).map_err(debug)?;
    let mut policy = NamespacePolicy::new(scope(), PolicyVersion::new(1).map_err(debug)?, quotas);
    policy
        .grant(
            PRINCIPAL,
            NamespaceGrant::new(
                PermissionSet::from_actions([
                    Action::ReadRecord,
                    Action::ReadHistory,
                    Action::Commit,
                    Action::ManageSchema,
                ]),
                quotas,
            ),
        )
        .map_err(debug)?;
    Ok(policy)
}

fn reopen(
    fs: &mut MemoryFileSystem,
    name: &EntryName,
    limits: DiskProfileLimits,
    repair: bool,
) -> Result<(Disk, disk::DiskAdmissionMeasurement), String> {
    static ENTROPY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(20_000_000);
    let entropy = ENTROPY.fetch_add(1_000_000, std::sync::atomic::Ordering::Relaxed);
    let (recovery, _, frontier) = AuthenticatedIndexRecovery::open_with_disk_blob_metadata(
        fs,
        name,
        scope(),
        CounterEntropy(entropy),
        CounterEntropy(entropy + 500_000),
        &mut TestKeyAdapter,
        limits.blob_recovery,
        &mut PageCache::new(64 * 1024 * 1024).map_err(debug)?,
    )
    .map_err(debug)?;
    admit_development_disk(
        fs,
        recovery,
        frontier.ok_or("missing BM-06 frontier")?,
        limits,
        repair,
    )
}

/// Encrypted authorized writes and cold disk-backed recovery, never a native timing campaign.
/// The cap precedes key/filesystem allocation. All production constructors and guards remain.
pub fn verify_disk_recovery(
    profile: Bm06Profile,
) -> Result<RecoveryDevelopmentVerification, String> {
    if profile.records() > MAX_DEVELOPMENT_RECORDS {
        return Err("BM-06 memory-adapter verifier accepts at most 2 records".into());
    }
    let limits = DiskProfileLimits::recovery(profile)?;
    let policy = recovery_policy()?;
    let mut fs = MemoryFileSystem::default();
    let name = EntryName::new("bm06-disk-development").map_err(debug)?;
    let vault = KeyVault::create(scope().database(), &mut TestKeyAdapter, CounterEntropy(501))
        .map_err(debug)?;
    let mut raw = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).map_err(debug)?,
        name.clone(),
        vault,
        CounterEntropy(1001),
        GraphState::new(scope()),
    )
    .map_err(debug)?;
    let install = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        Vec::new(),
        uste_graph::DurablePolicyMutation::Install {
            policy: policy.clone(),
        },
    ))
    .map_err(debug)?;
    raw.commit(
        &mut fs,
        TransactionRequest {
            principal: PRINCIPAL,
            idempotency_key: identity(1, IdempotencyKey::from_bytes),
            transaction_id: identity(1, TransactionId::from_bytes),
            canonical_request: &install,
            blob_inventory: None,
        },
        &mut clock(1),
        &NeverCancel,
    )
    .map_err(debug)?;
    let snapshot = raw.read_view().map_err(debug)?.state().clone();
    uste_graph::publish_graph_state_root(&mut raw, &mut fs, &snapshot).map_err(debug)?;
    uste_txn::publish_coordinator_metadata_root(&mut raw, &mut fs).map_err(debug)?;
    uste_txn::publish_coordinator_transaction_index(&mut raw, &mut fs).map_err(debug)?;
    drop(snapshot);
    drop(raw);
    fs.restart().map_err(debug)?;
    let (mut disk, _) = reopen(&mut fs, &name, limits, false)?;
    let mut kernel = kernel(policy)?;
    let principal = kernel.authenticate(&mut AuthAdapter, &()).map_err(debug)?;
    for sequence in 2..=profile.checkpoint_revision() {
        commit_batch(
            &mut disk,
            &mut fs,
            &mut kernel,
            &principal,
            DiskBatch {
                sequence,
                idempotency_key: identity(sequence, IdempotencyKey::from_bytes),
                transaction_id: identity(sequence, TransactionId::from_bytes),
                operations: profile.batch(scope(), sequence)?,
            },
            &mut clock(sequence),
            limits,
        )?;
    }
    // Deliberately refuse only derived publication of the final certified batch. This leaves
    // the real checkpoint/root boundary and journal suffix for cold private-stage recovery.
    // The bounded profile has one batch per generation; this is not a 196-revision trial.
    let sequence = profile.frontier();
    let encoded = encode_transaction(&GraphTransaction::new(
        scope(),
        profile.batch(scope(), sequence)?,
    ))
    .map_err(debug)?;
    let tiny = uste_storage::IndexRunReadLimits::new(1, 1, 1).map_err(debug)?;
    let merge = uste_storage::IndexRunMergeLimits::new(tiny, 1, 1, 1, 1).map_err(debug)?;
    let mut writer = uste_txn::AuthorizedDiskWriter::new_with_cache_budget(
        &mut disk,
        &mut kernel,
        uste_graph::GraphDiskWritePreparationLimits {
            proof: limits.preparation,
            delta: uste_graph::GraphStateDeltaLimits::new(1_000_000, 64 * 1024 * 1024)
                .map_err(debug)?,
        },
        uste_graph::GraphStateRootMergeLimits::uniform(merge, limits.history_group_bytes)
            .map_err(debug)?,
        64 * 1024 * 1024,
    )
    .map_err(debug)?;
    let outcome = match writer.commit(
        &mut fs,
        &principal,
        AuthorizedTransactionRequest {
            idempotency_key: identity(sequence, IdempotencyKey::from_bytes),
            transaction_id: identity(sequence, TransactionId::from_bytes),
            canonical_request: &encoded,
            blob_inventory: None,
        },
        &mut clock(sequence),
        &NeverCancel,
    ) {
        Err(uste_txn::AuthorizedDiskWriteError::CommittedPublication {
            outcome,
            error:
                uste_txn::TransactionError::Storage(uste_storage::journal::StorageError::ResourceLimit),
        }) => outcome,
        other => {
            return Err(format!(
                "BM-06 expected certified publication refusal: {other:?}"
            ));
        }
    };
    if outcome.revision.get() != sequence {
        return Err("BM-06 tail revision mismatch".into());
    }
    drop(writer);
    drop(disk);
    fs.restart().map_err(debug)?;
    let (mut disk, admission) = reopen(&mut fs, &name, limits, true)?;
    if admission.graph_revision != profile.checkpoint_revision()
        || admission.metadata_revision != profile.checkpoint_revision()
    {
        return Err("BM-06 recovery did not start at selected base".into());
    }
    disk.rebase_metadata(
        &mut fs,
        uste_txn::CoordinatorMetadataRebaseLimits {
            merge: limits.merge,
            reuse: limits.merge_read,
        },
    )
    .map_err(debug)?;
    commit_batch(
        &mut disk,
        &mut fs,
        &mut kernel,
        &principal,
        DiskBatch {
            sequence,
            idempotency_key: identity(sequence, IdempotencyKey::from_bytes),
            transaction_id: identity(sequence, TransactionId::from_bytes),
            operations: profile.batch(scope(), sequence)?,
        },
        &mut clock(sequence),
        limits,
    )?;
    drop(disk);
    fs.restart().map_err(debug)?;
    let (disk, terminal) = reopen(&mut fs, &name, limits, false)?;
    if terminal.graph_revision != sequence
        || disk.overlay_counts() != (0, 0)
        || terminal.state_counts != [profile.records(), profile.events(), 0, 0, 0, 0, 1, 1]
    {
        return Err("BM-06 terminal cardinality mismatch".into());
    }
    let verified = verify_history(&disk, &mut fs, &kernel, &principal, profile, VERSIONS)?;
    Ok(RecoveryDevelopmentVerification {
        records: profile.records(),
        verified_versions: verified,
        verified_payload_bytes: verified * PAYLOAD_BYTES as u64,
        base_revision: admission.graph_revision,
        recovered_revision: terminal.graph_revision,
        storage_metadata_memory_resident: disk.certificate_anchor_residency().0
            || disk.blob_metadata_residency().0,
    })
}

/// Verify the complete admitted fixture prefix before a native adapter appends more events.
/// It is bounded by the caller's profile, not an alternative authorization path.
pub(crate) fn verify_history<F, W, E, I>(
    disk: &DiskCommitCoordinator<GraphDiskLiveState, F, W, E, I>,
    fs: &mut F,
    kernel: &PolicyKernel,
    principal: &AuthenticatedPrincipal,
    profile: Bm06Profile,
    versions: u64,
) -> Result<u64, String>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if versions == 0 || versions > VERSIONS {
        return Err("BM-06 unsupported generation frontier".into());
    }
    let limits = DiskProfileLimits::recovery(profile)?;
    let base = disk
        .state()
        .map_err(debug)?
        .current_base()
        .ok_or("BM-06 pending base")?;
    if base.revision().get() != 1 + versions * profile.batches_per_version()
        || base.state_counts()
            != [
                profile.records(),
                profile.records() * versions,
                0,
                0,
                0,
                0,
                1,
                1,
            ]
    {
        return Err("BM-06 profile/frontier mismatch".into());
    }
    let reader = AuthorizedDiskReader::new_with_cache_budget(
        disk,
        kernel,
        GraphDiskReadLimits {
            current: IndexGetLimits::new(64, 16 * 1024).map_err(debug)?,
            historical: IndexPredecessorLimits::new(limits.historical_page_visits, 16 * 1024)
                .map_err(debug)?,
            expansion: None,
        },
        64 * 1024 * 1024,
    )
    .map_err(debug)?;
    let mut verified = 0;
    for generation in 0..versions {
        for record in 0..profile.records() {
            let ordinal = generation * profile.records() + record;
            let revision =
                uste_types::CommitRevision::new(profile.event_revision(ordinal)?).map_err(debug)?;
            let actual = reader
                .read(
                    fs,
                    principal,
                    &GraphReadRequest::RecordAt {
                        id: profile.record_ref(scope(), record)?,
                        revision,
                    },
                    &NeverCancel,
                )
                .map_err(|error| format!("BM-06 historical oracle ordinal {ordinal}: {error:?}"))?;
            let GraphReadOutput::Record(Some(actual)) = actual else {
                return Err("BM-06 missing historical record".into());
            };
            let Record::Entity(actual) = *actual else {
                return Err("BM-06 wrong record kind".into());
            };
            if actual.version.get() != generation + 1
                || actual.modified_revision != revision
                || actual.id != profile.record_ref(scope(), record)?
                || actual.created_revision.get() != profile.event_revision(record)?
                || actual.lifecycle != uste_graph::EntityLifecycle::Active
                || actual.entity_type.as_str() != crate::recovery_materialization::PROFILE
                || actual.schema_version != 1
                || actual.properties
                    != Value::bytes(profile.payload(ordinal)?.to_vec()).map_err(debug)?
            {
                return Err("BM-06 historical oracle mismatch".into());
            }
            verified += 1;
        }
    }
    Ok(verified)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bm06_disk_profile_admits_exact_dimensions_without_running_them() {
        for records in [1, 2, 511, 512, 513, 100_000] {
            let profile = Bm06Profile::new(records).unwrap();
            let limits = DiskProfileLimits::recovery(profile).unwrap();
            assert_eq!(limits.groups, profile.frontier());
            assert_eq!(limits.history_group_bytes, 100 * 16 * 1024);
            assert_eq!(limits.historical_page_visits, 664);
            assert_eq!(
                limits.merge_read.maximum_entries(),
                profile.events().max(profile.frontier() + 1)
            );
        }
        assert!(verify_disk_recovery(Bm06Profile::new(3).unwrap()).is_err());
        assert!(verify_disk_recovery(Bm06Profile::new(100_000).unwrap()).is_err());
    }
    #[test]
    fn bm06_disk_recovery_preserves_every_historical_payload_and_exact_frontier() {
        for records in [1, 2] {
            let report = verify_disk_recovery(Bm06Profile::new(records).unwrap()).unwrap();
            assert_eq!(
                (
                    report.records,
                    report.verified_versions,
                    report.verified_payload_bytes
                ),
                (records, records * 100, records * 409600)
            );
            assert_eq!(
                (report.base_revision, report.recovered_revision),
                (100, 101)
            );
            assert!(!report.storage_metadata_memory_resident);
        }
    }
}
