use std::collections::BTreeSet;

use uste_testkit::{
    AssertionAction, AssertionStatus, DeletePolicy, EntityLifecycle, Expected, IntervalBound,
    Model, ModelError, NewAssertion, NewEntity, NewEvidence, NewRecord, NewRelationship, Operation,
    Predicate, Record, RecordVersion, Transaction, ValidTime, generate_entity_history,
    generate_graph_history,
};
use uste_types::{
    BoundedString, CommitRevision, DatabaseId, NamespaceId, NamespaceRef, RecordId, RecordRef,
    UtcInstant, Value,
};

const TRANSITIONS: &str = include_str!("../../../acceptance/r0/transitions.tsv");

#[test]
fn literal_r0_assertion_transitions_are_executed_by_the_oracle() {
    for (index, line) in TRANSITIONS.lines().skip(1).enumerate() {
        let columns: Vec<&str> = line.split('\t').collect();
        assert_eq!(columns.len(), 5, "literal row {line:?}");
        let mut fixture = Fixture::new(index as u8);
        fixture.create_assertion();
        match columns[1] {
            "proposed" => {}
            "accepted" => fixture
                .act(fixture.assertion, AssertionAction::Accept, None)
                .unwrap(),
            "rejected" => fixture
                .act(fixture.assertion, AssertionAction::Reject, None)
                .unwrap(),
            other => panic!("unsupported literal from-state {other}"),
        }

        let action = action(columns[2]);
        let correction = (action == AssertionAction::Correct).then(|| NewAssertion {
            id: fixture.record(9),
            subject: fixture.entity,
            predicate: text("name"),
            object: Value::string("corrected".to_owned()).unwrap(),
            evidence: vec![fixture.evidence],
            valid_time: ValidTime::Unknown,
        });
        let before = fixture.model.clone();
        let result = fixture.act(fixture.assertion, action, correction);
        if columns[4] == "ok" {
            result.unwrap_or_else(|error| panic!("{} rejected: {error}", columns[0]));
            if action == AssertionAction::Correct {
                let Record::Assertion(original) = fixture.model.record(fixture.assertion).unwrap()
                else {
                    panic!("assertion kind")
                };
                assert_eq!(original.status, AssertionStatus::Accepted);
                let Record::Assertion(corrected) = fixture.model.record(fixture.record(9)).unwrap()
                else {
                    panic!("correction kind")
                };
                assert_eq!(corrected.status, AssertionStatus::Proposed);
                assert_eq!(corrected.correction_of, Some(fixture.assertion));
            } else {
                let Record::Assertion(assertion) = fixture.model.record(fixture.assertion).unwrap()
                else {
                    panic!("assertion kind")
                };
                assert_eq!(status_name(assertion.status), columns[3]);
            }
        } else {
            assert!(matches!(result, Err(ModelError::InvalidTransition { .. })));
            assert_eq!(fixture.model, before, "failed transition must be atomic");
        }
    }
}

#[test]
fn all_unlisted_lifecycle_pairs_and_purge_are_rejected_atomically() {
    let statuses = [
        AssertionStatus::Proposed,
        AssertionStatus::Accepted,
        AssertionStatus::Rejected,
        AssertionStatus::Disputed,
        AssertionStatus::Superseded,
        AssertionStatus::Retracted,
        AssertionStatus::Expired,
    ];
    let actions = [
        AssertionAction::Accept,
        AssertionAction::Reject,
        AssertionAction::Dispute,
        AssertionAction::Supersede,
        AssertionAction::Retract,
        AssertionAction::Expire,
        AssertionAction::Purge,
    ];
    for status in statuses {
        for action in actions {
            let allowed = matches!(
                (status, action),
                (AssertionStatus::Proposed, AssertionAction::Accept)
                    | (AssertionStatus::Proposed, AssertionAction::Reject)
                    | (AssertionStatus::Accepted, AssertionAction::Dispute)
                    | (AssertionStatus::Accepted, AssertionAction::Supersede)
                    | (AssertionStatus::Accepted, AssertionAction::Retract)
                    | (AssertionStatus::Accepted, AssertionAction::Expire)
            );
            if !reachable_status(status) {
                continue;
            }
            let mut fixture = Fixture::new((status as u8).wrapping_mul(16) + action as u8);
            fixture.create_assertion();
            fixture.drive_to(status);
            let before = fixture.model.clone();
            let result = fixture.act(fixture.assertion, action, None);
            assert_eq!(result.is_ok(), allowed, "{status:?} + {action:?}");
            if !allowed {
                assert_eq!(fixture.model, before);
            }
        }
    }
}

