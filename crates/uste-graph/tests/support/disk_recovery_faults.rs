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
    fixture_with_suffix(1, 1)
}

fn fixture_with_suffix(count: u8, graph_revision: u8) -> (FaultFs, EntryName, [u8; 32]) {
    fixture_variation(count, graph_revision, false)
}

fn fixture_variation(count: u8, graph_revision: u8, flip: bool) -> (FaultFs, EntryName, [u8; 32]) {
    fixture_roots(count, graph_revision, flip, true)
}

fn fixture_roots(
    count: u8,
    graph_revision: u8,
    flip: bool,
    roots: bool,
) -> (FaultFs, EntryName, [u8; 32]) {
    fixture_metadata_roots(count, graph_revision, flip, roots, None)
}

fn fixture_metadata_roots(
    count: u8,
    graph_revision: u8,
    flip: bool,
    roots: bool,
    metadata_at: Option<u8>,
) -> (FaultFs, EntryName, [u8; 32]) {
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
    if roots {
        let snapshot = coordinator.read_view().unwrap().state().clone();
        publish_graph_state_root(&mut coordinator, &mut filesystem, &snapshot).unwrap();
        publish_coordinator_metadata_root(&mut coordinator, &mut filesystem).unwrap();
        uste_txn::publish_coordinator_transaction_index(&mut coordinator, &mut filesystem).unwrap();
    }
    for revision in 2..=count + 1 {
        commit(
            &mut coordinator,
            &mut filesystem,
            revision,
            GraphTransaction::new(
                scope(),
                vec![Operation::ReplaceEntity {
                    target: record(1),
                    expected: Expected::Version(
                        uste_graph::RecordVersion::new(u64::from(revision - 1)).unwrap(),
                    ),
                    properties: Value::Bool(revision.is_multiple_of(2) != flip),
                }],
            ),
        );
        if roots && revision == graph_revision {
            let snapshot = coordinator.read_view().unwrap().state().clone();
            publish_graph_state_root(&mut coordinator, &mut filesystem, &snapshot).unwrap();
        }
        if metadata_at == Some(revision) {
            publish_coordinator_metadata_root(&mut coordinator, &mut filesystem).unwrap();
            uste_txn::publish_coordinator_transaction_index(&mut coordinator, &mut filesystem)
                .unwrap();
        }
    }
    let digest =
        GraphState::logical_state_digest(coordinator.read_view().unwrap().state()).unwrap();
    drop(coordinator);
    filesystem.restart().unwrap();
    (filesystem, name, digest)
}

fn prepare(filesystem: &mut FaultFs, name: &EntryName) -> RecoveryInput {
    prepare_mode(filesystem, name, false)
}

fn prepare_mode(filesystem: &mut FaultFs, name: &EntryName, streaming: bool) -> RecoveryInput {
    prepare_certificate_mode(filesystem, name, streaming, false)
}

fn prepare_certificate_mode(
    filesystem: &mut FaultFs,
    name: &EntryName,
    streaming: bool,
    disk_certificates: bool,
) -> RecoveryInput {
    prepare_source(filesystem, name, streaming, disk_certificates, false)
}

