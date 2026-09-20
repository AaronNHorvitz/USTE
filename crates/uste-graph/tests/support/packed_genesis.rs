use super::*;
use uste_graph::{PackedGraphGenesisLimits, PackedGraphStageLimits, stage_packed_graph_genesis};
use uste_storage::journal::PackedRootDiscoveryLimits;
use uste_txn::RecoveredGenesis;

pub(super) fn genesis_limits(batch: usize) -> PackedGraphGenesisLimits {
    let old = limits(batch);
    PackedGraphGenesisLimits {
        stage: PackedGraphStageLimits {
            staging_cache_bytes: None,
            certificates: old.certificates,
            batch: old.batch,
            deltas_per_batch: batch,
            maximum_batches: old.maximum_batches,
            maximum_read_pages: old.maximum_packed_read_pages,
            maximum_written_pages: old.maximum_written_pages,
        },
        maximum_entries: 10000,
        maximum_logical_bytes: 16 * 1024 * 1024,
    }
}
fn reopen_genesis(fs: &mut Fs, entropy: u64) -> (Recovery, RecoveredGenesis<GraphState>) {
    fs.restart().unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    let (recovery, _, _) = AuthenticatedIndexRecovery::open_with_disk_certificate_anchors(
        fs,
        &EntryName::new("packed-genesis").unwrap(),
        scope(),
        CounterEntropy(entropy),
        CounterEntropy(entropy + 10000),
        &mut TestKeyAdapter,
        limits(2).certificates,
    )
    .unwrap();
    let genesis = recovery
        .recover_primary_genesis(fs, GraphState::new(scope()), 4 * 1024 * 1024, 512)
        .unwrap();
    (recovery, genesis)
}
fn genesis_fixture(kind: u8) -> (Fs, Recovery, RecoveredGenesis<GraphState>, [u8; 32]) {
    genesis_fixture_named(kind, "packed-genesis")
}
pub(super) fn genesis_fixture_named(
    kind: u8,
    name: &str,
) -> (Fs, Recovery, RecoveredGenesis<GraphState>, [u8; 32]) {
    let mut memory = MemoryFileSystem::new(64 * 1024 * 1024);
    let mut live = CommitCoordinator::create(
        &mut memory,
        scope(),
        RetentionDays::new(30).unwrap(),
        EntryName::new(name).unwrap(),
        create_vault(scope().database(), 2_100_000),
        CounterEntropy(2_110_000),
        GraphState::new(scope()),
    )
    .unwrap();
    let mut operations = Vec::new();
    if kind != 0 {
        for id in [1, 2] {
            operations.push(Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: record(id),
                    entity_type: text("genesis"),
                    schema_version: 1,
                    properties: if id == 1 {
                        Value::Bytes(uste_types::BoundedBytes::new(vec![37; 20_000]).unwrap())
                    } else {
                        Value::Null
                    },
                }),
            });
        }
        operations.push(Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Evidence(NewEvidence {
                id: record(3),
                digest: [83; 32],
                locator: text("fixture://genesis"),
            }),
        });
        operations.push(Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Relationship(NewRelationship {
                id: record(4),
                from: record(1),
                to: record(2),
                relationship_type: text("edge"),
                properties: Value::Null,
                evidence: vec![record(3)],
                valid_time: ValidTime::Unknown,
            }),
        });
    }
    let transaction = if kind == 1 {
        GraphTransaction::new(scope(), operations)
    } else {
        GraphTransaction::with_policy_mutation(
            scope(),
            operations,
            DurablePolicyMutation::Install {
                policy: NamespacePolicy::new(
                    scope(),
                    PolicyVersion::new(1).unwrap(),
                    QuotaLimits::new(100, 1024 * 1024, 1024 * 1024, 8, 1024).unwrap(),
                ),
            },
        )
    };
    commit(&mut live, &mut memory, 1, transaction);
    let expected = GraphState::logical_state_digest(live.read_view().unwrap().state()).unwrap();
    drop(live);
    let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
    fs.restart().unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    let (recovery, _, _) = AuthenticatedIndexRecovery::open_with_disk_certificate_anchors(
        &mut fs,
        &EntryName::new(name).unwrap(),
        scope(),
        CounterEntropy(2_120_000),
        CounterEntropy(2_130_000),
        &mut TestKeyAdapter,
        limits(2).certificates,
    )
    .unwrap();
    let genesis = recovery
        .recover_primary_genesis(&mut fs, GraphState::new(scope()), 4 * 1024 * 1024, 512)
        .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    (fs, recovery, genesis, expected)
}
fn assert_no_roots(recovery: &Recovery, fs: &mut Fs) {
    for profile in [
        GRAPH_PACKED_PROFILE_V1,
        uste_txn::COORDINATOR_PACKED_PROFILE_V1,
        uste_txn::COORDINATOR_PACKED_USAGE_PROFILE_V1,
    ] {
        let roots = recovery
            .discover_packed_roots_at_revision(
                fs,
                profile,
                CommitRevision::FIRST,
                limits(2).certificates,
                PackedRootDiscoveryLimits::new(8, 8 * 4177).unwrap(),
            )
            .unwrap()
            .0;
        assert!(roots.is_empty());
    }
    assert!(
        load_graph_state_root_candidates_for_recovery(recovery, fs)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn packed_genesis_reference_partition_policy_and_empty_families_need_no_v1_roots() {
    genesis_reference(None);
}
#[test]
fn packed_genesis_buffered_reference_partition_policy_and_empty_families_need_no_v1_roots() {
    genesis_reference(Some(uste_storage::MIN_INDEX_CACHE_BYTES));
}
fn genesis_reference(cache: Option<usize>) {
    for kind in 0..3 {
        let mut commitments = None;
        for batch in [1, 2, 512] {
            let (mut fs, mut recovery, genesis, expected) = genesis_fixture(kind);
            assert_no_roots(&recovery, &mut fs);
            let mut selected_limits = genesis_limits(batch);
            selected_limits.stage.staging_cache_bytes = cache;
            let (base, report) =
                stage_packed_graph_genesis(&mut recovery, &mut fs, &genesis, selected_limits)
                    .unwrap();
            assert_eq!(
                report.stage.buffered_batches,
                if cache.is_some() {
                    report.stage.batches
                } else {
                    0
                }
            );
            assert_eq!(
                base.anchor(),
                (
                    CommitRevision::FIRST,
                    *genesis.transaction().certificate_digest()
                )
            );
            assert_eq!(base.source_v1_digest(), Some(&expected));
            assert_eq!(base.namespace_policy().is_some(), kind != 1);
            assert!(report.stage.peak_batch_deltas <= batch);
            let logical: Vec<_> = base.families().into_iter().map(|f| f.commitment).collect();
            if let Some(previous) = &commitments {
                assert_eq!(&logical, previous);
            } else {
                commitments = Some(logical);
            }
            assert_eq!(
                packed_graph_v1_digest(
                    &mut recovery,
                    &mut fs,
                    &base,
                    genesis.transaction(),
                    limits(2).certificates,
                    export_limits(),
                    4 * 1024 * 1024
                )
                .unwrap(),
                expected
            );
            assert_no_roots(&recovery, &mut fs);
        }
    }
}

#[test]
fn packed_genesis_exact_and_minus_one_admission_preserve_unpublished_state() {
    let (mut fs, mut recovery, genesis, _) = genesis_fixture(2);
    let (_, report) =
        stage_packed_graph_genesis(&mut recovery, &mut fs, &genesis, genesis_limits(1)).unwrap();
    assert!(report.stage.read_pages > 0 && report.stage.written_pages > 0);
    let mut exact = genesis_limits(1);
    exact.maximum_entries = report.entries;
    exact.maximum_logical_bytes = report.logical_bytes;
    exact.stage.maximum_batches = report.stage.batches;
    exact.stage.maximum_read_pages = report.stage.read_pages;
    exact.stage.maximum_written_pages = report.stage.written_pages;
    for variant in 0..=5 {
        let (mut fs, mut recovery, genesis, _) = genesis_fixture(2);
        let mut narrowed = exact;
        match variant {
            0 => narrowed.maximum_entries -= 1,
            1 => narrowed.maximum_logical_bytes -= 1,
            2 => narrowed.stage.maximum_batches -= 1,
            3 => narrowed.stage.maximum_read_pages -= 1,
            4 => narrowed.stage.maximum_written_pages -= 1,
            _ => {}
        }
        let result = stage_packed_graph_genesis(&mut recovery, &mut fs, &genesis, narrowed);
        assert_eq!(result.is_ok(), variant == 5);
        if variant < 3 {
            assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
            assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
        }
        assert_no_roots(&recovery, &mut fs);
    }
    for batch in [0, 513] {
        let mut invalid = genesis_limits(1);
        invalid.stage.deltas_per_batch = batch;
        fs.arm(FaultPlan::default()).unwrap();
        assert!(stage_packed_graph_genesis(&mut recovery, &mut fs, &genesis, invalid).is_err());
        assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
        assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
    }
}

#[test]
fn packed_genesis_every_observed_staging_fault_preserves_origin_and_restarts() {
    let operations = [
        FsOp::CreateDirectory,
        FsOp::OpenDirectory,
        FsOp::CreateNew,
        FsOp::OpenExisting,
        FsOp::Metadata,
        FsOp::ReadAt,
        FsOp::WriteAt,
        FsOp::SetLen,
        FsOp::SyncData,
        FsOp::SyncAll,
        FsOp::RenameNoReplace,
        FsOp::RemoveFile,
        FsOp::SyncDirectory,
        FsOp::TryLockExclusive,
    ];
    let (mut fs, mut recovery, genesis, _) = genesis_fixture(2);
    stage_packed_graph_genesis(&mut recovery, &mut fs, &genesis, genesis_limits(2)).unwrap();
    let counts = operations.map(|operation| fs.operation_count(operation));
    let mut cases = 0;
    for (operation, count) in operations.into_iter().zip(counts) {
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut fs, mut recovery, genesis, expected) = genesis_fixture(2);
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
                    stage_packed_graph_genesis(&mut recovery, &mut fs, &genesis, genesis_limits(2))
                        .is_err()
                );
                assert_eq!(fs.pending_faults(), 0);
                drop(recovery);
                let (mut recovery, genesis) = reopen_genesis(&mut fs, 2_150_000);
                assert_eq!(
                    GraphState::logical_state_digest(genesis.state().current_snapshot()).unwrap(),
                    expected
                );
                assert_no_roots(&recovery, &mut fs);
                let (base, _) =
                    stage_packed_graph_genesis(&mut recovery, &mut fs, &genesis, genesis_limits(2))
                        .unwrap();
                assert_eq!(
                    packed_graph_v1_digest(
                        &mut recovery,
                        &mut fs,
                        &base,
                        genesis.transaction(),
                        limits(2).certificates,
                        export_limits(),
                        4 * 1024 * 1024
                    )
                    .unwrap(),
                    expected
                );
                assert_no_roots(&recovery, &mut fs);
                cases += 1;
            }
        }
    }
    assert!(cases > 0);
    eprintln!("packed genesis staging fault cases: {cases}");
}