#[test]
fn intervals_preserve_unknown_and_explicit_bounds_and_reject_empty_ranges() {
    let mut fixture = Fixture::new(31);
    fixture.create_support();
    let start = UtcInstant::new(10, 0).unwrap();
    let end = UtcInstant::new(11, 0).unwrap();
    let bounded = ValidTime::HalfOpen {
        start: IntervalBound::Bounded(start),
        end: IntervalBound::Bounded(end),
    };
    assert_eq!(ValidTime::Unknown.contains(start), None);
    assert_eq!(bounded.contains(start), Some(true));
    assert_eq!(bounded.contains(end), Some(false));
    let valid = [
        ValidTime::Unknown,
        ValidTime::HalfOpen {
            start: IntervalBound::Unbounded,
            end: IntervalBound::Unbounded,
        },
        ValidTime::HalfOpen {
            start: IntervalBound::Unbounded,
            end: IntervalBound::Bounded(end),
        },
        ValidTime::HalfOpen {
            start: IntervalBound::Bounded(start),
            end: IntervalBound::Unbounded,
        },
        bounded,
    ];
    let operations = valid
        .iter()
        .enumerate()
        .map(|(index, interval)| {
            create_assertion(&fixture, fixture.record(20 + index as u8), *interval)
        })
        .collect();
    fixture.model.apply(&Transaction::new(operations)).unwrap();
    let first = fixture.record(20);
    let second = fixture.record(21);
    let Record::Assertion(first) = fixture.model.record(first).unwrap() else {
        panic!("assertion kind")
    };
    let Record::Assertion(second) = fixture.model.record(second).unwrap() else {
        panic!("assertion kind")
    };
    assert_ne!(first.valid_time, second.valid_time);

    for invalid in [
        ValidTime::HalfOpen {
            start: IntervalBound::Bounded(start),
            end: IntervalBound::Bounded(start),
        },
        ValidTime::HalfOpen {
            start: IntervalBound::Bounded(end),
            end: IntervalBound::Bounded(start),
        },
    ] {
        let before = fixture.model.clone();
        let result = fixture.model.apply(&Transaction::new(vec![create_assertion(
            &fixture,
            fixture.record(30),
            invalid,
        )]));
        assert!(matches!(result, Err(ModelError::InvalidInterval(_))));
        assert_eq!(fixture.model, before);
    }
}