fn prepare_source(
    filesystem: &mut FaultFs,
    name: &EntryName,
    streaming: bool,
    disk_certificates: bool,
    origin: bool,
) -> RecoveryInput {
    static ENTROPY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(200_000);
    let entropy = ENTROPY.fetch_add(10_000, std::sync::atomic::Ordering::Relaxed);
    let (mut recovery, _, frontier) = if disk_certificates {
        AuthenticatedIndexRecovery::open_with_disk_certificate_anchors(
            filesystem,
            name,
            scope(),
            CounterEntropy(entropy),
            CounterEntropy(entropy + 1000),
            &mut TestKeyAdapter,
            uste_storage::journal::CertificateAnchorReadLimits::new(4, 4 * 4161).unwrap(),
        )
    } else {
        AuthenticatedIndexRecovery::open_with_frontier_transaction(
            filesystem,
            name,
            scope(),
            CounterEntropy(entropy),
            CounterEntropy(entropy + 1000),
            &mut TestKeyAdapter,
        )
    }
    .unwrap();
    let lookup = uste_storage::IndexGetLimits::new(16, 136).unwrap();
    let mut cache = PageCache::new(64 * 1024).unwrap();
    let staged = if origin {
        let genesis = recovery
            .recover_inventory_free_genesis(filesystem, GraphState::new(scope()), 1_000_000)
            .unwrap();
        let read = IndexRunReadLimits::new(100, 100, 1024 * 1024).unwrap();
        let merge = IndexRunMergeLimits::new(read, 100, 1024 * 1024, 100, 1024 * 1024).unwrap();
        let graph =
            uste_graph::stage_graph_genesis_root(&mut recovery, filesystem, &genesis, merge)
                .unwrap();
        let (metadata, transactions) = uste_txn::stage_inventory_free_genesis_metadata(
            &mut recovery,
            filesystem,
            &genesis,
            merge,
        )
        .unwrap();
        assert_eq!(graph.generation(), 0);
        assert_eq!(metadata.generation(), 0);
        assert_eq!(transactions.generation(), 0);
        Some((graph, metadata, transactions))
    } else {
        None
    };
    let (graph_candidate, metadata_candidate, transaction_root) = match staged {
        Some((graph, metadata, transactions)) => (Some(graph), Some(metadata), transactions),
        None => (
            None,
            None,
            recovery
                .load_index_root_manifests(filesystem, uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1)
                .unwrap()
                .into_iter()
                .min_by_key(|root| root.revision())
                .unwrap(),
        ),
    };
    let transaction_groups = transaction_root.revision().get();
    let transactions = uste_txn::admit_coordinator_transaction_index_for_recovery(
        &recovery,
        filesystem,
        transaction_root,
        uste_txn::CoordinatorTransactionAdmissionLimits {
            run: IndexRunReadLimits::new(16, 10, 4096).unwrap(),
            lookup,
            maximum_groups: transaction_groups,
            maximum_encoded_bytes: 1_000_000,
        },
        &mut cache,
    )
    .unwrap();
    let candidate = metadata_candidate.unwrap_or_else(|| {
        load_coordinator_metadata_candidates_for_recovery::<GraphState, _, _, _, _>(
            &recovery, filesystem,
        )
        .unwrap()
        .into_iter()
        .min_by_key(|root| root.revision())
        .unwrap()
    });
    let metadata_groups = candidate.revision().get();
    let metadata = uste_txn::admit_coordinator_disk_base(
        &recovery,
        filesystem,
        candidate,
        transactions,
        uste_txn::CoordinatorDiskAdmissionLimits {
            metadata: CoordinatorMetadataLoadLimits::new(
                metadata_groups,
                0,
                metadata_groups + 1,
                16,
                4096,
            )
            .unwrap(),
            lookup,
            maximum_total_journal_groups: metadata_groups,
            maximum_encoded_bytes_per_pass: 1_000_000,
        },
        &mut cache,
    )
    .unwrap();
    let candidate = graph_candidate.unwrap_or_else(|| {
        load_graph_state_root_candidates_for_recovery(&recovery, filesystem)
            .unwrap()
            .remove(0)
    });
    let (base, _) = admit_graph_disk_base_candidate_for_recovery(
        &recovery,
        filesystem,
        &candidate,
        admission_limits(),
        &mut cache,
    )
    .unwrap();
    let frontier = frontier.unwrap();
    let suffix = if streaming || base.revision() == frontier.revision() {
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
    let state = GraphDiskLiveState::new(base);
    if origin {
        let (_, revision, certificate) =
            uste_txn::JournalAnchoredTransactionState::journal_base_anchor(&state).unwrap();
        assert!(
            uste_txn::DiskCoordinatorState::metadata_publication_input(
                &state,
                (revision, certificate),
            )
            .is_err()
        );
    }
    RecoveryInput {
        recovery,
        metadata,
        state,
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

fn recover_stream(
    filesystem: &mut FaultFs,
    mut input: RecoveryInput,
    maximum_revisions: u64,
    bytes: u64,
) -> Result<(Disk, uste_graph::GraphDiskSuffixRecoveryReport), GraphDiskError> {
    let read = IndexRunReadLimits::new(100, 100, 1024 * 1024).unwrap();
    let merge = IndexRunMergeLimits::new(read, 100, 1024 * 1024, 100, 1024 * 1024).unwrap();
    uste_graph::recover_graph_disk_suffix(
        input.recovery,
        filesystem,
        input.metadata,
        input.state,
        RetentionDays::new(30).unwrap(),
        uste_txn::DiskCoordinatorRecoveryLimits {
            overlay: uste_txn::CoordinatorRecoveryLimits::new(3, 0).unwrap(),
            lookup: uste_storage::IndexGetLimits::new(16, 136).unwrap(),
            maximum_encoded_bytes: bytes,
        },
        uste_graph::GraphDiskSuffixRecoveryLimits {
            maximum_revisions,
            preparation: GraphDiskPreparationLimits::new(8, 8, 8, 8, 1024 * 1024).unwrap(),
            deltas: GraphStateDeltaLimits::new(100, 1024 * 1024).unwrap(),
            merge: GraphStateRootMergeLimits::uniform(merge, 2 * 1024 * 1024).unwrap(),
        },
        &mut input.cache,
    )
}

#[test]
fn origin_graph_genesis_staging_faults_leave_no_discoverable_roots() {
    fn open_origin(fs: &mut FaultFs, name: &EntryName) -> Recovery {
        AuthenticatedIndexRecovery::open_with_disk_certificate_anchors(
            fs,
            name,
            scope(),
            CounterEntropy(880_000),
            CounterEntropy(890_000),
            &mut TestKeyAdapter,
            uste_storage::journal::CertificateAnchorReadLimits::new(4, 4 * 4161).unwrap(),
        )
        .unwrap()
        .0
    }
    fn stage(fs: &mut FaultFs, recovery: &mut Recovery) -> Result<(), GraphDiskError> {
        let genesis =
            recovery.recover_inventory_free_genesis(fs, GraphState::new(scope()), 1_000_000)?;
        let read = IndexRunReadLimits::new(100, 100, 1024 * 1024).unwrap();
        let merge = IndexRunMergeLimits::new(read, 100, 1024 * 1024, 100, 1024 * 1024).unwrap();
        uste_graph::stage_graph_genesis_root(recovery, fs, &genesis, merge)?;
        uste_txn::stage_inventory_free_genesis_metadata(recovery, fs, &genesis, merge)?;
        Ok(())
    }
    let (mut baseline, name, _) = fixture_roots(3, 1, false, false);
    let mut recovery = open_origin(&mut baseline, &name);
    baseline.arm(FaultPlan::default()).unwrap();
    stage(&mut baseline, &mut recovery).unwrap();
    drop(recovery);
    let mut attempts = 0;
    for operation in [
        FaultOperation::OpenExisting,
        FaultOperation::Metadata,
        FaultOperation::ReadAt,
        FaultOperation::CreateNew,
        FaultOperation::WriteAt,
        FaultOperation::SetLen,
        FaultOperation::SyncAll,
        FaultOperation::SyncDirectory,
        FaultOperation::RenameNoReplace,
        FaultOperation::RemoveFile,
        FaultOperation::SyncData,
    ] {
        for occurrence in 1..=baseline.operation_count(operation) {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut fs, name, _) = fixture_roots(3, 1, false, false);
                let mut recovery = open_origin(&mut fs, &name);
                fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                assert!(
                    stage(&mut fs, &mut recovery).is_err(),
                    "{operation:?} {occurrence}"
                );
                assert_eq!(fs.pending_faults(), 0);
                drop(recovery);
                fs.restart().unwrap();
                let recovery = open_origin(&mut fs, &name);
                for profile in [
                    GRAPH_STATE_PROFILE_V1,
                    uste_txn::COORDINATOR_METADATA_PROFILE_V1,
                    uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
                ] {
                    assert!(
                        recovery
                            .load_index_root_manifests(&mut fs, profile)
                            .unwrap()
                            .is_empty()
                    );
                }
                drop(recovery);
                let input = prepare_source(&mut fs, &name, true, true, true);
                assert_eq!(
                    recover_stream(&mut fs, input, 3, 1_000_000)
                        .unwrap()
                        .0
                        .checkpoint_anchor()
                        .unwrap()
                        .unwrap()
                        .0
                        .get(),
                    4
                );
                attempts += 1;
            }
        }
    }
    assert!(attempts > 0);
    eprintln!("genesis staging I/O error/crash attempts: {attempts}");
}

#[test]
fn private_origin_rebase_can_replace_stale_pairs_without_relaxing_ordinary_rebase() {
    let (mut fs, name, digest) = fixture_metadata_roots(3, 1, false, true, Some(3));
    let read = IndexRunReadLimits::new(100, 100, 1024 * 1024).unwrap();
    let merge = IndexRunMergeLimits::new(read, 100, 1024 * 1024, 100, 1024 * 1024).unwrap();
    let limits = uste_txn::CoordinatorMetadataRebaseLimits { merge, reuse: read };
    let input = prepare_source(&mut fs, &name, true, true, false);
    let (mut disk, _) = recover_stream(&mut fs, input, 3, 1_000_000).unwrap();
    let anchor = disk.checkpoint_anchor().unwrap();
    assert_eq!(
        disk.rebase_metadata(&mut fs, limits),
        Err(uste_txn::TransactionError::ResourceLimit)
    );
    drop(disk);
    fs.restart().unwrap();
    let input = prepare_source(&mut fs, &name, true, true, true);
    let (mut disk, _) = recover_stream(&mut fs, input, 3, 1_000_000).unwrap();
    disk.rebase_metadata(&mut fs, limits).unwrap();
    assert_eq!(disk.checkpoint_anchor().unwrap(), anchor);
    assert_eq!(disk.overlay_counts(), (0, 0));
    assert!(!disk.rebase_required());
    for profile in [
        uste_txn::COORDINATOR_METADATA_PROFILE_V1,
        uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
    ] {
        let roots = disk.load_index_root_manifests(&mut fs, profile).unwrap();
        let terminal = roots
            .iter()
            .find(|root| root.revision().get() == 4)
            .unwrap();
        assert_eq!(terminal.logical_state_digest(), &digest);
        assert!(
            roots
                .iter()
                .all(|root| [3, 4].contains(&root.revision().get()))
        );
    }
}

#[test]
fn origin_graph_recovery_stages_genesis_without_historical_publication() {
    for suffix in [0, 3] {
        let (mut fs, name, digest) = fixture_roots(suffix, 1, false, false);
        let input = prepare_source(&mut fs, &name, true, true, true);
        for profile in [
            GRAPH_STATE_PROFILE_V1,
            uste_txn::COORDINATOR_METADATA_PROFILE_V1,
            uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
        ] {
            assert!(
                input
                    .recovery
                    .load_index_root_manifests(&mut fs, profile)
                    .unwrap()
                    .is_empty()
            );
        }
        let (mut disk, report) =
            recover_stream(&mut fs, input, u64::from(suffix), 1_000_000).unwrap();
        assert_eq!(report.revisions, u64::from(suffix));
        assert_eq!(
            disk.state().unwrap().revision().get(),
            u64::from(suffix) + 1
        );
        let roots = disk
            .load_index_root_manifests(&mut fs, GRAPH_STATE_PROFILE_V1)
            .unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].logical_state_digest(), &digest);
        assert_ne!(roots[0].generation(), 0);
        assert!(disk.rebase_required());
        let read = IndexRunReadLimits::new(100, 100, 1024 * 1024).unwrap();
        let merge = IndexRunMergeLimits::new(read, 100, 1024 * 1024, 100, 1024 * 1024).unwrap();
        disk.rebase_metadata(
            &mut fs,
            uste_txn::CoordinatorMetadataRebaseLimits { merge, reuse: read },
        )
        .unwrap();
        assert_eq!(disk.overlay_counts(), (0, 0));
        assert!(!disk.rebase_required());
        for profile in [
            uste_txn::COORDINATOR_METADATA_PROFILE_V1,
            uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
        ] {
            let roots = disk.load_index_root_manifests(&mut fs, profile).unwrap();
            assert_eq!(roots.len(), 1);
            assert_eq!(roots[0].revision().get(), u64::from(suffix) + 1);
            assert_ne!(roots[0].generation(), 0);
        }
        for revision in 1..=suffix + 1 {
            assert_eq!(
                disk.outcome(
                    &mut fs,
                    uste_policy::PrincipalDigest::from_bytes([0x90; 32]),
                    IdempotencyKey::from_bytes([revision; 16]),
                    UtcInstant::new(5, 0).unwrap(),
                    uste_storage::IndexGetLimits::new(16, 136).unwrap(),
                    &mut PageCache::new(64 * 1024).unwrap(),
                )
                .unwrap()
                .unwrap()
                .revision
                .get(),
                u64::from(revision)
            );
        }
        assert_metadata_denial_precedes_disk_io(&disk, &mut fs);
        drop(disk);
        fs.restart().unwrap();
        // Normal cold admission uses the published terminal pair, not another origin rebuild.
        let input = prepare_source(&mut fs, &name, true, true, false);
        let (disk, report) = recover_stream(&mut fs, input, 0, 1_000_000).unwrap();
        assert_eq!(report.revisions, 0);
        assert!(!disk.rebase_required());
        assert_eq!(
            disk.checkpoint_anchor().unwrap().unwrap().0.get(),
            u64::from(suffix) + 1
        );
    }
}

