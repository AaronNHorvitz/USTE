use uste_graph::{
    AdjacencyDirection, AssertionAction, AssertionStatus, DeletePolicy, DurablePolicyMutation,
    Expected, GraphError, GraphState, GraphTransaction, MAX_TRANSACTION_OPERATIONS,
    MAX_TRANSACTION_REFERENCES, NewEntity, NewEvidence, NewRecord, NewRelationship, Operation,
    Record, RecordVersion, ValidTime, decode_transaction, encode_transaction,
};
use uste_policy::{
    Action, NamespaceGrant, NamespacePolicy, PermissionSet, PolicyVersion, PrincipalDigest,
    QuotaLimits, Target,
};
use uste_txn::{ApplyError, AuthorizedTransactionState, TransactionState};
use uste_types::{
    BoundedString, CanonicalMap, CommitRevision, DatabaseId, NamespaceId, NamespaceRef, RecordId,
    RecordRef, Value, decode_value, encode_value,
};

fn scope(value: u8) -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([1; 16]),
        NamespaceId::from_bytes([value; 16]),
    )
}

fn record(scope: NamespaceRef, value: u8) -> RecordRef {
    RecordRef::new(
        scope.database(),
        scope.namespace(),
        RecordId::from_bytes([value; 16]),
    )
}

fn text(value: &str) -> BoundedString {
    BoundedString::new(value.to_owned()).unwrap()
}

fn create_entity(id: RecordRef) -> Operation {
    Operation::Create {
        expected: Expected::Absent,
        record: NewRecord::Entity(NewEntity {
            id,
            entity_type: text("thing"),
            schema_version: 1,
            properties: Value::Null,
        }),
    }
}

fn apply(state: &mut GraphState, transaction: &GraphTransaction, revision: u64) {
    let prepared = state
        .prepare_transaction(transaction, CommitRevision::new(revision).unwrap())
        .unwrap();
    TransactionState::publish(state, prepared);
}

fn limits(bytes: u64) -> QuotaLimits {
    QuotaLimits::new(1024, bytes, bytes, 2, 1024).unwrap()
}

fn policy(scope: NamespaceRef, version: u64, actions: &[Action]) -> NamespacePolicy {
    let mut policy =
        NamespacePolicy::new(scope, PolicyVersion::new(version).unwrap(), limits(10_000));
    policy
        .grant(
            PrincipalDigest::from_bytes([3; 32]),
            NamespaceGrant::new(
                PermissionSet::from_actions(actions.iter().copied()),
                limits(10_000),
            ),
        )
        .unwrap();
    policy
}

#[test]
fn canonical_codec_round_trips_and_rejects_every_truncation_and_unknown_action() {
    let scope = scope(2);
    let relationship = record(scope, 4);
    let transaction = GraphTransaction::new(
        scope,
        vec![Operation::ActOnRelationship {
            target: relationship,
            expected: Expected::Version(RecordVersion::FIRST),
            action: AssertionAction::Accept,
            correction: None,
            correction_expected: None,
        }],
    );
    let encoded = encode_transaction(&transaction).unwrap();
    assert_eq!(decode_transaction(&encoded).unwrap(), transaction);
    assert_eq!(
        encode_transaction(&decode_transaction(&encoded).unwrap()).unwrap(),
        encoded
    );
    for cut in 0..encoded.len() {
        assert!(
            decode_transaction(&encoded[..cut]).is_err(),
            "accepted cut {cut}"
        );
    }
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert!(decode_transaction(&trailing).is_err());

    let offset = encoded
        .windows(b"accept".len())
        .position(|window| window == b"accept")
        .unwrap();
    let mut unknown = encoded;
    unknown[offset..offset + 6].copy_from_slice(b"purgee");
    assert!(decode_transaction(&unknown).is_err());
}

#[test]
fn canonical_codec_rejects_extra_and_missing_root_fields() {
    let scope = scope(20);
    let encoded = encode_transaction(&GraphTransaction::new(
        scope,
        vec![create_entity(record(scope, 1))],
    ))
    .unwrap();
    let Value::Map(root) = decode_value(&encoded).unwrap() else {
        panic!("map")
    };

    let mut extra = root.clone().into_vec();
    extra.push((text("unexpected"), Value::Null));
    let extra = encode_value(&Value::Map(CanonicalMap::new(extra).unwrap())).unwrap();
    assert!(decode_transaction(&extra).is_err());

    let missing = root
        .into_vec()
        .into_iter()
        .filter(|(key, _)| key.as_str() != "scope")
        .collect();
    let missing = encode_value(&Value::Map(CanonicalMap::new(missing).unwrap())).unwrap();
    assert!(decode_transaction(&missing).is_err());
}