#[test]
fn scope_reference_and_evidence_closure_are_checked_on_final_state() {
    let mut fixture = Fixture::new(40);
    let relationship = fixture.record(12);
    let other_entity = fixture.record(13);
    fixture
        .model
        .apply(&Transaction::new(vec![
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Relationship(NewRelationship {
                    id: relationship,
                    from: fixture.entity,
                    to: other_entity,
                    relationship_type: text("knows"),
                    properties: Value::Null,
                    evidence: vec![fixture.evidence],
                    valid_time: ValidTime::Unknown,
                }),
            },
            create_evidence(fixture.evidence),
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: other_entity,
                    properties: Value::Null,
                }),
            },
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: fixture.entity,
                    properties: Value::Null,
                }),
            },
        ]))
        .expect("final-state closure permits forward references");

    let missing_evidence = fixture.record(99);
    let before = fixture.model.clone();
    let result = fixture
        .model
        .apply(&Transaction::new(vec![Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Assertion(NewAssertion {
                id: fixture.assertion,
                subject: fixture.entity,
                predicate: text("name"),
                object: Value::Null,
                evidence: vec![missing_evidence],
                valid_time: ValidTime::Unknown,
            }),
        }]));
    assert_eq!(result, Err(ModelError::MissingEvidence(missing_evidence)));
    assert_eq!(fixture.model, before);

    let foreign = RecordRef::new(
        DatabaseId::from_bytes([0xf0; 16]),
        fixture.scope.namespace(),
        RecordId::from_bytes([0xf1; 16]),
    );
    let result = fixture
        .model
        .apply(&Transaction::new(vec![Operation::ReplaceEntity {
            target: fixture.entity,
            expected: Expected::Version(RecordVersion::FIRST),
            properties: Value::list(vec![Value::RecordRef(foreign)]).unwrap(),
        }]));
    assert_eq!(result, Err(ModelError::ScopeMismatch(foreign)));
    assert_eq!(fixture.model, before);
}

#[test]
fn transaction_failures_roll_back_and_success_publishes_one_revision() {
    let mut fixture = Fixture::new(51);
    let before = fixture.model.clone();
    let result = fixture.model.apply(&Transaction::new(vec![
        Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Entity(NewEntity {
                id: fixture.entity,
                properties: Value::Null,
            }),
        },
        Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Assertion(NewAssertion {
                id: fixture.assertion,
                subject: fixture.entity,
                predicate: text("bad"),
                object: Value::Null,
                evidence: Vec::new(),
                valid_time: ValidTime::Unknown,
            }),
        },
    ]));
    assert_eq!(result, Err(ModelError::EvidenceRequired(fixture.assertion)));
    assert_eq!(fixture.model, before);

    let receipt = fixture
        .model
        .apply(&Transaction::new(vec![
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: fixture.entity,
                    properties: Value::Null,
                }),
            },
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Evidence(NewEvidence {
                    id: fixture.evidence,
                    digest: [7; 32],
                    locator: text("fixture.bin"),
                }),
            },
        ]))
        .unwrap();
    assert_eq!(receipt.revision, CommitRevision::FIRST);
    assert_eq!(receipt.affected, vec![fixture.entity, fixture.evidence]);

    let before = fixture.model.clone();
    let result = fixture
        .model
        .apply(&Transaction::new(vec![Operation::ReplaceEntity {
            target: fixture.entity,
            expected: Expected::Version(RecordVersion::new(2).unwrap()),
            properties: Value::Bool(true),
        }]));
    assert_eq!(result, Err(ModelError::PreconditionFailed(fixture.entity)));
    assert_eq!(fixture.model, before);
    assert_eq!(
        fixture.model.apply(&Transaction::new(vec![])),
        Err(ModelError::EmptyTransaction)
    );
}

#[test]
fn operation_cap_is_enforced_before_preconditions_or_publication() {
    let mut fixture = Fixture::new(61);
    let operation = Operation::Create {
        expected: Expected::Absent,
        record: NewRecord::Entity(NewEntity {
            id: fixture.entity,
            properties: Value::Null,
        }),
    };
    let before = fixture.model.clone();
    let result = fixture
        .model
        .apply(&Transaction::new(vec![operation; 10_001]));
    assert_eq!(
        result,
        Err(ModelError::TooManyOperations {
            actual: 10_001,
            maximum: 10_000
        })
    );
    assert_eq!(fixture.model, before);
}