#[test]
fn streamed_graph_suffix_matches_full_reducer_with_lagging_metadata_and_ready_reopen() {
    for graph_revision in [1, 2, 4] {
        let (mut fs, name, digest) = fixture_with_suffix(3, graph_revision);
        let input = prepare_mode(&mut fs, &name, true);
        let (disk, report) =
            recover_stream(&mut fs, input, 4 - u64::from(graph_revision), 3 * 8322).unwrap();
        assert_eq!(report.revisions, 4 - u64::from(graph_revision));
        assert_eq!(disk.overlay_counts(), (3, 0));
        assert!(!disk.state().unwrap().is_pending());
        assert_eq!(disk.state().unwrap().revision().get(), 4);
        assert_ne!(
            disk.state().unwrap().current_base().unwrap().generation(),
            0
        );
        let roots = disk
            .load_index_root_manifests(&mut fs, GRAPH_STATE_PROFILE_V1)
            .unwrap();
        assert_eq!(roots[0].revision().get(), 4);
        assert_eq!(roots[0].logical_state_digest(), &digest);
        assert!(
            roots
                .iter()
                .all(|root| [1, u64::from(graph_revision), 4].contains(&root.revision().get()))
        );
        let lookup = uste_storage::IndexGetLimits::new(16, 136).unwrap();
        let mut cache = PageCache::new(64 * 1024).unwrap();
        for revision in 1..=4_u8 {
            let outcome = disk
                .outcome(
                    &mut fs,
                    uste_policy::PrincipalDigest::from_bytes([0x90; 32]),
                    IdempotencyKey::from_bytes([revision; 16]),
                    UtcInstant::new(5, 0).unwrap(),
                    lookup,
                    &mut cache,
                )
                .unwrap()
                .unwrap();
            assert_eq!(outcome.revision.get(), u64::from(revision));
        }
        assert_metadata_denial_precedes_disk_io(&disk, &mut fs);
        drop(disk);
        fs.restart().unwrap();
        let input = prepare_mode(&mut fs, &name, true);
        let (mut disk, report) = recover_stream(&mut fs, input, 0, 3 * 8322).unwrap();
        assert_eq!(report.revisions, 0);
        assert_eq!(disk.state().unwrap().revision().get(), 4);
        if graph_revision == 1 {
            let read = IndexRunReadLimits::new(100, 100, 1024 * 1024).unwrap();
            let merge = IndexRunMergeLimits::new(read, 100, 1024 * 1024, 100, 1024 * 1024).unwrap();
            disk.rebase_metadata(
                &mut fs,
                uste_txn::CoordinatorMetadataRebaseLimits { merge, reuse: read },
            )
            .unwrap();
            assert_eq!(disk.overlay_counts(), (0, 0));
            let encoded = encode_transaction(&GraphTransaction::new(
                scope(),
                vec![Operation::ReplaceEntity {
                    target: record(1),
                    expected: Expected::Version(uste_graph::RecordVersion::FIRST),
                    properties: Value::Bool(true),
                }],
            ))
            .unwrap();
            let request = TransactionRequest {
                principal: uste_policy::PrincipalDigest::from_bytes([0x90; 32]),
                idempotency_key: IdempotencyKey::from_bytes([2; 16]),
                transaction_id: TransactionId::from_bytes([34; 16]),
                canonical_request: &encoded,
                blob_inventory: None,
            };
            let retry = disk
                .commit(
                    &mut fs,
                    request,
                    &mut clock(5),
                    &NeverCancel,
                    lookup,
                    &mut cache,
                )
                .unwrap();
            assert_eq!(retry.revision.get(), 2);
            assert_eq!(
                disk.commit(
                    &mut fs,
                    TransactionRequest {
                        idempotency_key: IdempotencyKey::from_bytes([9; 16]),
                        ..request
                    },
                    &mut clock(5),
                    &NeverCancel,
                    lookup,
                    &mut cache
                ),
                Err(uste_txn::TransactionError::Conflict)
            );
            let transaction = GraphTransaction::new(
                scope(),
                vec![Operation::ReplaceEntity {
                    target: record(1),
                    expected: Expected::Version(uste_graph::RecordVersion::new(4).unwrap()),
                    properties: Value::Null,
                }],
            );
            let encoded = encode_transaction(&transaction).unwrap();
            let proof = uste_graph::load_graph_disk_coordinator_preparation_view(
                &disk,
                &mut fs,
                transaction,
                GraphDiskPreparationLimits::new(8, 8, 8, 8, 1024 * 1024).unwrap(),
                &mut cache,
            )
            .unwrap()
            .prepare()
            .unwrap();
            let prepared = prepare_graph_disk_commit(
                proof,
                GraphStateDeltaLimits::new(100, 1024 * 1024).unwrap(),
            )
            .unwrap();
            let outcome = disk
                .commit_prepared(
                    &mut fs,
                    TransactionRequest {
                        principal: request.principal,
                        idempotency_key: IdempotencyKey::from_bytes([5; 16]),
                        transaction_id: TransactionId::from_bytes([37; 16]),
                        canonical_request: &encoded,
                        blob_inventory: None,
                    },
                    prepared,
                    &mut clock(5),
                    &NeverCancel,
                    lookup,
                    &mut cache,
                )
                .unwrap();
            assert_eq!(outcome.revision.get(), 5);
            uste_graph::publish_graph_disk_coordinator_base(
                &mut disk,
                &mut fs,
                outcome,
                GraphStateRootMergeLimits::uniform(merge, 2 * 1024 * 1024).unwrap(),
            )
            .unwrap();
            assert_eq!(
                disk.state()
                    .unwrap()
                    .current_base()
                    .unwrap()
                    .revision()
                    .get(),
                5
            );
        }
    }
}