#[test]
fn operation_and_reference_caps_reject_the_first_overage_before_state_access() {
    let scope = scope(21);
    let target = record(scope, 1);
    let exact = GraphTransaction::new(
        scope,
        vec![create_entity(target); MAX_TRANSACTION_OPERATIONS],
    );
    let exact = encode_transaction(&exact).unwrap();
    <GraphState as AuthorizedTransactionState>::authorization_requirements(&exact, None).unwrap();

    let over = GraphTransaction::new(
        scope,
        vec![create_entity(target); MAX_TRANSACTION_OPERATIONS + 1],
    );
    let over = encode_transaction(&over).unwrap();
    assert_eq!(
        <GraphState as AuthorizedTransactionState>::authorization_requirements(&over, None),
        Err(ApplyError::ResourceLimit)
    );

    let references = |count: usize| {
        Value::list(
            (0..count)
                .map(|index| {
                    let mut bytes = [0_u8; 16];
                    bytes[..8].copy_from_slice(&(index as u64).to_be_bytes());
                    Value::RecordRef(RecordRef::new(
                        scope.database(),
                        scope.namespace(),
                        RecordId::from_bytes(bytes),
                    ))
                })
                .collect(),
        )
        .unwrap()
    };
    let reference_over = GraphTransaction::new(
        scope,
        vec![
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: record(scope, 2),
                    entity_type: text("many-refs"),
                    schema_version: 1,
                    properties: references((MAX_TRANSACTION_REFERENCES - 2) / 2),
                }),
            },
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: record(scope, 3),
                    entity_type: text("many-refs"),
                    schema_version: 1,
                    properties: references(MAX_TRANSACTION_REFERENCES / 2),
                }),
            },
        ],
    );
    let encoded = encode_transaction(&reference_over).unwrap();
    assert_eq!(
        <GraphState as AuthorizedTransactionState>::authorization_requirements(&encoded, None),
        Err(ApplyError::ResourceLimit)
    );
}

#[test]
fn evidence_relationship_correction_history_and_both_adjacency_directions_are_atomic() {
    let scope = scope(3);
    let left = record(scope, 1);
    let right = record(scope, 2);
    let evidence = record(scope, 3);
    let relationship = record(scope, 4);
    let correction = record(scope, 5);
    let mut state = GraphState::new(scope);

    let create = GraphTransaction::new(
        scope,
        vec![
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Relationship(NewRelationship {
                    id: relationship,
                    from: left,
                    to: right,
                    relationship_type: text("links"),
                    properties: Value::Null,
                    evidence: vec![evidence],
                    valid_time: ValidTime::Unknown,
                }),
            },
            create_entity(right),
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Evidence(NewEvidence {
                    id: evidence,
                    digest: [9; 32],
                    locator: text("source:1"),
                }),
            },
            create_entity(left),
        ],
    );
    apply(&mut state, &create, 1);
    assert!(
        state
            .snapshot()
            .adjacent(left, AdjacencyDirection::Outgoing, 10)
            .unwrap()
            .is_empty()
    );

    let accept = GraphTransaction::new(
        scope,
        vec![Operation::ActOnRelationship {
            target: relationship,
            expected: Expected::Version(RecordVersion::FIRST),
            action: AssertionAction::Accept,
            correction: None,
            correction_expected: None,
        }],
    );
    apply(&mut state, &accept, 2);
    let snapshot = state.snapshot();
    assert_eq!(
        snapshot
            .adjacent(left, AdjacencyDirection::Outgoing, 10)
            .unwrap()[0]
            .id,
        relationship
    );
    assert_eq!(
        snapshot
            .adjacent(right, AdjacencyDirection::Incoming, 10)
            .unwrap()[0]
            .id,
        relationship
    );
    assert_eq!(snapshot.supported_by(evidence, 10).unwrap().len(), 1);
    snapshot.validate_derived_indexes().unwrap();

    let correct = GraphTransaction::new(
        scope,
        vec![Operation::ActOnRelationship {
            target: relationship,
            expected: Expected::Version(RecordVersion::new(2).unwrap()),
            action: AssertionAction::Correct,
            correction: Some(NewRelationship {
                id: correction,
                from: right,
                to: left,
                relationship_type: text("links"),
                properties: Value::Unsigned(2),
                evidence: vec![evidence],
                valid_time: ValidTime::Unknown,
            }),
            correction_expected: Some(Expected::Absent),
        }],
    );
    apply(&mut state, &correct, 3);
    let snapshot = state.snapshot();
    let Record::Relationship(corrected) = snapshot.record(correction).unwrap() else {
        panic!("relationship")
    };
    assert_eq!(corrected.status, AssertionStatus::Proposed);
    assert_eq!(corrected.correction_of, Some(relationship));
    let Record::Relationship(at_two) = snapshot
        .record_at(CommitRevision::new(2).unwrap(), relationship)
        .unwrap()
        .unwrap()
    else {
        panic!("relationship")
    };
    assert_eq!(at_two.status, AssertionStatus::Accepted);
}