#[test]
fn aggregate_reference_cap_is_enforced_before_cloning_or_publication() {
    let mut fixture = Fixture::new(66);
    let repeated = Value::RecordRef(fixture.entity);
    let first = Value::list(vec![repeated.clone(); 50_000]).unwrap();
    let second = Value::list(vec![repeated; 50_000]).unwrap();
    let properties = Value::list(vec![first, second]).unwrap();
    let before = fixture.model.clone();
    let result = fixture
        .model
        .apply(&Transaction::new(vec![Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Entity(NewEntity {
                id: fixture.entity,
                properties,
            }),
        }]));
    assert_eq!(
        result,
        Err(ModelError::TooManyReferences {
            actual: 100_001,
            maximum: 100_000,
        })
    );
    assert_eq!(fixture.model, before);
}

#[test]
fn retained_read_view_predicates_detect_changed_and_false_assumptions() {
    let mut fixture = Fixture::new(71);
    fixture.create_support();
    let read_revision = CommitRevision::FIRST;
    fixture
        .model
        .apply(&Transaction::new(vec![Operation::ReplaceEntity {
            target: fixture.entity,
            expected: Expected::Version(RecordVersion::FIRST),
            properties: Value::Bool(true),
        }]))
        .unwrap();

    let before = fixture.model.clone();
    let result = fixture
        .model
        .apply(&Transaction::new(vec![Operation::Create {
            expected: Expected::ReadView {
                revision: read_revision,
                predicate: Predicate::RecordVersion {
                    record: fixture.entity,
                    version: RecordVersion::FIRST,
                },
            },
            record: NewRecord::Entity(NewEntity {
                id: fixture.record(88),
                properties: Value::Null,
            }),
        }]));
    assert_eq!(result, Err(ModelError::PredicateChanged(read_revision)));
    assert_eq!(fixture.model, before);

    let absent = fixture.record(89);
    let result = fixture
        .model
        .apply(&Transaction::new(vec![Operation::Create {
            expected: Expected::ReadView {
                revision: read_revision,
                predicate: Predicate::RecordVisible(absent),
            },
            record: NewRecord::Entity(NewEntity {
                id: fixture.record(90),
                properties: Value::Null,
            }),
        }]));
    assert_eq!(result, Err(ModelError::PredicateWasFalse(read_revision)));
    assert_eq!(fixture.model, before);

    let created = fixture.record(91);
    fixture
        .model
        .apply(&Transaction::new(vec![Operation::Create {
            expected: Expected::ReadView {
                revision: read_revision,
                predicate: Predicate::RecordVisible(fixture.entity),
            },
            record: NewRecord::Entity(NewEntity {
                id: created,
                properties: Value::Null,
            }),
        }]))
        .expect("unchanged positive predicate");

    let unknown = CommitRevision::new(99).unwrap();
    let result = fixture
        .model
        .apply(&Transaction::new(vec![Operation::Create {
            expected: Expected::ReadView {
                revision: unknown,
                predicate: Predicate::RecordVisible(fixture.entity),
            },
            record: NewRecord::Entity(NewEntity {
                id: fixture.record(92),
                properties: Value::Null,
            }),
        }]));
    assert_eq!(result, Err(ModelError::UnknownReadView(unknown)));

    let foreign = RecordRef::new(
        DatabaseId::from_bytes([0xee; 16]),
        fixture.scope.namespace(),
        RecordId::from_bytes([0xef; 16]),
    );
    let result = fixture
        .model
        .apply(&Transaction::new(vec![Operation::Create {
            expected: Expected::ReadView {
                revision: read_revision,
                predicate: Predicate::RecordAbsent(foreign),
            },
            record: NewRecord::Entity(NewEntity {
                id: fixture.record(93),
                properties: Value::Null,
            }),
        }]));
    assert_eq!(result, Err(ModelError::ScopeMismatch(foreign)));
}