#[test]
fn streamed_graph_suffix_admits_total_count_and_never_publishes_a_partial_byte_budget() {
    for (count, bytes) in [(2, 3 * 8322), (3, 3 * 8322 - 1)] {
        let (mut fs, name, _) = fixture_with_suffix(3, 1);
        let input = prepare_mode(&mut fs, &name, true);
        fs.arm(FaultPlan::default()).unwrap();
        assert!(recover_stream(&mut fs, input, count, bytes).is_err());
        if count == 2 {
            assert_eq!(fs.operation_count(FaultOperation::ReadAt), 0);
            assert_eq!(fs.operation_count(FaultOperation::CreateNew), 0);
        } else {
            assert!(fs.operation_count(FaultOperation::CreateNew) > 0);
        }
        fs.restart().unwrap();
        let input = prepare_mode(&mut fs, &name, true);
        assert_eq!(input.state.current_base().unwrap().revision().get(), 1);
        let (disk, report) = recover_stream(&mut fs, input, 3, 3 * 8322).unwrap();
        assert_eq!(disk.state().unwrap().revision().get(), 4);
        assert_eq!(report.revisions, 3);
    }
}

#[test]
fn streamed_graph_suffix_every_io_error_and_crash_keeps_only_old_or_terminal_roots() {
    streamed_graph_fault_matrix(false);
}