#[test]
fn exact_declared_cascade_retracts_edge_and_delete_conflicts_are_atomic() {
    let scope = scope(4);
    let left = record(scope, 1);
    let right = record(scope, 2);
    let evidence = record(scope, 3);
    let relationship = record(scope, 4);
    let mut state = GraphState::new(scope);
    let create = GraphTransaction::new(
        scope,
        vec![
            create_entity(left),
            create_entity(right),
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Evidence(NewEvidence {
                    id: evidence,
                    digest: [7; 32],
                    locator: text("source:2"),
                }),
            },
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Relationship(NewRelationship {
                    id: relationship,
                    from: left,
                    to: right,
                    relationship_type: text("owns"),
                    properties: Value::Null,
                    evidence: vec![evidence],
                    valid_time: ValidTime::Unknown,
                }),
            },
        ],
    );
    apply(&mut state, &create, 1);
    apply(
        &mut state,
        &GraphTransaction::new(
            scope,
            vec![Operation::ActOnRelationship {
                target: relationship,
                expected: Expected::Version(RecordVersion::FIRST),
                action: AssertionAction::Accept,
                correction: None,
                correction_expected: None,
            }],
        ),
        2,
    );
    let before = state.snapshot();
    let stale = GraphTransaction::new(
        scope,
        vec![Operation::DeleteEntity {
            target: right,
            expected: Expected::Version(RecordVersion::FIRST),
            policy: DeletePolicy::CascadeAndRetract {
                maximum_affected: 1,
            },
            affected: Vec::new(),
        }],
    );
    assert_eq!(
        state.prepare_transaction(&stale, CommitRevision::new(3).unwrap()),
        Err(GraphError::CascadeDeclarationChanged)
    );
    assert_eq!(state.snapshot(), before);

    let delete = GraphTransaction::new(
        scope,
        vec![Operation::DeleteEntity {
            target: right,
            expected: Expected::Version(RecordVersion::FIRST),
            policy: DeletePolicy::CascadeAndRetract {
                maximum_affected: 1,
            },
            affected: vec![relationship],
        }],
    );
    apply(&mut state, &delete, 3);
    let snapshot = state.snapshot();
    assert!(
        snapshot
            .adjacent(left, AdjacencyDirection::Outgoing, 10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        snapshot
            .supported_by(evidence, 10)
            .unwrap()
            .into_iter()
            .map(Record::id)
            .collect::<Vec<_>>(),
        vec![relationship]
    );
    let Record::Relationship(relationship) = snapshot.record(relationship).unwrap() else {
        panic!("relationship")
    };
    assert_eq!(relationship.status, AssertionStatus::Retracted);
    snapshot.validate_derived_indexes().unwrap();
}

