use uste_graph::{
    AdjacencyDirection, AssertionAction, DurablePolicyMutation, Expected, GraphError, GraphState,
    GraphTransaction, NewEntity, NewEvidence, NewRecord, NewRelationship, Operation, RecordVersion,
    ValidTime,
};
use uste_policy::{
    Action, NamespaceGrant, NamespacePolicy, PermissionSet, PolicyVersion, PrincipalDigest,
    QuotaLimits,
};
use uste_replay::{
    ReplayEvent, capture_reducer_checkpoint, cold_replay, verify_reducer_checkpoint,
};
use uste_txn::{CheckpointState, CheckpointStateError, TransactionState};
use uste_types::{
    BoundedString, CommitRevision, DatabaseId, NamespaceId, NamespaceRef, RecordId, RecordRef,
    Value,
};

fn scope(value: u8) -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([31; 16]),
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

fn policy(scope: NamespaceRef, version: u64) -> NamespacePolicy {
    let quotas = QuotaLimits::new(1_048_576, 1_048_576, 1_048_576, 2, 1_048_576).unwrap();
    let mut policy = NamespacePolicy::new(scope, PolicyVersion::new(version).unwrap(), quotas);
    policy
        .grant(
            PrincipalDigest::from_bytes([41; 32]),
            NamespaceGrant::new(
                PermissionSet::from_actions([
                    Action::ReadRecord,
                    Action::ReadHistory,
                    Action::Commit,
                ]),
                quotas,
            ),
        )
        .unwrap();
    policy
}

fn publish(state: &mut GraphState, transaction: GraphTransaction, revision: u64) {
    let prepared = state
        .prepare_transaction(&transaction, CommitRevision::new(revision).unwrap())
        .unwrap();
    TransactionState::publish(state, prepared);
}

#[test]
fn graph_checkpoint_round_trip_preserves_history_policy_and_rebuilt_indexes() {
    let scope = scope(32);
    let entity = record(scope, 1);
    let other = record(scope, 2);
    let evidence = record(scope, 3);
    let relationship = record(scope, 4);
    let mut state = GraphState::new(scope);
    publish(
        &mut state,
        GraphTransaction::with_policy_mutation(
            scope,
            vec![
                Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Entity(NewEntity {
                        id: entity,
                        entity_type: BoundedString::new("tracked-item".to_owned()).unwrap(),
                        schema_version: 1,
                        properties: Value::Unsigned(7),
                    }),
                },
                Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Entity(NewEntity {
                        id: other,
                        entity_type: BoundedString::new("location".to_owned()).unwrap(),
                        schema_version: 1,
                        properties: Value::Null,
                    }),
                },
                Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Evidence(NewEvidence {
                        id: evidence,
                        digest: [55; 32],
                        locator: BoundedString::new("source:checkpoint".to_owned()).unwrap(),
                    }),
                },
                Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Relationship(NewRelationship {
                        id: relationship,
                        from: entity,
                        to: other,
                        relationship_type: BoundedString::new("located_at".to_owned()).unwrap(),
                        properties: Value::Null,
                        evidence: vec![evidence],
                        valid_time: ValidTime::Unknown,
                    }),
                },
            ],
            DurablePolicyMutation::Install {
                policy: policy(scope, 1),
            },
        ),
        1,
    );
    publish(
        &mut state,
        GraphTransaction::with_policy_mutation(
            scope,
            vec![Operation::ReplaceEntity {
                target: entity,
                expected: Expected::Version(RecordVersion::FIRST),
                properties: Value::Unsigned(9),
            }],
            DurablePolicyMutation::Replace {
                expected: PolicyVersion::new(1).unwrap(),
                policy: policy(scope, 3),
            },
        ),
        2,
    );
    let correction = record(scope, 5);
    let accept_then_correct = GraphTransaction::new(
        scope,
        vec![
            Operation::ActOnRelationship {
                target: relationship,
                expected: Expected::Version(RecordVersion::FIRST),
                action: AssertionAction::Accept,
                correction: None,
                correction_expected: None,
            },
            Operation::ActOnRelationship {
                target: relationship,
                expected: Expected::Version(RecordVersion::FIRST),
                action: AssertionAction::Correct,
                correction: Some(NewRelationship {
                    id: correction,
                    from: entity,
                    to: other,
                    relationship_type: BoundedString::new("located_at".to_owned()).unwrap(),
                    properties: Value::Null,
                    evidence: vec![evidence],
                    valid_time: ValidTime::Unknown,
                }),
                correction_expected: Some(Expected::Absent),
            },
        ],
    );
    assert_eq!(
        state.prepare_transaction(&accept_then_correct, CommitRevision::new(3).unwrap()),
        Err(GraphError::DuplicateMutation(relationship))
    );
    publish(
        &mut state,
        GraphTransaction::new(
            scope,
            vec![Operation::ActOnRelationship {
                target: relationship,
                expected: Expected::Version(RecordVersion::FIRST),
                action: AssertionAction::Accept,
                correction: None,
                correction_expected: None,
            }],
        ),
        3,
    );
    publish(
        &mut state,
        GraphTransaction::new(
            scope,
            vec![Operation::ActOnRelationship {
                target: relationship,
                expected: Expected::Version(RecordVersion::new(2).unwrap()),
                action: AssertionAction::Correct,
                correction: Some(NewRelationship {
                    id: correction,
                    from: entity,
                    to: other,
                    relationship_type: BoundedString::new("located_at".to_owned()).unwrap(),
                    properties: Value::Null,
                    evidence: vec![evidence],
                    valid_time: ValidTime::Unknown,
                }),
                correction_expected: Some(Expected::Absent),
            }],
        ),
        4,
    );

    let checkpoint = capture_reducer_checkpoint(&state).unwrap();
    let restored = verify_reducer_checkpoint::<GraphState>(scope, &checkpoint).unwrap();
    assert_eq!(restored.snapshot(), state.snapshot());
    restored.snapshot().validate_derived_indexes().unwrap();
    assert_eq!(
        restored
            .snapshot()
            .adjacent(entity, AdjacencyDirection::Outgoing, 1)
            .unwrap()[0]
            .id,
        relationship
    );
    assert_eq!(
        restored
            .snapshot()
            .adjacent(other, AdjacencyDirection::Incoming, 1)
            .unwrap()[0]
            .id,
        relationship
    );
    assert_eq!(
        restored
            .snapshot()
            .supported_by(evidence, 2)
            .unwrap()
            .into_iter()
            .map(uste_graph::Record::id)
            .collect::<Vec<_>>(),
        vec![relationship, correction]
    );
    assert_eq!(
        restored
            .snapshot()
            .record_at(CommitRevision::FIRST, entity)
            .unwrap()
            .unwrap()
            .version()
            .get(),
        1
    );
    assert_eq!(
        restored
            .snapshot()
            .namespace_policy_at(CommitRevision::FIRST)
            .unwrap()
            .unwrap()
            .version()
            .get(),
        1
    );
}

