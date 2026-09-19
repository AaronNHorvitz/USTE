use super::*;
use uste_graph::{PackedGraphPreparationLimits, prepare_packed_graph_transaction};
use uste_storage::packed_tree_lookup::TreeLookupLimits;
use uste_txn::TransactionState;

fn preparation_limits() -> PackedGraphPreparationLimits {
    PackedGraphPreparationLimits {
        proof: GraphDiskPreparationLimits::new(100, 1000, 100, 100, 4 * 1024 * 1024).unwrap(),
        lookup: TreeLookupLimits {
            maximum_path_branches: 512,
            maximum_pages: 10000,
            maximum_encoded_bytes: 10000 * 20545,
            maximum_value_bytes: 4 * 1024 * 1024,
        },
        maximum_point_lookups: 1000,
        maximum_pages: 100000,
        maximum_encoded_bytes: 100000 * 20545,
        maximum_scan_candidates: 10000,
    }
}
fn cases() -> Vec<GraphTransaction> {
    let version = Expected::Version(uste_graph::RecordVersion::FIRST);
    let mut operations = vec![
        Operation::ReplaceEntity {
            target: record(1),
            expected: version.clone(),
            properties: Value::RecordRef(record(2)),
        },
        Operation::ReplaceEntity {
            target: record(1),
            expected: Expected::Version(uste_graph::RecordVersion::new(2).unwrap()),
            properties: Value::Null,
        },
        Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Entity(NewEntity {
                id: record(1),
                entity_type: text("duplicate"),
                schema_version: 1,
                properties: Value::Null,
            }),
        },
        Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Entity(NewEntity {
                id: record(9),
                entity_type: text("valid"),
                schema_version: 1,
                properties: Value::RecordRef(record(2)),
            }),
        },
        Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Entity(NewEntity {
                id: record(9),
                entity_type: text("missing"),
                schema_version: 1,
                properties: Value::RecordRef(record(99)),
            }),
        },
        Operation::DeleteEntity {
            target: record(1),
            expected: version.clone(),
            policy: DeletePolicy::Reject,
            affected: vec![],
        },
        Operation::DeleteEntity {
            target: record(1),
            expected: version.clone(),
            policy: DeletePolicy::CascadeAndRetract {
                maximum_affected: 1,
            },
            affected: vec![record(4)],
        },
        Operation::DeleteEntity {
            target: record(1),
            expected: version.clone(),
            policy: DeletePolicy::CascadeAndRetract {
                maximum_affected: 1,
            },
            affected: vec![],
        },
        Operation::ActOnRelationship {
            target: record(4),
            expected: Expected::Version(uste_graph::RecordVersion::new(2).unwrap()),
            action: AssertionAction::Retract,
            correction: None,
            correction_expected: None,
        },
        Operation::ActOnRelationship {
            target: record(4),
            expected: Expected::Version(uste_graph::RecordVersion::new(2).unwrap()),
            action: AssertionAction::Accept,
            correction: None,
            correction_expected: None,
        },
    ];
    for revision in [1, 2, 3, 4] {
        for predicate in [
            Predicate::RecordAbsent(record(4)),
            Predicate::RecordVisible(record(4)),
        ] {
            operations.push(Operation::ReplaceEntity {
                target: record(1),
                expected: Expected::ReadView {
                    revision: CommitRevision::new(revision).unwrap(),
                    predicate,
                },
                properties: Value::Bool(true),
            });
        }
    }
    let mut cases: Vec<_> = operations
        .into_iter()
        .map(|op| GraphTransaction::new(scope(), vec![op]))
        .collect();
    cases.push(GraphTransaction::new(
        scope(),
        vec![Operation::ActOnRelationship {
            target: record(4),
            expected: Expected::Version(uste_graph::RecordVersion::new(2).unwrap()),
            action: AssertionAction::Correct,
            correction: Some(NewRelationship {
                id: record(5),
                from: record(2),
                to: record(1),
                relationship_type: text("corrected"),
                properties: Value::Bool(true),
                evidence: vec![record(3)],
                valid_time: ValidTime::Unknown,
            }),
            correction_expected: Some(Expected::Absent),
        }],
    ));
    cases.push(complex_request());
    cases.push(GraphTransaction::with_policy_mutation(
        scope(),
        vec![],
        DurablePolicyMutation::Replace {
            expected: PolicyVersion::new(1).unwrap(),
            policy: NamespacePolicy::new(
                scope(),
                PolicyVersion::new(2).unwrap(),
                QuotaLimits::new(99, 1024 * 1024, 1024 * 1024, 8, 1024).unwrap(),
            ),
        },
    ));
    cases
}