#[test]
fn packed_genesis_owner_corruption_and_cold_canonical_admission_are_independent() {
    use uste_graph::{PackedGraphAdmissionLimits, admit_packed_graph_base};
    use uste_storage::FileSystem;
    use uste_storage::packed_tree_validation::TreeValidationLimits;
    let (mut fs, mut recovery, genesis, expected) = genesis_fixture(2);
    let (_, _other, foreign, _) = genesis_fixture(2);
    assert!(
        stage_packed_graph_genesis(&mut recovery, &mut fs, &foreign, genesis_limits(2)).is_err()
    );
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
    let (base, _) =
        stage_packed_graph_genesis(&mut recovery, &mut fs, &genesis, genesis_limits(2)).unwrap();
    let physical = base.families()[1]
        .root
        .unwrap()
        .resolve(scope(), GRAPH_PACKED_PROFILE_V1, 2, CommitRevision::FIRST)
        .unwrap();
    let directory = fs
        .open_directory(&fs.root(), &EntryName::new("packed-genesis").unwrap())
        .unwrap();
    let name = EntryName::new(format!(
        "pack-{}",
        physical
            .object
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ))
    .unwrap();
    let file = fs.open_existing(&directory, &name).unwrap();
    let offset = physical.page * 20545 + 137;
    let mut byte = [0];
    assert_eq!(fs.read_at(&file, offset, &mut byte).unwrap(), 1);
    byte[0] ^= 1;
    fs.write_at(&file, offset, &byte).unwrap();
    assert!(
        packed_graph_v1_digest(
            &mut recovery,
            &mut fs,
            &base,
            genesis.transaction(),
            limits(2).certificates,
            export_limits(),
            4 * 1024 * 1024
        )
        .is_err()
    );
    byte[0] ^= 1;
    fs.write_at(&file, offset, &byte).unwrap();
    let claims = base.publication_claims();
    recovery
        .publish_recovered_packed_root(
            &mut fs,
            GRAPH_PACKED_PROFILE_V1,
            claims,
            &base.families(),
            8,
        )
        .unwrap();
    drop(recovery);
    let (mut recovery, genesis) = reopen_genesis(&mut fs, 2_180_000);
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        packed_graph_v1_digest(
            &mut recovery,
            &mut fs,
            &base,
            genesis.transaction(),
            limits(2).certificates,
            export_limits(),
            4 * 1024 * 1024
        )
        .is_err()
    );
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
    let roots = recovery
        .discover_packed_roots_at_revision(
            &mut fs,
            GRAPH_PACKED_PROFILE_V1,
            CommitRevision::FIRST,
            limits(2).certificates,
            PackedRootDiscoveryLimits::new(8, 8 * 4177).unwrap(),
        )
        .unwrap()
        .0;
    assert_eq!(roots.len(), 1);
    let maintenance = recovery
        .packed_indexes_with_io(&mut fs, genesis.transaction(), limits(2).certificates)
        .unwrap();
    let (cold, _) = admit_packed_graph_base(
        &maintenance,
        &mut fs,
        &roots[0],
        PackedGraphAdmissionLimits {
            canonical: TreeValidationLimits {
                maximum_path_branches: 512,
                maximum_nodes: 10000,
                maximum_logical_bytes: 16 * 1024 * 1024,
                maximum_pages: 100000,
                maximum_encoded_bytes: 100000 * 20545,
            },
            semantic: admission_limits(),
            scan: export_limits(),
            lookup: uste_storage::packed_tree_lookup::TreeLookupLimits {
                maximum_path_branches: 512,
                maximum_pages: 10000,
                maximum_encoded_bytes: 10000 * 20545,
                maximum_value_bytes: 4 * 1024 * 1024,
            },
            maximum_lookup_encoded_bytes: 100000 * 20545,
        },
    )
    .unwrap();
    assert_eq!(cold.source_v1_digest(), Some(&expected));
    assert!(cold.publication_claims() == claims);
    assert!(cold.families() == base.families());
}