#[test]
fn streamed_graph_suffix_disk_certificates_every_io_error_and_crash_keeps_only_terminal_roots() {
    streamed_graph_fault_matrix(true);
}

fn streamed_graph_fault_matrix(disk_certificates: bool) {
    let bytes = 3 * 8322 + if disk_certificates { 6 * 4161 } else { 0 };
    let (mut baseline, name, digest) = fixture_with_suffix(3, 1);
    let input = prepare_certificate_mode(&mut baseline, &name, true, disk_certificates);
    baseline.arm(FaultPlan::default()).unwrap();
    let (disk, _) = recover_stream(&mut baseline, input, 3, bytes).unwrap();
    if disk_certificates {
        assert_eq!(disk.certificate_anchor_residency(), (false, 0));
    }
    drop(disk);
    for operation in [
        FaultOperation::OpenExisting,
        FaultOperation::Metadata,
        FaultOperation::ReadAt,
        FaultOperation::CreateNew,
        FaultOperation::WriteAt,
        FaultOperation::SetLen,
        FaultOperation::SyncAll,
        FaultOperation::SyncDirectory,
        FaultOperation::RenameNoReplace,
        FaultOperation::RemoveFile,
        FaultOperation::SyncData,
    ] {
        let count = baseline.operation_count(operation);
        eprintln!(
            "streamed_graph_recovery disk_certificates={disk_certificates} operation={operation:?} boundaries={count} fault_cases={}",
            count * 3
        );
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut fs, name, _) = fixture_with_suffix(3, 1);
                let input = prepare_certificate_mode(&mut fs, &name, true, disk_certificates);
                fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                let result = recover_stream(&mut fs, input, 3, bytes);
                if action == FaultAction::CrashAfter && !fs.is_crashed() {
                    // The adapter only crashes after successful operations. An optional missing
                    // root-slot open has no successful boundary; do not call it an injected crash.
                    assert!(matches!(
                        operation,
                        FaultOperation::OpenExisting | FaultOperation::RemoveFile
                    ));
                    let (disk, _) = result.unwrap();
                    assert_eq!(
                        disk.load_index_root_manifests(&mut fs, GRAPH_STATE_PROFILE_V1)
                            .unwrap()[0]
                            .logical_state_digest(),
                        &digest
                    );
                    eprintln!(
                        "no_crash_after_optional_error operation={operation:?} occurrence={occurrence}"
                    );
                    drop(disk);
                } else {
                    assert!(result.is_err(), "{operation:?}/{occurrence}/{action:?}");
                }
                assert_eq!(fs.pending_faults(), 0);
                fs.restart().unwrap();
                let input = prepare_certificate_mode(&mut fs, &name, true, disk_certificates);
                assert!([1, 4].contains(&input.state.revision().get()));
                let (disk, _) = recover_stream(&mut fs, input, 3, bytes).unwrap();
                if disk_certificates {
                    assert_eq!(disk.certificate_anchor_residency(), (false, 0));
                }
                let roots = disk
                    .load_index_root_manifests(&mut fs, GRAPH_STATE_PROFILE_V1)
                    .unwrap();
                assert_eq!(roots[0].logical_state_digest(), &digest);
                assert!(
                    roots
                        .iter()
                        .all(|root| [1, 4].contains(&root.revision().get()))
                );
            }
        }
    }
}

