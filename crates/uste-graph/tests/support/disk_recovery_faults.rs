use super::*;
use uste_storage::FileSystem;
use uste_storage::fault::{
    FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation as FaultOperation,
};

type FaultFs = FaultFileSystem<MemoryFileSystem>;
type Recovery = AuthenticatedIndexRecovery<FaultFs, TestEnvelope, CounterEntropy, CounterEntropy>;
type Disk = uste_txn::DiskCommitCoordinator<
    GraphDiskLiveState,
    FaultFs,
    TestEnvelope,
    CounterEntropy,
    CounterEntropy,
>;

struct RecoveryInput {
    recovery: Recovery,
    metadata: uste_txn::CoordinatorDiskBase,
    state: GraphDiskLiveState,
    suffix: Option<uste_txn::RecoveredPreparedSuffix<uste_graph::GraphDiskCommit>>,
    cache: PageCache,
}

fn fixture() -> (FaultFs, EntryName, [u8; 32]) {
    let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let name = EntryName::new("disk-graph-recovery-faults").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 150_000),
        CounterEntropy(151_000),
        GraphState::new(scope()),
    )
    .unwrap();
    commit(
        &mut coordinator,
        &mut filesystem,
        1,
        GraphTransaction::with_policy_mutation(
            scope(),
            vec![Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: record(1),
                    entity_type: text("recovery-fixture"),
                    schema_version: 1,
                    properties: Value::Null,
                }),
            }],
            DurablePolicyMutation::Install {
                policy: NamespacePolicy::new(
                    scope(),
                    PolicyVersion::new(1).unwrap(),
                    QuotaLimits::new(100, 1024 * 1024, 1024 * 1024, 8, 1024).unwrap(),
                ),
            },
        ),
    );
    let snapshot = coordinator.read_view().unwrap().state().clone();
    publish_graph_state_root(&mut coordinator, &mut filesystem, &snapshot).unwrap();
    publish_coordinator_metadata_root(&mut coordinator, &mut filesystem).unwrap();
    uste_txn::publish_coordinator_transaction_index(&mut coordinator, &mut filesystem).unwrap();
    commit(
        &mut coordinator,
        &mut filesystem,
        2,
        GraphTransaction::new(
            scope(),
            vec![Operation::ReplaceEntity {
                target: record(1),
                expected: Expected::Version(uste_graph::RecordVersion::FIRST),
                properties: Value::Bool(true),
            }],
        ),
    );
    let digest =
        GraphState::logical_state_digest(coordinator.read_view().unwrap().state()).unwrap();
    drop(coordinator);
    filesystem.restart().unwrap();
    (filesystem, name, digest)
}

fn prepare(filesystem: &mut FaultFs, name: &EntryName) -> RecoveryInput {
    static ENTROPY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(200_000);
    let entropy = ENTROPY.fetch_add(10_000, std::sync::atomic::Ordering::Relaxed);
    let (recovery, _, frontier) = AuthenticatedIndexRecovery::open_with_frontier_transaction(
        filesystem,
        name,
        scope(),
        CounterEntropy(entropy),
        CounterEntropy(entropy + 1000),
        &mut TestKeyAdapter,
    )
    .unwrap();
    let lookup = uste_storage::IndexGetLimits::new(16, 136).unwrap();
    let mut cache = PageCache::new(64 * 1024).unwrap();
    let transaction_root = recovery
        .load_index_root_manifests(filesystem, uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1)
        .unwrap()
        .remove(0);
    let transactions = uste_txn::admit_coordinator_transaction_index_for_recovery(
        &recovery,
        filesystem,
        transaction_root,
        uste_txn::CoordinatorTransactionAdmissionLimits {
            run: IndexRunReadLimits::new(16, 10, 4096).unwrap(),
            lookup,
            maximum_groups: 1,
            maximum_encoded_bytes: 1_000_000,
        },
        &mut cache,
    )
    .unwrap();
    let candidate = load_coordinator_metadata_candidates_for_recovery::<GraphState, _, _, _, _>(
        &recovery, filesystem,
    )
    .unwrap()
    .remove(0);
    let metadata = uste_txn::admit_coordinator_disk_base(
        &recovery,
        filesystem,
        candidate,
        transactions,
        uste_txn::CoordinatorDiskAdmissionLimits {
            metadata: CoordinatorMetadataLoadLimits::new(1, 0, 2, 16, 4096).unwrap(),
            lookup,
            maximum_total_journal_groups: 1,
            maximum_encoded_bytes_per_pass: 1_000_000,
        },
        &mut cache,
    )
    .unwrap();
    let candidate = load_graph_state_root_candidates_for_recovery(&recovery, filesystem)
        .unwrap()
        .remove(0);
    let (base, _) = admit_graph_disk_base_candidate_for_recovery(
        &recovery,
        filesystem,
        &candidate,
        admission_limits(),
        &mut cache,
    )
    .unwrap();
    let frontier = frontier.unwrap();
    let suffix = if base.revision() == frontier.revision() {
        None
    } else {
        let proof = load_graph_disk_recovery_preparation_view(
            &recovery,
            filesystem,
            &base,
            &frontier,
            GraphDiskPreparationLimits::new(8, 8, 8, 8, 1024 * 1024).unwrap(),
            &mut cache,
        )
        .unwrap()
        .prepare()
        .unwrap();
        Some(
            frontier.bind_prepared(
                prepare_graph_disk_commit(
                    proof,
                    GraphStateDeltaLimits::new(100, 1024 * 1024).unwrap(),
                )
                .unwrap(),
            ),
        )
    };
    RecoveryInput {
        recovery,
        metadata,
        state: GraphDiskLiveState::new(base),
        suffix,
        cache,
    }
}