#[test]
fn graph_genesis_has_a_replay_digest_without_becoming_a_publishable_checkpoint() {
    let scope = scope(30);
    let (state, report) = cold_replay(
        GraphState::new(scope),
        std::iter::empty::<ReplayEvent<'static>>(),
    )
    .unwrap();
    assert_eq!(state, GraphState::new(scope));
    assert_eq!(report.frontier, None);
    assert_ne!(report.logical_state_digest, [0; 32]);
    assert_eq!(
        capture_reducer_checkpoint(&state),
        Err(uste_replay::ReplayError::EmptyCheckpointState)
    );
}

#[test]
fn graph_checkpoint_rejects_wrong_metadata_and_every_truncation() {
    let wrong_scope = scope(34);
    let scope = scope(33);
    let entity = record(scope, 1);
    let mut state = GraphState::new(scope);
    publish(
        &mut state,
        GraphTransaction::new(
            scope,
            vec![Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: entity,
                    entity_type: BoundedString::new("entity".to_owned()).unwrap(),
                    schema_version: 1,
                    properties: Value::Null,
                }),
            }],
        ),
        1,
    );
    let snapshot = state.snapshot();
    let encoded = GraphState::encode_checkpoint(&snapshot).unwrap();
    for cut in 0..encoded.len() {
        assert!(
            GraphState::decode_checkpoint(scope, CommitRevision::FIRST, &encoded[..cut]).is_err(),
            "accepted truncated checkpoint at {cut}"
        );
    }
    assert_eq!(
        GraphState::decode_checkpoint(wrong_scope, CommitRevision::FIRST, &encoded),
        Err(CheckpointStateError::Invalid)
    );
    assert_eq!(
        GraphState::decode_checkpoint(scope, CommitRevision::new(2).unwrap(), &encoded),
        Err(CheckpointStateError::Invalid)
    );
    let mut unknown_profile = encoded;
    unknown_profile[0] ^= 0xff;
    assert_eq!(
        GraphState::decode_checkpoint(scope, CommitRevision::FIRST, &unknown_profile),
        Err(CheckpointStateError::UnsupportedProfile)
    );
}

