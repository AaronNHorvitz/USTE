use super::*;
#[path = "packed_preparation.rs"]
mod preparation;
use uste_graph::{
    GRAPH_ORDERED_STATE_PROFILE_V1, GRAPH_PACKED_PROFILE_V1, PackedGraphBridgeLimits,
    bridge_graph_base_to_packed, packed_graph_v1_digest,
};
use uste_storage::{
    fault::{FaultFileSystem, FaultPlan},
    journal::CertificateAnchorReadLimits,
    packed_index_pack::PackWriteLimits,
    packed_tree_batch::TreeBatchLimits,
    packed_tree_cursor::TreeCursorLimits,
};
type Fs = FaultFileSystem<MemoryFileSystem>;
type Recovery = AuthenticatedIndexRecovery<Fs, TestEnvelope, CounterEntropy, CounterEntropy>;
use uste_storage::fault::{FaultAction, FaultPoint, Operation as FsOp};

fn limits(batch: usize) -> PackedGraphBridgeLimits {
    PackedGraphBridgeLimits {
        certificates: CertificateAnchorReadLimits::new(64, 64 * 4161).unwrap(),
        source: IndexRunReadLimits::new(1000, 10000, 16 * 1024 * 1024).unwrap(),
        batch: TreeBatchLimits {
            maximum_deltas: 512,
            maximum_input_bytes: 4 * 1024 * 1024,
            maximum_dirty_nodes: 8192,
            maximum_path_branches: 512,
            maximum_read_pages: 10000,
            maximum_read_bytes: 10000 * 20545,
            pack: PackWriteLimits {
                maximum_pages: 1000,
                maximum_records: 10000,
                maximum_payload_bytes: 16 * 1024 * 1024,
            },
        },
        entries_per_batch: batch,
        maximum_batches: 10000,
        maximum_packed_read_pages: 100000,
        maximum_written_pages: 10000,
    }
}
fn export_limits() -> TreeCursorLimits {
    TreeCursorLimits {
        maximum_path_branches: 512,
        maximum_candidates: 10000,
        maximum_returned_bytes: 16 * 1024 * 1024,
        maximum_pages: 100000,
        maximum_encoded_bytes: 100000 * 20545,
    }
}
fn fixture() -> (MemoryFileSystem, EntryName, [u8; 32]) {
    let mut fs = MemoryFileSystem::new(64 * 1024 * 1024);
    let name = EntryName::new("packed-graph-bridge").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 910_000),
        CounterEntropy(920_000),
        GraphState::new(scope()),
    )
    .unwrap();
    commit(
        &mut coordinator,
        &mut fs,
        1,
        GraphTransaction::with_policy_mutation(
            scope(),
            vec![
                Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Entity(NewEntity {
                        id: record(1),
                        entity_type: text("node"),
                        schema_version: 1,
                        properties: Value::Null,
                    }),
                },
                Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Entity(NewEntity {
                        id: record(2),
                        entity_type: text("node"),
                        schema_version: 1,
                        properties: Value::Null,
                    }),
                },
                Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Evidence(NewEvidence {
                        id: record(3),
                        digest: [83; 32],
                        locator: text("fixture://packed-graph"),
                    }),
                },
            ],
            DurablePolicyMutation::Install {
                policy: NamespacePolicy::new(
                    scope(),
                    PolicyVersion::new(1).unwrap(),
                    QuotaLimits::new(100, 1024 * 1024, 1024 * 1024, 8, 1024).unwrap(),
                ),
            },
        ),
    );
    commit(
        &mut coordinator,
        &mut fs,
        2,
        GraphTransaction::new(
            scope(),
            vec![Operation::Create {
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
            }],
        ),
    );
    commit(
        &mut coordinator,
        &mut fs,
        3,
        GraphTransaction::new(
            scope(),
            vec![Operation::ActOnRelationship {
                target: record(4),
                expected: Expected::Version(uste_graph::RecordVersion::FIRST),
                action: AssertionAction::Accept,
                correction: None,
                correction_expected: None,
            }],
        ),
    );
    let snapshot = coordinator.read_view().unwrap().state().clone();
    let digest = GraphState::logical_state_digest(&snapshot).unwrap();
    publish_graph_state_root(&mut coordinator, &mut fs, &snapshot).unwrap();
    drop(coordinator);
    fs.restart().unwrap();
    (fs, name, digest)
}
fn open(
    fs: &mut Fs,
    name: &EntryName,
    entropy: u64,
) -> (
    Recovery,
    uste_graph::GraphDiskBase,
    uste_txn::RecoveredFrontierTransaction,
) {
    let (recovery, _, transaction) = AuthenticatedIndexRecovery::open_with_frontier_transaction(
        fs,
        name,
        scope(),
        CounterEntropy(entropy),
        CounterEntropy(entropy + 10_000),
        &mut TestKeyAdapter,
    )
    .unwrap();
    let candidates = load_graph_state_root_candidates_for_recovery(&recovery, fs).unwrap();
    let (base, _) = admit_graph_disk_base_candidate_for_recovery(
        &recovery,
        fs,
        &candidates[0],
        admission_limits(),
        &mut PageCache::new(1024 * 1024).unwrap(),
    )
    .unwrap();
    (recovery, base, transaction.unwrap())
}