fn complex_request() -> GraphTransaction {
    GraphTransaction::new(
        scope(),
        vec![
            Operation::ReplaceEntity {
                target: record(1),
                expected: Expected::ReadView {
                    revision: CommitRevision::new(3).unwrap(),
                    predicate: Predicate::RecordVisible(record(4)),
                },
                properties: Value::Bool(true),
            },
            Operation::DeleteEntity {
                target: record(2),
                expected: Expected::Version(uste_graph::RecordVersion::FIRST),
                policy: DeletePolicy::CascadeAndRetract {
                    maximum_affected: 1,
                },
                affected: vec![record(4)],
            },
        ],
    )
}

#[test]
fn packed_graph_preparation_exact_limits_and_each_minus_one_refuse_partial_proofs() {
    let (memory, name, _) = fixture();
    let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
    let (mut recovery, source, transaction) = open(&mut fs, &name, 930_000);
    let (base, _) =
        bridge_graph_base_to_packed(&mut recovery, &mut fs, &source, &transaction, limits(2))
            .unwrap();
    let maintenance = recovery
        .packed_indexes_with_io(&mut fs, &transaction, limits(2).certificates)
        .unwrap();
    let (prepared, proof, work) = prepare_packed_graph_transaction(
        &maintenance,
        &mut fs,
        &base,
        complex_request(),
        preparation_limits(),
    )
    .unwrap();
    assert!(proof.history_versions >= 2);
    assert!(proof.reverse_references >= 1);
    assert!(proof.reference_visits > 1);
    let exact = PackedGraphPreparationLimits {
        proof: GraphDiskPreparationLimits::new(
            proof.record_proofs,
            proof.reference_visits.max(1),
            proof.history_versions,
            proof.reverse_references,
            proof.proof_logical_bytes,
        )
        .unwrap(),
        maximum_point_lookups: work.point_lookups,
        maximum_pages: work.pages,
        maximum_encoded_bytes: work.encoded_bytes,
        maximum_scan_candidates: work.scan_candidates,
        ..preparation_limits()
    };
    let (same, actual_proof, actual_work) =
        prepare_packed_graph_transaction(&maintenance, &mut fs, &base, complex_request(), exact)
            .unwrap();
    assert_eq!(same.result_digest(), prepared.result_digest());
    assert_eq!(actual_proof, proof);
    assert_eq!(actual_work, work);
    for index in 0..10 {
        let mut selected = exact;
        match index {
            0 => selected.maximum_point_lookups -= 1,
            1 => selected.maximum_pages -= 1,
            2 => selected.maximum_encoded_bytes -= 1,
            3 => selected.maximum_scan_candidates -= 1,
            4 => {
                selected.proof = GraphDiskPreparationLimits::new(
                    proof.record_proofs - 1,
                    1000,
                    100,
                    100,
                    4 * 1024 * 1024,
                )
                .unwrap()
            }
            5 => {
                selected.proof = GraphDiskPreparationLimits::new(
                    100,
                    1000,
                    proof.history_versions - 1,
                    100,
                    4 * 1024 * 1024,
                )
                .unwrap()
            }
            6 => {
                selected.proof = GraphDiskPreparationLimits::new(
                    100,
                    1000,
                    100,
                    proof.reverse_references - 1,
                    4 * 1024 * 1024,
                )
                .unwrap()
            }
            7 => {
                selected.proof = GraphDiskPreparationLimits::new(
                    100,
                    1000,
                    100,
                    100,
                    proof.proof_logical_bytes - 1,
                )
                .unwrap()
            }
            8 => selected.lookup.maximum_path_branches = 0,
            9 => {
                selected.proof = GraphDiskPreparationLimits::new(
                    100,
                    proof.reference_visits - 1,
                    100,
                    100,
                    4 * 1024 * 1024,
                )
                .unwrap()
            }
            _ => unreachable!(),
        }
        fs.arm(FaultPlan::default()).unwrap();
        assert!(
            prepare_packed_graph_transaction(
                &maintenance,
                &mut fs,
                &base,
                complex_request(),
                selected
            )
            .is_err(),
            "limit {index}"
        );
        assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
    }
}