#[test]
fn streamed_graph_suffix_late_certificate_corruption_discards_all_stages() {
    let (mut fs, name, digest) = fixture_with_suffix(3, 1);
    let input = prepare_mode(&mut fs, &name, true);
    let directory = fs.open_directory(&fs.root(), &name).unwrap();
    let certificates = fs
        .open_existing(&directory, &EntryName::new("CERTIFICATES").unwrap())
        .unwrap();
    let offset = 4 * 4161 + 100;
    let mut byte = [0];
    assert_eq!(fs.read_at(&certificates, offset, &mut byte).unwrap(), 1);
    byte[0] ^= 1;
    assert_eq!(fs.write_at(&certificates, offset, &byte).unwrap(), 1);
    fs.arm(FaultPlan::default()).unwrap();
    assert!(recover_stream(&mut fs, input, 3, 3 * 8322).is_err());
    assert!(fs.operation_count(FaultOperation::CreateNew) > 0);
    byte[0] ^= 1;
    assert_eq!(fs.write_at(&certificates, offset, &byte).unwrap(), 1);
    fs.restart().unwrap();
    let input = prepare_mode(&mut fs, &name, true);
    assert_eq!(input.state.revision().get(), 1);
    let (disk, _) = recover_stream(&mut fs, input, 3, 3 * 8322).unwrap();
    assert_eq!(
        disk.load_index_root_manifests(&mut fs, GRAPH_STATE_PROFILE_V1)
            .unwrap()[0]
            .logical_state_digest(),
        &digest
    );
}