#[test]
fn packed_graph_bridge_partition_independence_and_exact_v1_export() {
    let mut commitments = None;
    for batch in [1, 2, 3, 512] {
        let (memory, name, digest) = fixture();
        let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
        let (mut recovery, source, transaction) = open(&mut fs, &name, 930_000);
        let (base, report) = bridge_graph_base_to_packed(
            &mut recovery,
            &mut fs,
            &source,
            &transaction,
            limits(batch),
        )
        .unwrap();
        assert_eq!(base.scope(), scope());
        assert_eq!(
            base.anchor(),
            (transaction.revision(), *transaction.certificate_digest())
        );
        assert_eq!(base.source_v1_digest(), Some(&digest));
        assert_eq!(base.namespace_policy(), source.namespace_policy());
        assert!(report.peak_batch_entries <= batch);
        assert_eq!(
            packed_graph_v1_digest(
                &mut recovery,
                &mut fs,
                &base,
                &transaction,
                limits(batch).certificates,
                export_limits(),
                4 * 1024 * 1024
            )
            .unwrap(),
            digest
        );
        let claims = base.publication_claims();
        assert_eq!(
            claims.state_commitment_profile,
            GRAPH_ORDERED_STATE_PROFILE_V1
        );
        assert_ne!(claims.state_digest, digest);
        let logical = base.families().map(|f| f.commitment);
        assert!(logical.iter().all(|c| c.entries() > 0));
        if let Some(expected) = commitments {
            assert_eq!(logical, expected);
        } else {
            commitments = Some(logical);
        }
        recovery
            .publish_recovered_packed_root(
                &mut fs,
                GRAPH_PACKED_PROFILE_V1,
                claims,
                &base.families(),
                2,
            )
            .unwrap();
    }
}

#[test]
fn packed_graph_bridge_exact_and_minus_one_aggregate_limits() {
    let (memory, name, _) = fixture();
    let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
    let (mut recovery, source, transaction) = open(&mut fs, &name, 930_000);
    let (_, report) =
        bridge_graph_base_to_packed(&mut recovery, &mut fs, &source, &transaction, limits(2))
            .unwrap();
    let exact = PackedGraphBridgeLimits {
        source: IndexRunReadLimits::new(
            report.source_pages,
            report.source_entries,
            report.source_logical_bytes,
        )
        .unwrap(),
        maximum_batches: report.batches,
        maximum_packed_read_pages: report.packed_read_pages,
        maximum_written_pages: report.written_pages,
        ..limits(2)
    };
    drop(recovery);
    for index in 0..9 {
        let (memory, name, digest) = fixture();
        let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
        let (mut recovery, source, transaction) = open(&mut fs, &name, 930_000);
        let mut selected = exact;
        match index {
            0 => {}
            1 => {
                selected.source = IndexRunReadLimits::new(
                    report.source_pages - 1,
                    report.source_entries,
                    report.source_logical_bytes,
                )
                .unwrap()
            }
            2 => {
                selected.source = IndexRunReadLimits::new(
                    report.source_pages,
                    report.source_entries - 1,
                    report.source_logical_bytes,
                )
                .unwrap()
            }
            3 => {
                selected.source = IndexRunReadLimits::new(
                    report.source_pages,
                    report.source_entries,
                    report.source_logical_bytes - 1,
                )
                .unwrap()
            }
            4 => selected.maximum_batches -= 1,
            5 => selected.maximum_packed_read_pages -= 1,
            6 => selected.maximum_written_pages -= 1,
            7 => selected.entries_per_batch = 0,
            8 => selected.entries_per_batch = 513,
            _ => unreachable!(),
        }
        fs.arm(FaultPlan::default()).unwrap();
        let result =
            bridge_graph_base_to_packed(&mut recovery, &mut fs, &source, &transaction, selected);
        if index == 0 {
            let (base, actual) = result.unwrap();
            assert_eq!(actual, report);
            assert_eq!(
                packed_graph_v1_digest(
                    &mut recovery,
                    &mut fs,
                    &base,
                    &transaction,
                    exact.certificates,
                    export_limits(),
                    4 * 1024 * 1024
                )
                .unwrap(),
                digest
            );
            let mut narrow = export_limits();
            narrow.maximum_returned_bytes = report.source_logical_bytes - 1;
            assert!(
                packed_graph_v1_digest(
                    &mut recovery,
                    &mut fs,
                    &base,
                    &transaction,
                    exact.certificates,
                    narrow,
                    4 * 1024 * 1024
                )
                .is_err()
            );
            narrow = export_limits();
            narrow.maximum_candidates = report.source_entries - 1;
            assert!(
                packed_graph_v1_digest(
                    &mut recovery,
                    &mut fs,
                    &base,
                    &transaction,
                    exact.certificates,
                    narrow,
                    4 * 1024 * 1024
                )
                .is_err()
            );
            assert!(
                packed_graph_v1_digest(
                    &mut recovery,
                    &mut fs,
                    &base,
                    &transaction,
                    exact.certificates,
                    export_limits(),
                    1
                )
                .is_err()
            );
        } else {
            assert!(result.is_err(), "limit {index}");
            if [1, 2, 4, 7, 8].contains(&index) {
                assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
                assert_eq!(fs.operation_count(FsOp::CreateNew), 0);
            }
        }
    }
}