#[test]
fn delete_uses_transaction_overlay_reverse_dependencies() {
    let scope = scope(22);
    let target = record(scope, 1);
    let owner = record(scope, 2);
    let mut removed = GraphState::new(scope);
    apply(
        &mut removed,
        &GraphTransaction::new(
            scope,
            vec![
                create_entity(target),
                Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Entity(NewEntity {
                        id: owner,
                        entity_type: text("owner"),
                        schema_version: 1,
                        properties: Value::RecordRef(target),
                    }),
                },
            ],
        ),
        1,
    );
    apply(
        &mut removed,
        &GraphTransaction::new(
            scope,
            vec![
                Operation::ReplaceEntity {
                    target: owner,
                    expected: Expected::Version(RecordVersion::FIRST),
                    properties: Value::Null,
                },
                Operation::DeleteEntity {
                    target,
                    expected: Expected::Version(RecordVersion::FIRST),
                    policy: DeletePolicy::Reject,
                    affected: Vec::new(),
                },
            ],
        ),
        2,
    );
    let snapshot = removed.snapshot();
    let Record::Entity(target_record) = snapshot.record(target).unwrap() else {
        panic!("entity")
    };
    assert_eq!(
        target_record.lifecycle,
        uste_graph::EntityLifecycle::Deleted
    );
    snapshot.validate_derived_indexes().unwrap();

    let mut added = GraphState::new(scope);
    apply(
        &mut added,
        &GraphTransaction::new(scope, vec![create_entity(target), create_entity(owner)]),
        1,
    );
    let before = added.snapshot();
    let add_then_delete = GraphTransaction::new(
        scope,
        vec![
            Operation::ReplaceEntity {
                target: owner,
                expected: Expected::Version(RecordVersion::FIRST),
                properties: Value::RecordRef(target),
            },
            Operation::DeleteEntity {
                target,
                expected: Expected::Version(RecordVersion::FIRST),
                policy: DeletePolicy::Reject,
                affected: Vec::new(),
            },
        ],
    );
    assert_eq!(
        added.prepare_transaction(&add_then_delete, CommitRevision::new(2).unwrap()),
        Err(GraphError::DeleteRestricted {
            record: target,
            dependents: 1,
        })
    );
    assert_eq!(added.snapshot(), before);
}

#[test]
fn delete_observes_same_transaction_claim_status_changes() {
    let scope = scope(23);
    let left = record(scope, 1);
    let right = record(scope, 2);
    let evidence = record(scope, 3);
    let relationship = record(scope, 4);
    let mut state = GraphState::new(scope);
    apply(
        &mut state,
        &GraphTransaction::new(
            scope,
            vec![
                create_entity(left),
                create_entity(right),
                Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Evidence(NewEvidence {
                        id: evidence,
                        digest: [0x23; 32],
                        locator: text("source:overlay-status"),
                    }),
                },
                Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Relationship(NewRelationship {
                        id: relationship,
                        from: left,
                        to: right,
                        relationship_type: text("depends"),
                        properties: Value::Null,
                        evidence: vec![evidence],
                        valid_time: ValidTime::Unknown,
                    }),
                },
            ],
        ),
        1,
    );
    let accept = Operation::ActOnRelationship {
        target: relationship,
        expected: Expected::Version(RecordVersion::FIRST),
        action: AssertionAction::Accept,
        correction: None,
        correction_expected: None,
    };
    let delete = |affected| Operation::DeleteEntity {
        target: right,
        expected: Expected::Version(RecordVersion::FIRST),
        policy: DeletePolicy::CascadeAndRetract {
            maximum_affected: 1,
        },
        affected,
    };
    assert_eq!(
        state.prepare_transaction(
            &GraphTransaction::new(scope, vec![accept.clone(), delete(Vec::new())]),
            CommitRevision::new(2).unwrap(),
        ),
        Err(GraphError::CascadeDeclarationChanged)
    );
    assert_eq!(
        state.prepare_transaction(
            &GraphTransaction::new(scope, vec![accept.clone(), delete(vec![relationship])]),
            CommitRevision::new(2).unwrap(),
        ),
        Err(GraphError::DuplicateMutation(relationship))
    );
    apply(&mut state, &GraphTransaction::new(scope, vec![accept]), 2);
    apply(
        &mut state,
        &GraphTransaction::new(
            scope,
            vec![
                Operation::ActOnRelationship {
                    target: relationship,
                    expected: Expected::Version(RecordVersion::new(2).unwrap()),
                    action: AssertionAction::Retract,
                    correction: None,
                    correction_expected: None,
                },
                Operation::DeleteEntity {
                    target: right,
                    expected: Expected::Version(RecordVersion::FIRST),
                    policy: DeletePolicy::Reject,
                    affected: Vec::new(),
                },
            ],
        ),
        3,
    );
    state.snapshot().validate_derived_indexes().unwrap();
}