struct WrongPreparedDomain {
    prepared: Option<uste_graph::GraphDiskCommit>,
    calls: [usize; 3],
}

impl
    uste_txn::DiskRecoveryDomain<
        GraphDiskLiveState,
        FaultFs,
        TestEnvelope,
        CounterEntropy,
        CounterEntropy,
    > for WrongPreparedDomain
{
    fn admit(&mut self, revisions: u64) -> Result<(), uste_storage::journal::StorageError> {
        assert_eq!(revisions, 1);
        Ok(())
    }
    fn prepare(
        &mut self,
        _: &Recovery,
        _: &mut FaultFs,
        _: &GraphDiskLiveState,
        _: &uste_txn::RecoveredFrontierTransaction,
        _: &mut PageCache,
    ) -> Result<uste_graph::GraphDiskCommit, uste_storage::journal::StorageError> {
        self.calls[0] += 1;
        Ok(self.prepared.take().unwrap())
    }
    fn advance(
        &mut self,
        _: &mut Recovery,
        _: &mut FaultFs,
        _: &mut GraphDiskLiveState,
        _: &uste_txn::RecoveredFrontierTransaction,
        _: &mut PageCache,
    ) -> Result<(), uste_storage::journal::StorageError> {
        self.calls[1] += 1;
        Err(uste_storage::journal::StorageError::IntegrityFailure)
    }
    fn finish(
        &mut self,
        _: &mut Recovery,
        _: &mut FaultFs,
        _: &mut GraphDiskLiveState,
        _: (CommitRevision, [u8; 32]),
        _: &mut PageCache,
    ) -> Result<(), uste_storage::journal::StorageError> {
        self.calls[2] += 1;
        Err(uste_storage::journal::StorageError::IntegrityFailure)
    }
}