#[test]
fn later_assertions_and_corrections_do_not_leak_into_earlier_knowledge_views() {
    let mut fixture = Fixture::new(76);
    fixture.create_support();
    let support_revision = fixture.model.current_revision().unwrap();
    fixture
        .model
        .apply(&Transaction::new(vec![create_assertion(
            &fixture,
            fixture.assertion,
            ValidTime::HalfOpen {
                start: IntervalBound::Unbounded,
                end: IntervalBound::Bounded(UtcInstant::new(-1_000, 0).unwrap()),
            },
        )]))
        .unwrap();
    assert_eq!(
        fixture
            .model
            .record_at(support_revision, fixture.assertion)
            .unwrap(),
        None
    );
    fixture
        .act(fixture.assertion, AssertionAction::Accept, None)
        .unwrap();
    let before_correction = fixture.model.current_revision().unwrap();
    let correction_id = fixture.record(77);
    fixture
        .act(
            fixture.assertion,
            AssertionAction::Correct,
            Some(NewAssertion {
                id: correction_id,
                subject: fixture.entity,
                predicate: text("historical-name"),
                object: Value::string("corrected".to_owned()).unwrap(),
                evidence: vec![fixture.evidence],
                valid_time: ValidTime::Unknown,
            }),
        )
        .unwrap();
    assert_eq!(
        fixture
            .model
            .record_at(before_correction, correction_id)
            .unwrap(),
        None
    );
    assert!(fixture.model.record(correction_id).is_some());
}

#[test]
fn correction_creation_needs_declared_absence_and_terminal_refs_stay_scoped() {
    let mut fixture = Fixture::new(79);
    fixture.create_assertion();
    fixture
        .act(fixture.assertion, AssertionAction::Accept, None)
        .unwrap();
    let correction = NewAssertion {
        id: fixture.record(78),
        subject: fixture.entity,
        predicate: text("corrected"),
        object: Value::Null,
        evidence: vec![fixture.evidence],
        valid_time: ValidTime::Unknown,
    };
    let before = fixture.model.clone();
    let result = fixture
        .model
        .apply(&Transaction::new(vec![Operation::ActOnAssertion {
            target: fixture.assertion,
            expected: Expected::Version(RecordVersion::new(2).unwrap()),
            action: AssertionAction::Correct,
            correction: Some(correction),
            correction_expected: None,
        }]));
    assert_eq!(result, Err(ModelError::CorrectionPreconditionRequired));
    assert_eq!(fixture.model, before);

    let mut fixture = Fixture::new(80);
    fixture.create_support();
    let foreign = RecordRef::new(
        DatabaseId::from_bytes([0xda; 16]),
        fixture.scope.namespace(),
        RecordId::from_bytes([0xdb; 16]),
    );
    let before = fixture.model.clone();
    let result = fixture.model.apply(&Transaction::new(vec![
        Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Assertion(NewAssertion {
                id: fixture.assertion,
                subject: fixture.entity,
                predicate: text("foreign-terminal"),
                object: Value::RecordRef(foreign),
                evidence: vec![fixture.evidence],
                valid_time: ValidTime::Unknown,
            }),
        },
        Operation::ActOnAssertion {
            target: fixture.assertion,
            expected: Expected::Absent,
            action: AssertionAction::Reject,
            correction: None,
            correction_expected: None,
        },
    ]));
    assert_eq!(result, Err(ModelError::ScopeMismatch(foreign)));
    assert_eq!(fixture.model, before);
}