#[test]
fn authorization_requirements_cover_targets_history_references_and_cascade_mutations() {
    let scope = scope(5);
    let target = record(scope, 1);
    let predicate = record(scope, 2);
    let dependent = record(scope, 3);
    let transaction = GraphTransaction::new(
        scope,
        vec![Operation::DeleteEntity {
            target,
            expected: Expected::ReadView {
                revision: CommitRevision::FIRST,
                predicate: uste_graph::Predicate::RecordVisible(predicate),
            },
            policy: DeletePolicy::CascadeAndRetract {
                maximum_affected: 1,
            },
            affected: vec![dependent],
        }],
    );
    let encoded = encode_transaction(&transaction).unwrap();
    let requirements =
        <GraphState as AuthorizedTransactionState>::authorization_requirements(&encoded, None)
            .unwrap();
    let actual: Vec<_> = requirements
        .iter()
        .map(|requirement| (requirement.action, requirement.target))
        .collect();
    for required in [
        (Action::Commit, Target::Record(target)),
        (Action::ReadRecord, Target::Record(target)),
        (Action::ReadRecord, Target::Record(predicate)),
        (Action::ReadHistory, Target::Record(predicate)),
        (Action::ReadRecord, Target::Record(dependent)),
        (Action::Commit, Target::Record(dependent)),
    ] {
        assert!(actual.contains(&required), "missing {required:?}");
    }
}

#[test]
fn durable_policy_install_replace_codec_and_history_are_exact() {
    let scope = scope(6);
    let initial = policy(scope, 1, &[Action::Commit, Action::ManagePolicy]);
    let install = GraphTransaction::with_policy_mutation(
        scope,
        Vec::new(),
        DurablePolicyMutation::Install {
            policy: initial.clone(),
        },
    );
    let encoded = encode_transaction(&install).unwrap();
    assert_eq!(decode_transaction(&encoded).unwrap(), install);
    let mut state = GraphState::new(scope);
    apply(
        &mut state,
        &GraphTransaction::new(scope, vec![create_entity(record(scope, 1))]),
        1,
    );
    apply(&mut state, &install, 2);
    assert_eq!(state.snapshot().namespace_policy(), Some(&initial));

    let next = policy(scope, 2, &[Action::ReadRecord]);
    let replace = GraphTransaction::with_policy_mutation(
        scope,
        Vec::new(),
        DurablePolicyMutation::Replace {
            expected: PolicyVersion::new(1).unwrap(),
            policy: next.clone(),
        },
    );
    apply(&mut state, &replace, 3);
    let snapshot = state.snapshot();
    assert_eq!(snapshot.namespace_policy(), Some(&next));
    assert_eq!(
        snapshot.namespace_policy_at(CommitRevision::FIRST).unwrap(),
        None
    );
    assert_eq!(
        snapshot
            .namespace_policy_at(CommitRevision::new(2).unwrap())
            .unwrap(),
        Some(&initial)
    );
    assert_eq!(
        snapshot
            .namespace_policy_at(CommitRevision::new(3).unwrap())
            .unwrap(),
        Some(&next)
    );
    assert_eq!(
        snapshot.namespace_policy_at(CommitRevision::new(4).unwrap()),
        Err(GraphError::UnknownReadView(CommitRevision::new(4).unwrap()))
    );
    let debug = format!("{next:?}");
    assert!(debug.contains("[REDACTED]"));
    assert!(!debug.contains("03030303"));
}