#[test]
fn packed_graph_preparation_all_observed_read_faults_return_no_prepared_result() {
    let (memory, name, _) = fixture();
    let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
    let (mut recovery, source, transaction) = open(&mut fs, &name, 930_000);
    let (base, _) =
        bridge_graph_base_to_packed(&mut recovery, &mut fs, &source, &transaction, limits(2))
            .unwrap();
    let maintenance = recovery
        .packed_indexes_with_io(&mut fs, &transaction, limits(2).certificates)
        .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    let expected = prepare_packed_graph_transaction(
        &maintenance,
        &mut fs,
        &base,
        complex_request(),
        preparation_limits(),
    )
    .unwrap()
    .0
    .result_digest();
    let observed =
        [FsOp::OpenExisting, FsOp::Metadata, FsOp::ReadAt].map(|op| (op, fs.operation_count(op)));
    drop(maintenance);
    drop(recovery);
    let mut cases = 0;
    for (operation, count) in observed {
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (memory, name, _) = fixture();
                let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
                let (mut recovery, source, transaction) = open(&mut fs, &name, 930_000);
                let (base, _) = bridge_graph_base_to_packed(
                    &mut recovery,
                    &mut fs,
                    &source,
                    &transaction,
                    limits(2),
                )
                .unwrap();
                let maintenance = recovery
                    .packed_indexes_with_io(&mut fs, &transaction, limits(2).certificates)
                    .unwrap();
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
                    prepare_packed_graph_transaction(
                        &maintenance,
                        &mut fs,
                        &base,
                        complex_request(),
                        preparation_limits()
                    )
                    .is_err()
                );
                assert_eq!(fs.pending_faults(), 0);
                assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
                drop(maintenance);
                drop(recovery);
                fs.restart().unwrap();
                let (mut recovery, source, transaction) = open(&mut fs, &name, 970_000);
                let (base, _) = bridge_graph_base_to_packed(
                    &mut recovery,
                    &mut fs,
                    &source,
                    &transaction,
                    limits(2),
                )
                .unwrap();
                let maintenance = recovery
                    .packed_indexes_with_io(&mut fs, &transaction, limits(2).certificates)
                    .unwrap();
                assert_eq!(
                    prepare_packed_graph_transaction(
                        &maintenance,
                        &mut fs,
                        &base,
                        complex_request(),
                        preparation_limits()
                    )
                    .unwrap()
                    .0
                    .result_digest(),
                    expected
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 270);
}