#[test]
fn packed_graph_bridge_every_observed_fault_retains_source_and_restarts() {
    let (memory, name, _) = fixture();
    let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
    let (mut recovery, source, transaction) = open(&mut fs, &name, 930_000);
    fs.arm(FaultPlan::default()).unwrap();
    let (base, _) =
        bridge_graph_base_to_packed(&mut recovery, &mut fs, &source, &transaction, limits(2))
            .unwrap();
    let expected = base.publication_claims().state_digest;
    let observed = [
        FsOp::OpenExisting,
        FsOp::Metadata,
        FsOp::ReadAt,
        FsOp::CreateNew,
        FsOp::WriteAt,
        FsOp::SetLen,
        FsOp::SyncAll,
        FsOp::SyncDirectory,
    ]
    .map(|op| (op, fs.operation_count(op)));
    drop(recovery);
    let mut cases = 0;
    for (operation, count) in observed {
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (memory, name, digest) = fixture();
                let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
                let (mut recovery, source, transaction) = open(&mut fs, &name, 930_000);
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
                    bridge_graph_base_to_packed(
                        &mut recovery,
                        &mut fs,
                        &source,
                        &transaction,
                        limits(2)
                    )
                    .is_err(),
                    "{operation:?} {occurrence} {action:?}"
                );
                assert_eq!(fs.pending_faults(), 0);
                drop(recovery);
                fs.restart().unwrap();
                let (mut recovery, source, transaction) = open(&mut fs, &name, 960_000);
                let (base, _) = bridge_graph_base_to_packed(
                    &mut recovery,
                    &mut fs,
                    &source,
                    &transaction,
                    limits(2),
                )
                .unwrap();
                assert_eq!(base.publication_claims().state_digest, expected);
                assert_eq!(
                    packed_graph_v1_digest(
                        &mut recovery,
                        &mut fs,
                        &base,
                        &transaction,
                        limits(2).certificates,
                        export_limits(),
                        4 * 1024 * 1024
                    )
                    .unwrap(),
                    digest
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 405);
}