#[test]
fn result_digest_binds_changed_record_content() {
    let scope = scope(23);
    let left = record(scope, 1);
    let right = record(scope, 2);
    let evidence = record(scope, 3);
    let relationship = record(scope, 4);
    let mut state = GraphState::new(scope);
    apply(
        &mut state,
        &GraphTransaction::new(
            scope,
            vec![
                create_entity(left),
                create_entity(right),
                Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Evidence(NewEvidence {
                        id: evidence,
                        digest: [5; 32],
                        locator: text("digest-source"),
                    }),
                },
                Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Relationship(NewRelationship {
                        id: relationship,
                        from: left,
                        to: right,
                        relationship_type: text("digest-edge"),
                        properties: Value::Null,
                        evidence: vec![evidence],
                        valid_time: ValidTime::Unknown,
                    }),
                },
            ],
        ),
        1,
    );
    let prepare = |action| {
        let transaction = GraphTransaction::new(
            scope,
            vec![Operation::ActOnRelationship {
                target: relationship,
                expected: Expected::Version(RecordVersion::FIRST),
                action,
                correction: None,
                correction_expected: None,
            }],
        );
        let encoded = encode_transaction(&transaction).unwrap();
        TransactionState::prepare(&state, &encoded, None, CommitRevision::new(2).unwrap()).unwrap()
    };
    let accepted = prepare(AssertionAction::Accept);
    let rejected = prepare(AssertionAction::Reject);
    assert_ne!(
        GraphState::result_digest(&accepted),
        GraphState::result_digest(&rejected)
    );

    let empty = GraphState::new(scope);
    let policy_result = |actions: &[Action]| {
        let transaction = GraphTransaction::with_policy_mutation(
            scope,
            Vec::new(),
            DurablePolicyMutation::Install {
                policy: policy(scope, 1, actions),
            },
        );
        let encoded = encode_transaction(&transaction).unwrap();
        TransactionState::prepare(&empty, &encoded, None, CommitRevision::FIRST).unwrap()
    };
    assert_ne!(
        GraphState::result_digest(&policy_result(&[Action::Commit])),
        GraphState::result_digest(&policy_result(&[Action::ReadRecord]))
    );
}

#[test]
fn cycles_self_loops_parallel_edges_and_terminal_statuses_keep_indexes_symmetric() {
    let scope = scope(22);
    let entities = [record(scope, 1), record(scope, 2), record(scope, 3)];
    let evidence = record(scope, 4);
    let edges = [
        (record(scope, 10), entities[0], entities[0]),
        (record(scope, 11), entities[0], entities[1]),
        (record(scope, 12), entities[0], entities[1]),
        (record(scope, 13), entities[1], entities[2]),
        (record(scope, 14), entities[2], entities[0]),
    ];
    let mut operations: Vec<_> = entities.iter().copied().map(create_entity).collect();
    operations.push(Operation::Create {
        expected: Expected::Absent,
        record: NewRecord::Evidence(NewEvidence {
            id: evidence,
            digest: [4; 32],
            locator: text("cycle-source"),
        }),
    });
    operations.extend(edges.iter().map(|(id, from, to)| Operation::Create {
        expected: Expected::Absent,
        record: NewRecord::Relationship(NewRelationship {
            id: *id,
            from: *from,
            to: *to,
            relationship_type: text("path"),
            properties: Value::Null,
            evidence: vec![evidence],
            valid_time: ValidTime::Unknown,
        }),
    }));
    let mut state = GraphState::new(scope);
    apply(&mut state, &GraphTransaction::new(scope, operations), 1);
    let accepts = edges
        .iter()
        .map(|(id, _, _)| Operation::ActOnRelationship {
            target: *id,
            expected: Expected::Version(RecordVersion::FIRST),
            action: AssertionAction::Accept,
            correction: None,
            correction_expected: None,
        })
        .collect();
    apply(&mut state, &GraphTransaction::new(scope, accepts), 2);
    let snapshot = state.snapshot();
    let outgoing: Vec<_> = snapshot
        .adjacent(entities[0], AdjacencyDirection::Outgoing, 10)
        .unwrap()
        .into_iter()
        .map(|edge| edge.id)
        .collect();
    assert_eq!(outgoing, vec![edges[0].0, edges[1].0, edges[2].0]);
    let incoming: Vec<_> = snapshot
        .adjacent(entities[0], AdjacencyDirection::Incoming, 10)
        .unwrap()
        .into_iter()
        .map(|edge| edge.id)
        .collect();
    assert_eq!(incoming, vec![edges[0].0, edges[4].0]);

    apply(
        &mut state,
        &GraphTransaction::new(
            scope,
            vec![Operation::ActOnRelationship {
                target: edges[1].0,
                expected: Expected::Version(RecordVersion::new(2).unwrap()),
                action: AssertionAction::Supersede,
                correction: None,
                correction_expected: None,
            }],
        ),
        3,
    );
    let snapshot = state.snapshot();
    let outgoing: Vec<_> = snapshot
        .adjacent(entities[0], AdjacencyDirection::Outgoing, 10)
        .unwrap()
        .into_iter()
        .map(|edge| edge.id)
        .collect();
    assert_eq!(outgoing, vec![edges[0].0, edges[2].0]);
    snapshot.validate_derived_indexes().unwrap();
}