#[test]
fn streamed_graph_suffix_core_rejects_wrong_preparation_before_domain_advance() {
    let (mut other_fs, other_name, _) = fixture_variation(1, 1, true);
    let mut other = prepare_mode(&mut other_fs, &other_name, true);
    let revision = CommitRevision::new(2).unwrap();
    let mut cursor = other
        .recovery
        .open_transaction_cursor(revision, revision, 1, 8322)
        .unwrap();
    let transaction = other
        .recovery
        .next_recovered_transaction(&mut other_fs, &mut cursor)
        .unwrap()
        .unwrap();
    let proof = load_graph_disk_recovery_preparation_view(
        &other.recovery,
        &mut other_fs,
        other.state.current_base().unwrap(),
        &transaction,
        GraphDiskPreparationLimits::new(8, 8, 8, 8, 1024 * 1024).unwrap(),
        &mut other.cache,
    )
    .unwrap()
    .prepare()
    .unwrap();
    let prepared =
        prepare_graph_disk_commit(proof, GraphStateDeltaLimits::new(100, 1024 * 1024).unwrap())
            .unwrap();
    let mut domain = WrongPreparedDomain {
        prepared: Some(prepared),
        calls: [0; 3],
    };
    let (mut fs, name, _) = fixture();
    let mut input = prepare_mode(&mut fs, &name, true);
    fs.arm(FaultPlan::default()).unwrap();
    let result = Disk::recover_with_streaming_domain(
        input.recovery,
        &mut fs,
        input.metadata,
        input.state,
        RetentionDays::new(30).unwrap(),
        uste_txn::DiskCoordinatorRecoveryLimits {
            overlay: uste_txn::CoordinatorRecoveryLimits::new(1, 0).unwrap(),
            lookup: uste_storage::IndexGetLimits::new(16, 136).unwrap(),
            maximum_encoded_bytes: 8322,
        },
        &mut input.cache,
        &mut domain,
    );
    assert!(matches!(
        result,
        Err(uste_txn::TransactionError::IntegrityFailure)
    ));
    assert_eq!(domain.calls, [1, 0, 0]);
    assert_eq!(fs.operation_count(FaultOperation::CreateNew), 0);
}

#[test]
fn streamed_graph_suffix_ready_root_resync_is_bounded_and_does_not_rotate_slots() {
    let (mut baseline, name, _) = fixture_with_suffix(3, 4);
    let input = prepare_mode(&mut baseline, &name, true);
    let generation = input.state.current_base().unwrap().generation();
    baseline.arm(FaultPlan::default()).unwrap();
    let (disk, report) = recover_stream(&mut baseline, input, 0, 3 * 8322).unwrap();
    assert_eq!(report.revisions, 0);
    assert_eq!(
        disk.state().unwrap().current_base().unwrap().generation(),
        generation
    );
    assert_eq!(baseline.operation_count(FaultOperation::CreateNew), 0);
    assert_eq!(baseline.operation_count(FaultOperation::WriteAt), 0);
    drop(disk);
    for operation in [FaultOperation::SyncAll, FaultOperation::SyncDirectory] {
        let count = baseline.operation_count(operation);
        assert!(count > 0);
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut fs, name, digest) = fixture_with_suffix(3, 4);
                let input = prepare_mode(&mut fs, &name, true);
                fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                assert!(recover_stream(&mut fs, input, 0, 3 * 8322).is_err());
                assert_eq!(fs.pending_faults(), 0);
                fs.restart().unwrap();
                let input = prepare_mode(&mut fs, &name, true);
                let (disk, _) = recover_stream(&mut fs, input, 0, 3 * 8322).unwrap();
                assert_eq!(
                    disk.state().unwrap().current_base().unwrap().generation(),
                    generation
                );
                assert_eq!(
                    disk.load_index_root_manifests(&mut fs, GRAPH_STATE_PROFILE_V1)
                        .unwrap()[0]
                        .logical_state_digest(),
                    &digest
                );
            }
        }
    }
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
        uste_graph::GraphDiskReadLimits {
            expansion: None,
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