#[test]
fn packed_graph_bridge_empty_families_and_exact_terminal_write_budget() {
    let mut memory = MemoryFileSystem::new(16 * 1024 * 1024);
    let name = EntryName::new("packed-empty-graph").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut memory,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 910_000),
        CounterEntropy(920_000),
        GraphState::new(scope()),
    )
    .unwrap();
    commit(
        &mut coordinator,
        &mut memory,
        1,
        GraphTransaction::with_policy_mutation(
            scope(),
            vec![],
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
    let digest = GraphState::logical_state_digest(&snapshot).unwrap();
    publish_graph_state_root(&mut coordinator, &mut memory, &snapshot).unwrap();
    drop(coordinator);
    memory.restart().unwrap();
    let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
    let (mut recovery, source, transaction) = open(&mut fs, &name, 930_000);
    let (base, report) =
        bridge_graph_base_to_packed(&mut recovery, &mut fs, &source, &transaction, limits(512))
            .unwrap();
    assert_eq!(report.batches, 8);
    assert_eq!(report.written_pages, 2);
    assert_eq!(
        base.families().map(|f| f.commitment.entries()),
        [1, 0, 0, 0, 0, 0, 0, 2]
    );
    assert_eq!(
        packed_graph_v1_digest(
            &mut recovery,
            &mut fs,
            &base,
            &transaction,
            limits(512).certificates,
            export_limits(),
            1
        )
        .unwrap(),
        digest
    );
    let (again, work) = bridge_graph_base_to_packed(
        &mut recovery,
        &mut fs,
        &source,
        &transaction,
        PackedGraphBridgeLimits {
            maximum_written_pages: 2,
            maximum_packed_read_pages: 0,
            maximum_batches: 8,
            ..limits(512)
        },
    )
    .unwrap();
    assert_eq!(work, report);
    assert_eq!(
        again.publication_claims().state_digest,
        base.publication_claims().state_digest
    );
}

#[test]
fn packed_graph_bridge_late_packed_corruption_and_foreign_owner_refuse_export() {
    use uste_storage::FileSystem;
    let (memory, name, digest) = fixture();
    let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
    let (mut recovery, source, transaction) = open(&mut fs, &name, 930_000);
    let (base, _) =
        bridge_graph_base_to_packed(&mut recovery, &mut fs, &source, &transaction, limits(2))
            .unwrap();
    let (foreign_memory, foreign_name, _) = fixture();
    let mut foreign_fs = FaultFileSystem::new(foreign_memory, FaultPlan::default());
    let (mut foreign, _, foreign_transaction) = open(&mut foreign_fs, &foreign_name, 970_000);
    assert!(
        packed_graph_v1_digest(
            &mut foreign,
            &mut foreign_fs,
            &base,
            &foreign_transaction,
            limits(2).certificates,
            export_limits(),
            4 * 1024 * 1024
        )
        .is_err()
    );
    assert_eq!(
        packed_graph_v1_digest(
            &mut recovery,
            &mut fs,
            &base,
            &transaction,
            limits(2).certificates,
            export_limits(),
            4 * 1024 * 1024
        )
        .unwrap(),
        digest
    );
    let family = base.families()[7];
    let physical = family
        .root
        .unwrap()
        .resolve(scope(), GRAPH_PACKED_PROFILE_V1, 8, transaction.revision())
        .unwrap();
    let file_name = EntryName::new(format!(
        "pack-{}",
        physical
            .object
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ))
    .unwrap();
    let directory = fs.open_directory(&fs.root(), &name).unwrap();
    let file = fs.open_existing(&directory, &file_name).unwrap();
    let offset = physical.page * 20545 + 137;
    let mut byte = [0];
    assert_eq!(fs.read_at(&file, offset, &mut byte).unwrap(), 1);
    byte[0] ^= 1;
    assert_eq!(fs.write_at(&file, offset, &byte).unwrap(), 1);
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        packed_graph_v1_digest(
            &mut recovery,
            &mut fs,
            &base,
            &transaction,
            limits(2).certificates,
            export_limits(),
            4 * 1024 * 1024
        )
        .is_err()
    );
    assert!(fs.operation_count(FsOp::ReadAt) > 10);
    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
    byte[0] ^= 1;
    assert_eq!(fs.write_at(&file, offset, &byte).unwrap(), 1);
    assert_eq!(
        packed_graph_v1_digest(
            &mut recovery,
            &mut fs,
            &base,
            &transaction,
            limits(2).certificates,
            export_limits(),
            4 * 1024 * 1024
        )
        .unwrap(),
        digest
    );
}