#[test]
fn delete_is_reject_by_default_and_bounded_cascade_is_atomic() {
    let mut fixture = Fixture::new(81);
    let other = fixture.record(11);
    let relationship = fixture.record(12);
    fixture
        .model
        .apply(&Transaction::new(vec![
            create_entity(fixture.entity),
            create_entity(other),
            create_evidence(fixture.evidence),
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Assertion(NewAssertion {
                    id: fixture.assertion,
                    subject: fixture.entity,
                    predicate: text("located-at"),
                    object: Value::RecordRef(other),
                    evidence: vec![fixture.evidence],
                    valid_time: ValidTime::Unknown,
                }),
            },
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Relationship(NewRelationship {
                    id: relationship,
                    from: fixture.entity,
                    to: other,
                    relationship_type: text("near"),
                    properties: Value::Null,
                    evidence: vec![fixture.evidence],
                    valid_time: ValidTime::Unknown,
                }),
            },
        ]))
        .unwrap();
    fixture
        .act(fixture.assertion, AssertionAction::Accept, None)
        .unwrap();
    fixture
        .model
        .apply(&Transaction::new(vec![Operation::ActOnRelationship {
            target: relationship,
            expected: Expected::Version(RecordVersion::FIRST),
            action: AssertionAction::Accept,
            correction: None,
            correction_expected: None,
        }]))
        .unwrap();

    let version = fixture.model.record(fixture.entity).unwrap().version();
    let before = fixture.model.clone();
    let reject = fixture
        .model
        .apply(&Transaction::new(vec![Operation::DeleteEntity {
            target: fixture.entity,
            expected: Expected::Version(version),
            policy: DeletePolicy::Reject,
        }]));
    assert!(matches!(
        reject,
        Err(ModelError::DeleteRestricted { dependents: 2, .. })
    ));
    assert_eq!(fixture.model, before);

    let bounded = fixture
        .model
        .apply(&Transaction::new(vec![Operation::DeleteEntity {
            target: fixture.entity,
            expected: Expected::Version(version),
            policy: DeletePolicy::CascadeAndRetract {
                maximum_affected: 1,
            },
        }]));
    assert_eq!(
        bounded,
        Err(ModelError::CascadeLimitExceeded {
            actual: 2,
            maximum: 1
        })
    );
    assert_eq!(fixture.model, before);

    let receipt = fixture
        .model
        .apply(&Transaction::new(vec![Operation::DeleteEntity {
            target: fixture.entity,
            expected: Expected::Version(version),
            policy: DeletePolicy::CascadeAndRetract {
                maximum_affected: 2,
            },
        }]))
        .unwrap();
    assert_eq!(
        receipt.affected.into_iter().collect::<BTreeSet<_>>(),
        [fixture.entity, fixture.assertion, relationship]
            .into_iter()
            .collect()
    );
    let Record::Relationship(relationship) = fixture.model.record(relationship).unwrap() else {
        panic!("relationship kind")
    };
    assert_eq!(relationship.status, AssertionStatus::Retracted);
    let Record::Entity(entity) = fixture.model.record(fixture.entity).unwrap() else {
        panic!("entity kind")
    };
    assert_eq!(entity.lifecycle, EntityLifecycle::Deleted);
    let Record::Assertion(assertion) = fixture.model.record(fixture.assertion).unwrap() else {
        panic!("assertion kind")
    };
    assert_eq!(assertion.status, AssertionStatus::Retracted);
}

#[test]
fn generated_histories_replay_identically_and_injected_conflicts_do_not_mutate() {
    let scope = scope(91);
    let history = generate_entity_history(scope, 0x9e37_79b9_7f4a_7c15, 300).unwrap();
    let mut left = Model::new(scope);
    let mut right = Model::new(scope);
    for transaction in &history.transactions {
        assert_eq!(left.apply(transaction), right.apply(transaction));
    }
    assert_eq!(left, right);
    assert_eq!(left.current_revision().unwrap().get(), 300);
    assert_eq!(left.records().len(), 100);

    let target = record(scope, 0x9e37_79b9_7f4a_7c15, 0);
    let before = left.clone();
    let error = left.apply(&Transaction::new(vec![Operation::ReplaceEntity {
        target,
        expected: Expected::Version(RecordVersion::FIRST),
        properties: Value::Null,
    }]));
    assert_eq!(error, Err(ModelError::PreconditionFailed(target)));
    assert_eq!(left, before);
}