#[test]
fn packed_graph_preparation_rechecks_current_history_reverse_and_owner_before_use() {
    use uste_storage::FileSystem;
    let (memory, name, _) = fixture();
    let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
    let (mut recovery, source, transaction) = open(&mut fs, &name, 930_000);
    let (base, _) =
        bridge_graph_base_to_packed(&mut recovery, &mut fs, &source, &transaction, limits(2))
            .unwrap();
    let (foreign_memory, foreign_name, _) = fixture();
    let mut foreign_fs = FaultFileSystem::new(foreign_memory, FaultPlan::default());
    let (mut foreign, _, foreign_transaction) = open(&mut foreign_fs, &foreign_name, 970_000);
    let maintenance = foreign
        .packed_indexes_with_io(
            &mut foreign_fs,
            &foreign_transaction,
            limits(2).certificates,
        )
        .unwrap();
    foreign_fs.arm(FaultPlan::default()).unwrap();
    assert!(
        prepare_packed_graph_transaction(
            &maintenance,
            &mut foreign_fs,
            &base,
            complex_request(),
            preparation_limits()
        )
        .is_err()
    );
    assert_eq!(foreign_fs.operation_count(FsOp::ReadAt), 0);
    let maintenance = recovery
        .packed_indexes_with_io(&mut fs, &transaction, limits(2).certificates)
        .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    let foreign_scope = NamespaceRef::new(scope().database(), NamespaceId::from_bytes([9; 16]));
    let wrong = GraphTransaction::new(
        foreign_scope,
        vec![Operation::ReplaceEntity {
            target: record(1),
            expected: Expected::Version(uste_graph::RecordVersion::FIRST),
            properties: Value::Null,
        }],
    );
    assert!(
        prepare_packed_graph_transaction(&maintenance, &mut fs, &base, wrong, preparation_limits())
            .is_err()
    );
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
    for family in [2_u8, 3, 7] {
        let physical = base.families()[usize::from(family - 1)]
            .root
            .unwrap()
            .resolve(
                scope(),
                GRAPH_PACKED_PROFILE_V1,
                family,
                transaction.revision(),
            )
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
        let directory = fs
            .open_directory(&fs.root(), &EntryName::new("packed-graph-bridge").unwrap())
            .unwrap();
        let file = fs.open_existing(&directory, &name).unwrap();
        let offset = physical.page * 20545 + 137;
        let mut byte = [0];
        assert_eq!(fs.read_at(&file, offset, &mut byte).unwrap(), 1);
        byte[0] ^= 1;
        assert_eq!(fs.write_at(&file, offset, &byte).unwrap(), 1);
        fs.arm(FaultPlan::default()).unwrap();
        assert!(
            prepare_packed_graph_transaction(
                &maintenance,
                &mut fs,
                &base,
                complex_request(),
                preparation_limits()
            )
            .is_err()
        );
        assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
        byte[0] ^= 1;
        assert_eq!(fs.write_at(&file, offset, &byte).unwrap(), 1);
        assert!(
            prepare_packed_graph_transaction(
                &maintenance,
                &mut fs,
                &base,
                complex_request(),
                preparation_limits()
            )
            .is_ok()
        );
    }
}

#[test]
fn packed_graph_preparation_matches_full_reducer_success_and_rejections() {
    let (memory, name, _) = fixture();
    let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
    let (mut recovery, source, transaction) = open(&mut fs, &name, 930_000);
    let (base, _) =
        bridge_graph_base_to_packed(&mut recovery, &mut fs, &source, &transaction, limits(2))
            .unwrap();
    let (mut oracle_fs, oracle_name, _) = fixture();
    let (oracle, _) = CommitCoordinator::open(
        &mut oracle_fs,
        &oracle_name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(970_000),
        CounterEntropy(980_000),
        &mut TestKeyAdapter,
        GraphState::new(scope()),
    )
    .unwrap();
    let state = oracle.reducer_state_for_checkpoint().unwrap();
    for (index, request) in cases().into_iter().enumerate() {
        let expected = state.prepare_transaction(&request, CommitRevision::new(4).unwrap());
        let maintenance = recovery
            .packed_indexes_with_io(&mut fs, &transaction, limits(2).certificates)
            .unwrap();
        let actual = prepare_packed_graph_transaction(
            &maintenance,
            &mut fs,
            &base,
            request,
            preparation_limits(),
        );
        match (actual, expected) {
            (Ok((actual, logical, work)), Ok(expected)) => {
                assert_eq!(
                    actual.result_digest(),
                    GraphState::result_digest(&expected),
                    "case {index}"
                );
                assert_eq!(actual.change_count(), expected.change_count());
                assert_eq!(actual.revision().get(), 4);
                assert_eq!(actual.base_anchor(), base.anchor());
                assert_eq!(
                    actual.base_commitment(),
                    &base.publication_claims().state_digest
                );
                assert_eq!(actual.base_policy(), base.namespace_policy());
                assert!(logical.proof_logical_bytes > 0);
                assert!(work.point_lookups <= preparation_limits().maximum_point_lookups);
            }
            (Err(GraphDiskError::Graph(actual)), Err(expected)) => {
                assert_eq!(actual, expected, "case {index}")
            }
            (actual, expected) => panic!(
                "case {index}: packed error {:?}, oracle {:?}",
                actual.err(),
                expected.err()
            ),
        }
    }
}