#[test]
fn packed_graph_bridge_rechecks_late_source_ciphertext_after_semantic_admission() {
    use uste_storage::FileSystem;
    let (memory, name, digest) = fixture();
    let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
    let (mut recovery, source, transaction) = open(&mut fs, &name, 930_000);
    let directory = fs.open_directory(&fs.root(), &name).unwrap();
    // Fixture-only bounded enumeration of its deterministic identity-entropy outputs.
    // The source publisher emits eight ordered families; the last run is policy history.
    let mut files = Vec::new();
    for counter in 920_001_u64..=920_064 {
        let name = EntryName::new(format!("i-{counter:016x}{:016x}", counter + 1)).unwrap();
        match fs.open_existing(&directory, &name) {
            Ok(file) => files.push(file),
            Err(error) if error.kind() == uste_storage::AdapterErrorKind::NotFound => {}
            Err(error) => panic!("{error:?}"),
        }
    }
    assert_eq!(files.len(), 8);
    let file = files.last().unwrap();
    let mut byte = [0];
    assert_eq!(fs.read_at(file, 137, &mut byte).unwrap(), 1);
    byte[0] ^= 1;
    assert_eq!(fs.write_at(file, 137, &byte).unwrap(), 1);
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        bridge_graph_base_to_packed(&mut recovery, &mut fs, &source, &transaction, limits(2))
            .is_err()
    );
    assert!(fs.operation_count(FsOp::CreateNew) >= 7);
    byte[0] ^= 1;
    assert_eq!(fs.write_at(file, 137, &byte).unwrap(), 1);
    let (base, _) =
        bridge_graph_base_to_packed(&mut recovery, &mut fs, &source, &transaction, limits(2))
            .unwrap();
    assert_eq!(
        packed_graph_v1_digest(
            &mut recovery,
            &mut fs,
            &base,
            &transaction,
            limits(2).certificates,
            export_limits(),
            4 * 1024 * 1024
        )
        .unwrap(),
        digest
    );
}

#[test]
fn packed_graph_bridge_disk_certificate_owner_and_cold_published_family_admission() {
    let (memory, name, digest) = fixture();
    let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
    let (mut recovery, _, transaction) =
        AuthenticatedIndexRecovery::open_with_disk_certificate_anchors(
            &mut fs,
            &name,
            scope(),
            CounterEntropy(930_000),
            CounterEntropy(940_000),
            &mut TestKeyAdapter,
            limits(2).certificates,
        )
        .unwrap();
    let transaction = transaction.unwrap();
    let candidates = load_graph_state_root_candidates_for_recovery(&recovery, &mut fs).unwrap();
    let (source, _) = admit_graph_disk_base_candidate_for_recovery(
        &recovery,
        &mut fs,
        &candidates[0],
        admission_limits(),
        &mut PageCache::new(1024 * 1024).unwrap(),
    )
    .unwrap();
    let (base, _) =
        bridge_graph_base_to_packed(&mut recovery, &mut fs, &source, &transaction, limits(2))
            .unwrap();
    assert_eq!(
        packed_graph_v1_digest(
            &mut recovery,
            &mut fs,
            &base,
            &transaction,
            limits(2).certificates,
            export_limits(),
            4 * 1024 * 1024
        )
        .unwrap(),
        digest
    );
    let claims = base.publication_claims();
    let expected = base.families().map(|family| family.commitment);
    recovery
        .publish_recovered_packed_root(
            &mut fs,
            GRAPH_PACKED_PROFILE_V1,
            claims,
            &base.families(),
            2,
        )
        .unwrap();
    drop(recovery);
    fs.restart().unwrap();
    let (mut recovery, _, transaction) = open(&mut fs, &name, 960_000);
    let (roots, _) = recovery
        .discover_packed_roots_at_revision(
            &mut fs,
            GRAPH_PACKED_PROFILE_V1,
            transaction.revision(),
            limits(2).certificates,
            uste_storage::journal::PackedRootDiscoveryLimits::new(2, 2 * 4177).unwrap(),
        )
        .unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(
        roots[0].manifest().claims().state_digest,
        claims.state_digest
    );
    let maintenance = recovery
        .packed_indexes_with_io(&mut fs, &transaction, limits(2).certificates)
        .unwrap();
    for family in 1..=8 {
        let (tree, _) = maintenance
            .admit(
                &mut fs,
                &roots[0],
                family,
                uste_storage::packed_tree_validation::TreeValidationLimits {
                    maximum_path_branches: 512,
                    maximum_nodes: 10000,
                    maximum_logical_bytes: 16 * 1024 * 1024,
                    maximum_pages: 100000,
                    maximum_encoded_bytes: 100000 * 20545,
                },
            )
            .unwrap();
        assert_eq!(
            tree.family_descriptor().commitment,
            expected[usize::from(family - 1)]
        );
    }
    // Canonical per-family admission is not cold semantic PackedGraphBase admission.
}