#[test]
fn generated_graph_history_has_independently_expected_claim_projection() {
    let scope = scope(96);
    let history = generate_graph_history(scope, 0xd1b5_4a32_d192_ed03, 40).unwrap();
    assert_eq!(history.transactions.len(), 200);
    let mut model = Model::new(scope);
    for transaction in &history.transactions {
        model.apply(transaction).unwrap();
    }
    assert_eq!(model.records().len(), 280);
    let mut assertions = 0;
    let mut relationships = 0;
    let mut corrections = 0;
    for (_, record) in model.records() {
        match record {
            Record::Assertion(assertion) => {
                assertions += 1;
                corrections += usize::from(assertion.correction_of.is_some());
                assert_eq!(assertion.status, AssertionStatus::Accepted);
            }
            Record::Relationship(relationship) => {
                relationships += 1;
                corrections += usize::from(relationship.correction_of.is_some());
                assert_eq!(relationship.status, AssertionStatus::Accepted);
                assert_eq!(relationship.evidence.len(), 1);
            }
            _ => {}
        }
    }
    assert_eq!((assertions, relationships, corrections), (80, 80, 80));
}

#[test]
fn entity_property_references_block_both_delete_policies_without_partial_change() {
    let mut fixture = Fixture::new(101);
    let dependent = fixture.record(11);
    fixture
        .model
        .apply(&Transaction::new(vec![
            create_entity(fixture.entity),
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: dependent,
                    properties: Value::list(vec![Value::RecordRef(fixture.entity)]).unwrap(),
                }),
            },
        ]))
        .unwrap();
    let before = fixture.model.clone();
    let reject = fixture
        .model
        .apply(&Transaction::new(vec![Operation::DeleteEntity {
            target: fixture.entity,
            expected: Expected::Version(RecordVersion::FIRST),
            policy: DeletePolicy::Reject,
        }]));
    assert!(matches!(
        reject,
        Err(ModelError::DeleteRestricted { dependents: 1, .. })
    ));
    assert_eq!(fixture.model, before);
    let cascade = fixture
        .model
        .apply(&Transaction::new(vec![Operation::DeleteEntity {
            target: fixture.entity,
            expected: Expected::Version(RecordVersion::FIRST),
            policy: DeletePolicy::CascadeAndRetract {
                maximum_affected: 10,
            },
        }]));
    assert_eq!(
        cascade,
        Err(ModelError::UncascadeableReferences {
            record: fixture.entity,
            dependents: 1,
        })
    );
    assert_eq!(fixture.model, before);
}

fn reachable_status(status: AssertionStatus) -> bool {
    matches!(
        status,
        AssertionStatus::Proposed
            | AssertionStatus::Accepted
            | AssertionStatus::Rejected
            | AssertionStatus::Disputed
            | AssertionStatus::Superseded
            | AssertionStatus::Retracted
            | AssertionStatus::Expired
    )
}

fn status_name(status: AssertionStatus) -> &'static str {
    match status {
        AssertionStatus::Proposed => "proposed",
        AssertionStatus::Accepted => "accepted",
        AssertionStatus::Rejected => "rejected",
        AssertionStatus::Disputed => "disputed",
        AssertionStatus::Superseded => "superseded",
        AssertionStatus::Retracted => "retracted",
        AssertionStatus::Expired => "expired",
    }
}

fn action(value: &str) -> AssertionAction {
    match value {
        "accept" => AssertionAction::Accept,
        "reject" => AssertionAction::Reject,
        "dispute" => AssertionAction::Dispute,
        "supersede" => AssertionAction::Supersede,
        "retract" => AssertionAction::Retract,
        "expire" => AssertionAction::Expire,
        "correct" => AssertionAction::Correct,
        "purge" => AssertionAction::Purge,
        other => panic!("unknown literal action {other}"),
    }
}

fn text(value: &str) -> BoundedString {
    BoundedString::new(value.to_owned()).unwrap()
}

fn create_entity(id: RecordRef) -> Operation {
    Operation::Create {
        expected: Expected::Absent,
        record: NewRecord::Entity(NewEntity {
            id,
            properties: Value::Null,
        }),
    }
}

