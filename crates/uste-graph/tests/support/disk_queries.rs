use super::*;
use uste_graph::{
    GraphDiskExpansionLimits, GraphDiskReadLimits, GraphReadOutput, GraphReadRequest,
};
use uste_policy::{Action, NamespaceGrant, PermissionSet, PolicyKernel, PrincipalDigest};
use uste_storage::fault::{
    FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation as FaultOperation,
};
use uste_txn::{AuthorizedDiskReader, AuthorizedReadState};

struct Identity;
impl uste_policy::TrustedPrincipalAdapter for Identity {
    type Credential = u8;
    fn authenticate(
        &mut self,
        value: &u8,
    ) -> Result<PrincipalDigest, uste_policy::AuthenticationError> {
        Ok(PrincipalDigest::from_bytes([*value; 32]))
    }
}

fn limits(expansion: GraphDiskExpansionLimits) -> GraphDiskReadLimits {
    GraphDiskReadLimits {
        current: uste_storage::IndexGetLimits::new(64, 4096).unwrap(),
        historical: IndexPredecessorLimits::new(64, 4096).unwrap(),
        expansion: Some(expansion),
    }
}

#[test]
fn authorized_disk_expansion_matches_reference_and_shares_all_work_budgets() {
    let mut filesystem = FaultFileSystem::new(
        MemoryFileSystem::new(16 * 1024 * 1024),
        FaultPlan::default(),
    );
    let name = EntryName::new("authorized-disk-expansion").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 600_000),
        CounterEntropy(610_000),
        GraphState::new(scope()),
    )
    .unwrap();
    let quotas = QuotaLimits::new(100, 1024 * 1024, 1024 * 1024, 8, 1024).unwrap();
    let mut policy = NamespacePolicy::new(scope(), PolicyVersion::new(1).unwrap(), quotas);
    for who in [1, 2, 3] {
        let actions = if who == 3 {
            vec![Action::ReadRecord]
        } else {
            vec![Action::ReadRecord, Action::ReadHistory, Action::ExpandGraph]
        };
        let mut grant = NamespaceGrant::new(PermissionSet::from_actions(actions), quotas);
        if who == 2 {
            for hidden in [record(3), record(15)] {
                grant
                    .deny_record(
                        hidden.record(),
                        PermissionSet::from_actions([Action::ReadRecord, Action::ExpandGraph]),
                    )
                    .unwrap();
            }
        }
        policy
            .grant(PrincipalDigest::from_bytes([who; 32]), grant)
            .unwrap();
    }
    let mut operations = Vec::new();
    for id in [1, 2, 3] {
        operations.push(Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Entity(NewEntity {
                id: record(id),
                entity_type: text("node"),
                schema_version: 1,
                properties: if id == 2 {
                    Value::RecordRef(record(3))
                } else {
                    Value::Null
                },
            }),
        });
    }
    operations.push(Operation::Create {
        expected: Expected::Absent,
        record: NewRecord::Evidence(NewEvidence {
            id: record(4),
            digest: [4; 32],
            locator: text("synthetic:row:4"),
        }),
    });
    for (id, from, to) in [(11, 1, 2), (12, 1, 3), (13, 1, 1), (14, 2, 1), (15, 1, 2)] {
        operations.push(Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Relationship(NewRelationship {
                id: record(id),
                from: record(from),
                to: record(to),
                relationship_type: text("connects"),
                properties: Value::Null,
                evidence: vec![record(4)],
                valid_time: ValidTime::Unknown,
            }),
        });
    }
    operations.push(Operation::Create {
        expected: Expected::Absent,
        record: NewRecord::Assertion(NewAssertion {
            id: record(20),
            subject: record(1),
            predicate: text("hidden-reference"),
            object: Value::RecordRef(record(3)),
            evidence: vec![record(4)],
            valid_time: ValidTime::Unknown,
        }),
    });
    commit(
        &mut coordinator,
        &mut filesystem,
        1,
        GraphTransaction::with_policy_mutation(
            scope(),
            operations,
            DurablePolicyMutation::Install {
                policy: policy.clone(),
            },
        ),
    );
    commit(
        &mut coordinator,
        &mut filesystem,
        2,
        GraphTransaction::new(
            scope(),
            (11..=15)
                .map(|id| Operation::ActOnRelationship {
                    target: record(id),
                    expected: Expected::Version(uste_graph::RecordVersion::FIRST),
                    action: AssertionAction::Accept,
                    correction: None,
                    correction_expected: None,
                })
                .collect(),
        ),
    );
    let snapshot = coordinator.read_view().unwrap().state().clone();
    publish_graph_state_root(&mut coordinator, &mut filesystem, &snapshot).unwrap();
    publish_coordinator_metadata_root(&mut coordinator, &mut filesystem).unwrap();
    uste_txn::publish_coordinator_transaction_index(&mut coordinator, &mut filesystem).unwrap();
    drop(coordinator);
    filesystem.restart().unwrap();
    let (recovery, _) = AuthenticatedIndexRecovery::open(
        &mut filesystem,
        &name,
        scope(),
        CounterEntropy(620_000),
        CounterEntropy(630_000),
        &mut TestKeyAdapter,
    )
    .unwrap();
    let mut cache = PageCache::new(64 * 1024).unwrap();
    let lookup = uste_storage::IndexGetLimits::new(64, 136).unwrap();
    let transaction_root = recovery
        .load_index_root_manifests(
            &mut filesystem,
            uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
        )
        .unwrap()
        .remove(0);
    let transactions = uste_txn::admit_coordinator_transaction_index_for_recovery(
        &recovery,
        &mut filesystem,
        transaction_root,
        uste_txn::CoordinatorTransactionAdmissionLimits {
            run: IndexRunReadLimits::new(16, 2, 4096).unwrap(),
            lookup,
            maximum_groups: 2,
            maximum_encoded_bytes: 1_000_000,
        },
        &mut cache,
    )
    .unwrap();
    let metadata_candidate =
        load_coordinator_metadata_candidates_for_recovery::<GraphState, _, _, _, _>(
            &recovery,
            &mut filesystem,
        )
        .unwrap()
        .remove(0);
    let metadata = uste_txn::admit_coordinator_disk_base(
        &recovery,
        &mut filesystem,
        metadata_candidate,
        transactions,
        uste_txn::CoordinatorDiskAdmissionLimits {
            metadata: CoordinatorMetadataLoadLimits::new(2, 0, 3, 16, 4096).unwrap(),
            lookup,
            maximum_total_journal_groups: 2,
            maximum_encoded_bytes_per_pass: 1_000_000,
        },
        &mut cache,
    )
    .unwrap();
    let candidate = load_graph_state_root_candidates_for_recovery(&recovery, &mut filesystem)
        .unwrap()
        .remove(0);
    let (base, _) = admit_graph_disk_base_candidate_for_recovery(
        &recovery,
        &mut filesystem,
        &candidate,
        admission_limits(),
        &mut cache,
    )
    .unwrap();
    let disk = uste_txn::DiskCommitCoordinator::recover_from_admitted_base(
        recovery,
        &mut filesystem,
        metadata,
        GraphDiskLiveState::new(base),
        RetentionDays::new(30).unwrap(),
        uste_txn::DiskCoordinatorRecoveryLimits {
            overlay: uste_txn::CoordinatorRecoveryLimits::new(0, 0).unwrap(),
            lookup,
            maximum_encoded_bytes: 0,
        },
        &mut cache,
    )
    .unwrap();
    let mut kernel = PolicyKernel::new();
    kernel.install_initial_policy(policy).unwrap();
    let admin = kernel.authenticate(&mut Identity, &1).unwrap();
    let bob = kernel.authenticate(&mut Identity, &2).unwrap();
    let no_history = kernel.authenticate(&mut Identity, &3).unwrap();
    let ample = GraphDiskExpansionLimits::new(1000, 100, 1024 * 1024, 100).unwrap();
    let reader = AuthorizedDiskReader::new(&disk, &kernel, limits(ample)).unwrap();
    let requests = [
        GraphReadRequest::Adjacent {
            entity: record(1),
            direction: AdjacencyDirection::Outgoing,
            maximum: 10,
        },
        GraphReadRequest::Adjacent {
            entity: record(1),
            direction: AdjacencyDirection::Incoming,
            maximum: 10,
        },
        GraphReadRequest::Adjacent {
            entity: record(1),
            direction: AdjacencyDirection::Either,
            maximum: 10,
        },
        GraphReadRequest::SupportedBy {
            evidence: record(4),
            maximum: 10,
        },
        GraphReadRequest::Record { id: record(2) },
        GraphReadRequest::RecordAt {
            id: record(2),
            revision: CommitRevision::FIRST,
        },
        GraphReadRequest::Adjacent {
            entity: record(99),
            direction: AdjacencyDirection::Either,
            maximum: 0,
        },
    ];
    for _warm in [false, true] {
        for principal in [&admin, &bob] {
            for request in &requests {
                let expected = <GraphState as AuthorizedReadState>::read_authorized(
                    &snapshot,
                    request,
                    &mut |action, target| kernel.authorize(principal, action, target).is_ok(),
                )
                .unwrap();
                assert_eq!(
                    reader
                        .read(&mut filesystem, principal, request, &NeverCancel)
                        .unwrap(),
                    expected
                );
            }
        }
    }
    assert!(
        reader
            .read(
                &mut filesystem,
                &no_history,
                &GraphReadRequest::Record { id: record(1) },
                &NeverCancel
            )
            .is_ok()
    );
    assert!(matches!(
        reader.read(&mut filesystem, &no_history, &requests[5], &NeverCancel),
        Err(uste_txn::AuthorizedReadError::Authorization(
            uste_txn::AuthorizedError::Unauthorized
        ))
    ));
    assert_eq!(
        reader
            .read(&mut filesystem, &bob, &requests[4], &NeverCancel)
            .unwrap(),
        GraphReadOutput::Record(None)
    );
    let all = &requests[2];
    let expected = reader
        .read(&mut filesystem, &admin, all, &NeverCancel)
        .unwrap();
    let GraphReadOutput::Adjacent(neighbors) = &expected else {
        panic!("expected adjacency")
    };
    let exact_bytes = 6 * 48
        + neighbors
            .iter()
            .map(|neighbor| {
                encode_stored_record(&Record::Relationship(neighbor.relationship.clone()))
                    .unwrap()
                    .len()
                    + encode_stored_record(&Record::Entity(neighbor.entity.clone()))
                        .unwrap()
                        .len()
            })
            .sum::<usize>();
    for budget in [
        GraphDiskExpansionLimits::new(23, 100, 1024 * 1024, 100).unwrap(),
        GraphDiskExpansionLimits::new(1000, 5, 1024 * 1024, 100).unwrap(),
        GraphDiskExpansionLimits::new(1000, 100, exact_bytes - 1, 100).unwrap(),
        GraphDiskExpansionLimits::new(1000, 100, 1024 * 1024, 9).unwrap(),
    ] {
        let bounded = AuthorizedDiskReader::new(&disk, &kernel, limits(budget)).unwrap();
        for _ in 0..2 {
            assert!(matches!(
                bounded.read(&mut filesystem, &admin, all, &NeverCancel),
                Err(uste_txn::AuthorizedReadError::Domain(
                    GraphDiskError::Storage(uste_storage::journal::StorageError::ResourceLimit)
                        | GraphDiskError::Transaction(uste_txn::TransactionError::Storage(
                            uste_storage::journal::StorageError::ResourceLimit
                        ))
                ))
            ));
        }
    }
    let exact = AuthorizedDiskReader::new(
        &disk,
        &kernel,
        limits(GraphDiskExpansionLimits::new(24, 6, exact_bytes, 10).unwrap()),
    )
    .unwrap();
    for _ in 0..2 {
        assert_eq!(
            exact
                .read(&mut filesystem, &admin, all, &NeverCancel)
                .unwrap(),
            reader
                .read(&mut filesystem, &admin, all, &NeverCancel)
                .unwrap()
        );
    }
    let zero = GraphReadRequest::Adjacent {
        entity: record(1),
        direction: AdjacencyDirection::Either,
        maximum: 0,
    };
    assert!(matches!(
        reader.read(&mut filesystem, &admin, &zero, &NeverCancel),
        Err(uste_txn::AuthorizedReadError::Domain(
            GraphDiskError::Graph(GraphError::ResultLimit { .. })
        ))
    ));
    struct CancelAfterDispatch(std::cell::Cell<usize>);
    impl uste_txn::Cancellation for CancelAfterDispatch {
        fn is_cancelled(&self) -> bool {
            self.0.set(self.0.get() + 1);
            self.0.get() >= 2
        }
    }
    assert!(matches!(
        reader.read(
            &mut filesystem,
            &admin,
            all,
            &CancelAfterDispatch(std::cell::Cell::new(0))
        ),
        Err(uste_txn::AuthorizedReadError::Authorization(
            uste_txn::AuthorizedError::Transaction(uste_txn::TransactionError::Cancelled)
        ))
    ));
    filesystem.arm(FaultPlan::default()).unwrap();
    AuthorizedDiskReader::new(&disk, &kernel, limits(ample))
        .unwrap()
        .read(&mut filesystem, &admin, all, &NeverCancel)
        .unwrap();
    let reads = filesystem.operation_count(FaultOperation::ReadAt);
    assert!(reads > 0);
    for occurrence in 1..=reads {
        let fresh = AuthorizedDiskReader::new(&disk, &kernel, limits(ample)).unwrap();
        filesystem
            .arm(
                FaultPlan::new([FaultPoint {
                    operation: FaultOperation::ReadAt,
                    occurrence,
                    action: FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                }])
                .unwrap(),
            )
            .unwrap();
        assert!(
            fresh
                .read(&mut filesystem, &admin, all, &NeverCancel)
                .is_err()
        );
        assert_eq!(filesystem.pending_faults(), 0);
        filesystem.arm(FaultPlan::default()).unwrap();
        assert_eq!(
            fresh
                .read(&mut filesystem, &admin, all, &NeverCancel)
                .unwrap(),
            expected
        );
    }
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
    let fresh = AuthorizedDiskReader::new(&disk, &kernel, limits(ample)).unwrap();
    assert!(matches!(
        fresh.read(&mut filesystem, &no_history, all, &NeverCancel),
        Err(uste_txn::AuthorizedReadError::Authorization(
            uste_txn::AuthorizedError::Unauthorized
        ))
    ));
    assert_eq!(filesystem.operation_count(FaultOperation::ReadAt), 0);
    assert!(
        fresh
            .read(&mut filesystem, &admin, all, &NeverCancel)
            .is_err()
    );
    assert_eq!(filesystem.pending_faults(), 0);
}