fn recover(
    filesystem: &mut FaultFs,
    mut input: RecoveryInput,
) -> Result<Disk, uste_txn::TransactionError> {
    uste_txn::DiskCommitCoordinator::recover_with_prepared_suffix(
        input.recovery,
        filesystem,
        input.metadata,
        input.state,
        input.suffix,
        RetentionDays::new(30).unwrap(),
        uste_txn::DiskCoordinatorRecoveryLimits {
            overlay: uste_txn::CoordinatorRecoveryLimits::new(1, 0).unwrap(),
            lookup: uste_storage::IndexGetLimits::new(16, 136).unwrap(),
            maximum_encoded_bytes: 1_000_000,
        },
        &mut input.cache,
    )
}

#[test]
fn every_disk_graph_suffix_read_fault_refuses_provisional_state_then_recovers() {
    let (mut filesystem, name, _) = fixture();
    let input = prepare(&mut filesystem, &name);
    filesystem.arm(FaultPlan::default()).unwrap();
    let disk = recover(&mut filesystem, input).unwrap();
    let reads = filesystem.operation_count(FaultOperation::ReadAt);
    assert!(reads > 0);
    eprintln!(
        "disk_graph_suffix_read_boundaries={reads} fault_cases={}",
        reads * 3
    );
    drop(disk);
    for occurrence in 1..=reads {
        for action in [
            FaultAction::Error(uste_storage::AdapterErrorKind::Io),
            FaultAction::CrashBefore,
            FaultAction::CrashAfter,
        ] {
            let (mut filesystem, name, expected_digest) = fixture();
            let input = prepare(&mut filesystem, &name);
            filesystem
                .arm(
                    FaultPlan::new([FaultPoint {
                        operation: FaultOperation::ReadAt,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
            assert!(
                recover(&mut filesystem, input).is_err(),
                "read {occurrence}/{action:?}"
            );
            assert_eq!(
                filesystem.pending_faults(),
                0,
                "every planned fault must fire"
            );
            filesystem.restart().unwrap();
            let input = prepare(&mut filesystem, &name);
            let mut disk = recover(&mut filesystem, input).unwrap();
            assert_eq!(disk.overlay_counts(), (1, 0));
            assert_eq!(disk.state().unwrap().revision().get(), 2);
            assert!(disk.state().unwrap().is_pending());
            let lookup = uste_storage::IndexGetLimits::new(16, 136).unwrap();
            let mut cache = PageCache::new(64 * 1024).unwrap();
            let outcome = disk
                .outcome(
                    &mut filesystem,
                    uste_policy::PrincipalDigest::from_bytes([0x90; 32]),
                    IdempotencyKey::from_bytes([2; 16]),
                    UtcInstant::new(3, 0).unwrap(),
                    lookup,
                    &mut cache,
                )
                .unwrap()
                .unwrap();
            let read = IndexRunReadLimits::new(100, 100, 1024 * 1024).unwrap();
            let merge = IndexRunMergeLimits::new(read, 100, 1024 * 1024, 100, 1024 * 1024).unwrap();
            uste_graph::publish_graph_disk_coordinator_base(
                &mut disk,
                &mut filesystem,
                outcome,
                GraphStateRootMergeLimits::uniform(merge, 2 * 1024 * 1024).unwrap(),
            )
            .unwrap();
            let roots = disk
                .load_index_root_manifests(&mut filesystem, GRAPH_STATE_PROFILE_V1)
                .unwrap();
            assert_eq!(roots[0].logical_state_digest(), &expected_digest);
            disk.rebase_metadata(
                &mut filesystem,
                uste_txn::CoordinatorMetadataRebaseLimits { merge, reuse: read },
            )
            .unwrap();
            assert_eq!(disk.overlay_counts(), (0, 0));
            if occurrence == 1 && action == FaultAction::Error(uste_storage::AdapterErrorKind::Io) {
                assert_metadata_denial_precedes_disk_io(&disk, &mut filesystem);
            }
        }
    }
}

fn assert_metadata_denial_precedes_disk_io(disk: &Disk, filesystem: &mut FaultFs) {
    struct Identity;
    impl uste_policy::TrustedPrincipalAdapter for Identity {
        type Credential = ();
        fn authenticate(
            &mut self,
            _: &(),
        ) -> Result<uste_policy::PrincipalDigest, uste_policy::AuthenticationError> {
            Ok(uste_policy::PrincipalDigest::from_bytes([0x90; 32]))
        }
    }
    let mut kernel = uste_policy::PolicyKernel::new();
    kernel
        .install_initial_policy(
            disk.state()
                .unwrap()
                .current_base()
                .unwrap()
                .namespace_policy()
                .unwrap()
                .clone(),
        )
        .unwrap();
    let principal = kernel.authenticate(&mut Identity, &()).unwrap();
    let facade = uste_txn::AuthorizedDiskMetadata::new(disk, &kernel).unwrap();
    let reader = uste_txn::AuthorizedDiskReader::new(
        disk,
        &kernel,
        uste_graph::GraphDiskRecordReadLimits {
            current: uste_storage::IndexGetLimits::new(64, 4096).unwrap(),
            historical: IndexPredecessorLimits::new(64, 4096).unwrap(),
        },
    )
    .unwrap();
    filesystem
        .arm(
            FaultPlan::new([FaultPoint {
                operation: FaultOperation::ReadAt,
                occurrence: 1,
                action: FaultAction::Error(uste_storage::AdapterErrorKind::Io),
            }])
            .unwrap(),
        )
        .unwrap();
    let mut unused_clock = ScriptedClock::new([]);
    assert_eq!(
        facade.outcome(
            filesystem,
            &principal,
            IdempotencyKey::from_bytes([2; 16]),
            &mut unused_clock,
        ),
        Err(uste_txn::AuthorizedError::Unauthorized)
    );
    assert_eq!(
        facade.transaction_outcome(
            filesystem,
            &principal,
            TransactionId::from_bytes([34; 16]),
            &mut unused_clock,
        ),
        Err(uste_txn::AuthorizedError::Unauthorized)
    );
    assert_eq!(
        facade.committed_blob_usage(
            filesystem,
            &principal,
            uste_txn::DiskBlobAccountingLimits {
                base: IndexRunReadLimits::new(1, 1, 1).unwrap(),
                maximum_total_owners: 0,
            }
        ),
        Err(uste_txn::AuthorizedError::Unauthorized)
    );
    assert_eq!(filesystem.operation_count(FaultOperation::ReadAt), 0);
    for request in [
        uste_graph::GraphReadRequest::Record { id: record(0x21) },
        uste_graph::GraphReadRequest::RecordAt {
            id: record(0x21),
            revision: CommitRevision::FIRST,
        },
    ] {
        assert!(matches!(
            reader.read(filesystem, &principal, &request, &NeverCancel),
            Err(uste_txn::AuthorizedReadError::Authorization(
                uste_txn::AuthorizedError::Unauthorized
            ))
        ));
    }
    assert_eq!(filesystem.operation_count(FaultOperation::ReadAt), 0);
    assert_eq!(filesystem.pending_faults(), 1);
    assert!(
        disk.load_index_root_manifests(filesystem, GRAPH_STATE_PROFILE_V1)
            .is_err()
    );
    assert_eq!(
        filesystem.pending_faults(),
        0,
        "the untouched fault must still fire on a real read"
    );
}

#[test]
fn certificate_mutation_after_disk_graph_admission_fails_closed_again() {
    let (mut filesystem, name, _) = fixture();
    let input = prepare(&mut filesystem, &name);
    let directory = filesystem
        .open_directory(&filesystem.root(), &name)
        .unwrap();
    let file = filesystem
        .open_existing(&directory, &EntryName::new("CERTIFICATES").unwrap())
        .unwrap();
    let offset = filesystem.metadata(&file).unwrap().len - 1;
    let mut byte = [0];
    assert_eq!(filesystem.read_at(&file, offset, &mut byte).unwrap(), 1);
    byte[0] ^= 1;
    assert_eq!(filesystem.write_at(&file, offset, &byte).unwrap(), 1);
    filesystem.sync_data(&file).unwrap();
    assert!(matches!(
        recover(&mut filesystem, input),
        Err(uste_txn::TransactionError::IntegrityFailure)
    ));
    filesystem.restart().unwrap();
    assert!(matches!(
        AuthenticatedIndexRecovery::open(
            &mut filesystem,
            &name,
            scope(),
            CounterEntropy(500_000),
            CounterEntropy(510_000),
            &mut TestKeyAdapter
        ),
        Err(uste_txn::TransactionError::IntegrityFailure)
    ));
}

fn publish_pending(disk: &mut Disk, filesystem: &mut FaultFs) -> Result<(), GraphDiskError> {
    let mut cache = PageCache::new(64 * 1024).unwrap();
    let outcome = disk
        .outcome(
            filesystem,
            uste_policy::PrincipalDigest::from_bytes([0x90; 32]),
            IdempotencyKey::from_bytes([2; 16]),
            UtcInstant::new(3, 0).unwrap(),
            uste_storage::IndexGetLimits::new(16, 136).unwrap(),
            &mut cache,
        )?
        .unwrap();
    let read = IndexRunReadLimits::new(100, 100, 1024 * 1024).unwrap();
    let merge = IndexRunMergeLimits::new(read, 100, 1024 * 1024, 100, 1024 * 1024).unwrap();
    uste_graph::publish_graph_disk_coordinator_base(
        disk,
        filesystem,
        outcome,
        GraphStateRootMergeLimits::uniform(merge, 2 * 1024 * 1024).unwrap(),
    )
    .map(|_| ())
}

#[test]
fn every_disk_graph_terminal_publication_fault_retains_recoverable_certified_state() {
    let (mut filesystem, name, _) = fixture();
    let input = prepare(&mut filesystem, &name);
    let mut disk = recover(&mut filesystem, input).unwrap();
    filesystem.arm(FaultPlan::default()).unwrap();
    publish_pending(&mut disk, &mut filesystem).unwrap();
    let counts = [
        FaultOperation::CreateNew,
        FaultOperation::WriteAt,
        FaultOperation::SetLen,
        FaultOperation::SyncAll,
        FaultOperation::SyncDirectory,
    ]
    .map(|operation| (operation, filesystem.operation_count(operation)));
    assert!(counts.iter().filter(|(_, count)| *count > 0).count() >= 4);
    eprintln!(
        "disk_graph_publication_boundaries={counts:?} fault_cases={}",
        counts.iter().map(|(_, count)| count * 3).sum::<u64>()
    );
    drop(disk);
    for (operation, count) in counts {
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut filesystem, name, expected_digest) = fixture();
                let input = prepare(&mut filesystem, &name);
                let mut disk = recover(&mut filesystem, input).unwrap();
                let anchor = disk.checkpoint_anchor().unwrap();
                filesystem
                    .arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                assert!(
                    publish_pending(&mut disk, &mut filesystem).is_err(),
                    "{operation:?}/{occurrence}/{action:?}"
                );
                assert_eq!(
                    filesystem.pending_faults(),
                    0,
                    "every planned publication fault must fire"
                );
                assert!(disk.state().unwrap().is_pending());
                assert_eq!(disk.overlay_counts(), (1, 0));
                assert_eq!(disk.checkpoint_anchor().unwrap(), anchor);
                if !filesystem.is_crashed() {
                    publish_pending(&mut disk, &mut filesystem).unwrap();
                    assert!(!disk.state().unwrap().is_pending());
                }
                drop(disk);
                filesystem.restart().unwrap();
                let input = prepare(&mut filesystem, &name);
                let mut disk = recover(&mut filesystem, input).unwrap();
                assert_eq!(disk.checkpoint_anchor().unwrap(), anchor);
                assert_eq!(disk.state().unwrap().revision().get(), 2);
                if disk.state().unwrap().is_pending() {
                    publish_pending(&mut disk, &mut filesystem).unwrap();
                }
                let roots = disk
                    .load_index_root_manifests(&mut filesystem, GRAPH_STATE_PROFILE_V1)
                    .unwrap();
                assert_eq!(roots[0].logical_state_digest(), &expected_digest);
                let read = IndexRunReadLimits::new(100, 100, 1024 * 1024).unwrap();
                let merge =
                    IndexRunMergeLimits::new(read, 100, 1024 * 1024, 100, 1024 * 1024).unwrap();
                disk.rebase_metadata(
                    &mut filesystem,
                    uste_txn::CoordinatorMetadataRebaseLimits { merge, reuse: read },
                )
                .unwrap();
                assert_eq!(disk.overlay_counts(), (0, 0));
            }
        }
    }
}