fn create_evidence(id: RecordRef) -> Operation {
    Operation::Create {
        expected: Expected::Absent,
        record: NewRecord::Evidence(NewEvidence {
            id,
            digest: [3; 32],
            locator: text("source.bin"),
        }),
    }
}

fn create_assertion(fixture: &Fixture, id: RecordRef, valid_time: ValidTime) -> Operation {
    Operation::Create {
        expected: Expected::Absent,
        record: NewRecord::Assertion(NewAssertion {
            id,
            subject: fixture.entity,
            predicate: text("name"),
            object: Value::string("value".to_owned()).unwrap(),
            evidence: vec![fixture.evidence],
            valid_time,
        }),
    }
}

struct Fixture {
    scope: NamespaceRef,
    model: Model,
    entity: RecordRef,
    evidence: RecordRef,
    assertion: RecordRef,
}

impl Fixture {
    fn new(seed: u8) -> Self {
        let scope = scope(seed);
        Self {
            model: Model::new(scope),
            entity: record(scope, seed as u64, 1),
            evidence: record(scope, seed as u64, 2),
            assertion: record(scope, seed as u64, 3),
            scope,
        }
    }

    fn record(&self, byte: u8) -> RecordRef {
        RecordRef::new(
            self.scope.database(),
            self.scope.namespace(),
            RecordId::from_bytes([byte; 16]),
        )
    }

    fn create_support(&mut self) {
        self.model
            .apply(&Transaction::new(vec![
                create_entity(self.entity),
                create_evidence(self.evidence),
            ]))
            .unwrap();
    }

    fn create_assertion(&mut self) {
        self.create_support();
        self.model
            .apply(&Transaction::new(vec![create_assertion(
                self,
                self.assertion,
                ValidTime::Unknown,
            )]))
            .unwrap();
    }

    fn act(
        &mut self,
        target: RecordRef,
        action: AssertionAction,
        correction: Option<NewAssertion>,
    ) -> Result<(), ModelError> {
        let version = self.model.record(target).unwrap().version();
        self.model
            .apply(&Transaction::new(vec![Operation::ActOnAssertion {
                target,
                expected: Expected::Version(version),
                action,
                correction_expected: correction.as_ref().map(|_| Expected::Absent),
                correction,
            }]))
            .map(|_| ())
    }

    fn drive_to(&mut self, status: AssertionStatus) {
        match status {
            AssertionStatus::Proposed => {}
            AssertionStatus::Accepted => self
                .act(self.assertion, AssertionAction::Accept, None)
                .unwrap(),
            AssertionStatus::Rejected => self
                .act(self.assertion, AssertionAction::Reject, None)
                .unwrap(),
            AssertionStatus::Disputed => {
                self.act(self.assertion, AssertionAction::Accept, None)
                    .unwrap();
                self.act(self.assertion, AssertionAction::Dispute, None)
                    .unwrap();
            }
            AssertionStatus::Superseded => {
                self.act(self.assertion, AssertionAction::Accept, None)
                    .unwrap();
                self.act(self.assertion, AssertionAction::Supersede, None)
                    .unwrap();
            }
            AssertionStatus::Retracted => {
                self.act(self.assertion, AssertionAction::Accept, None)
                    .unwrap();
                self.act(self.assertion, AssertionAction::Retract, None)
                    .unwrap();
            }
            AssertionStatus::Expired => {
                self.act(self.assertion, AssertionAction::Accept, None)
                    .unwrap();
                self.act(self.assertion, AssertionAction::Expire, None)
                    .unwrap();
            }
        }
    }
}

fn scope(seed: u8) -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([seed; 16]),
        NamespaceId::from_bytes([seed.wrapping_add(1); 16]),
    )
}

fn record(scope: NamespaceRef, seed: u64, counter: u64) -> RecordRef {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&seed.to_le_bytes());
    bytes[8..].copy_from_slice(&counter.to_le_bytes());
    RecordRef::new(
        scope.database(),
        scope.namespace(),
        RecordId::from_bytes(bytes),
    )
}