#[test]
fn malformed_history_count_does_not_drive_count_sized_allocation() {
    let scope = scope(35);
    let mut state = GraphState::new(scope);
    publish(
        &mut state,
        GraphTransaction::new(
            scope,
            vec![Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: record(scope, 1),
                    entity_type: BoundedString::new("bounded".to_owned()).unwrap(),
                    schema_version: 1,
                    properties: Value::Null,
                }),
            }],
        ),
        1,
    );
    let mut encoded = GraphState::encode_checkpoint(&state.snapshot()).unwrap();
    let first_record_length =
        usize::try_from(u64::from_be_bytes(encoded[72..80].try_into().unwrap())).unwrap();
    let history_version_count_offset = 80 + first_record_length + 8 + 16;
    encoded[history_version_count_offset..history_version_count_offset + 8]
        .copy_from_slice(&10_000_000_u64.to_be_bytes());
    assert_eq!(
        GraphState::decode_checkpoint(scope, CommitRevision::FIRST, &encoded),
        Err(CheckpointStateError::Invalid)
    );
}

#[test]
fn historical_record_scope_invariants_are_verified() {
    let scope = scope(36);
    let id = record(scope, 1);
    let mut state = GraphState::new(scope);
    publish(
        &mut state,
        GraphTransaction::new(
            scope,
            vec![Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id,
                    entity_type: BoundedString::new("self-linked".to_owned()).unwrap(),
                    schema_version: 1,
                    properties: Value::RecordRef(id),
                }),
            }],
        ),
        1,
    );
    publish(
        &mut state,
        GraphTransaction::new(
            scope,
            vec![Operation::ReplaceEntity {
                target: id,
                expected: Expected::Version(RecordVersion::FIRST),
                properties: Value::Null,
            }],
        ),
        2,
    );
    let mut encoded = GraphState::encode_checkpoint(&state.snapshot()).unwrap();
    let current_length =
        usize::try_from(u64::from_be_bytes(encoded[72..80].try_into().unwrap())).unwrap();
    let version_count_offset = 80 + current_length + 8 + 16;
    let first_history_length = usize::try_from(u64::from_be_bytes(
        encoded[version_count_offset + 8..version_count_offset + 16]
            .try_into()
            .unwrap(),
    ))
    .unwrap();
    let first_history_start = version_count_offset + 16;
    let first_history_end = first_history_start + first_history_length;
    let namespace_id = scope.namespace();
    let namespace = namespace_id.as_bytes();
    let occurrences: Vec<usize> = encoded[first_history_start..first_history_end]
        .windows(namespace.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == namespace).then_some(offset))
        .collect();
    assert!(occurrences.len() >= 2);
    encoded[first_history_start + occurrences[1]] ^= 0x80;
    assert_eq!(
        GraphState::decode_checkpoint(scope, CommitRevision::new(2).unwrap(), &encoded),
        Err(CheckpointStateError::Invalid)
    );
}

#[test]
fn duplicate_mutations_in_one_revision_are_rejected_and_single_steps_restore() {
    let scope = scope(37);
    let id = record(scope, 1);
    let mut state = GraphState::new(scope);
    publish(
        &mut state,
        GraphTransaction::new(
            scope,
            vec![Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id,
                    entity_type: BoundedString::new("versioned".to_owned()).unwrap(),
                    schema_version: 1,
                    properties: Value::Unsigned(1),
                }),
            }],
        ),
        1,
    );
    let duplicate = GraphTransaction::new(
        scope,
        vec![
            Operation::ReplaceEntity {
                target: id,
                expected: Expected::Version(RecordVersion::FIRST),
                properties: Value::Unsigned(2),
            },
            Operation::ReplaceEntity {
                target: id,
                expected: Expected::Version(RecordVersion::FIRST),
                properties: Value::Unsigned(3),
            },
        ],
    );
    assert_eq!(
        state.prepare_transaction(&duplicate, CommitRevision::new(2).unwrap()),
        Err(GraphError::DuplicateMutation(id))
    );
    publish(
        &mut state,
        GraphTransaction::new(
            scope,
            vec![Operation::ReplaceEntity {
                target: id,
                expected: Expected::Version(RecordVersion::FIRST),
                properties: Value::Unsigned(2),
            }],
        ),
        2,
    );
    let checkpoint = capture_reducer_checkpoint(&state).unwrap();
    let restored = verify_reducer_checkpoint::<GraphState>(scope, &checkpoint).unwrap();
    assert_eq!(restored.snapshot(), state.snapshot());
}
