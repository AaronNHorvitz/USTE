use uste_graph as production;
use uste_testkit as reference;
use uste_txn::TransactionState;
use uste_types::{
    BoundedString, CommitRevision, DatabaseId, NamespaceId, NamespaceRef, RecordRef, UtcInstant,
    Value,
};

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([31; 16]),
        NamespaceId::from_bytes([32; 16]),
    )
}

#[test]
fn deterministic_entity_and_graph_histories_match_the_independent_oracle_each_revision() {
    for generated in [
        reference::generate_entity_history(scope(), 0x1234_5678, 60).unwrap(),
        reference::generate_graph_history(scope(), 0x8765_4321, 20).unwrap(),
    ] {
        let mut oracle = reference::Model::new(scope());
        let mut engine = production::GraphState::new(scope());
        for transaction in generated.transactions {
            let receipt = oracle.apply(&transaction).unwrap();
            let transaction = convert_transaction(&transaction);
            let bytes = production::encode_transaction(&transaction).unwrap();
            let prepared = TransactionState::prepare(&engine, &bytes, None, receipt.revision)
                .unwrap_or_else(|error| panic!("production rejected revision: {error:?}"));
            TransactionState::publish(&mut engine, prepared);
            compare(&oracle, &engine.snapshot(), receipt.revision);
        }
    }
}

fn compare(
    oracle: &reference::Model,
    engine: &production::GraphSnapshot,
    revision: CommitRevision,
) {
    assert_eq!(engine.revision(), Some(revision));
    assert_eq!(engine.records().len(), oracle.records().len());
    for (id, expected) in oracle.records() {
        let actual = engine
            .record(*id)
            .unwrap_or_else(|| panic!("missing {id:?}"));
        assert_eq!(normalize_production(actual), normalize_reference(expected));
    }
    engine.validate_derived_indexes().unwrap();
}

fn convert_transaction(transaction: &reference::Transaction) -> production::GraphTransaction {
    production::GraphTransaction::new(
        scope(),
        transaction
            .operations()
            .iter()
            .map(convert_operation)
            .collect(),
    )
}

fn convert_operation(operation: &reference::Operation) -> production::Operation {
    match operation {
        reference::Operation::Create { expected, record } => production::Operation::Create {
            expected: convert_expected(expected),
            record: convert_new_record(record),
        },
        reference::Operation::ReplaceEntity {
            target,
            expected,
            properties,
        } => production::Operation::ReplaceEntity {
            target: *target,
            expected: convert_expected(expected),
            properties: properties.clone(),
        },
        reference::Operation::ActOnAssertion {
            target,
            expected,
            action,
            correction,
            correction_expected,
        } => production::Operation::ActOnAssertion {
            target: *target,
            expected: convert_expected(expected),
            action: convert_action(*action),
            correction: correction.as_ref().map(convert_assertion),
            correction_expected: correction_expected.as_ref().map(convert_expected),
        },
        reference::Operation::ActOnRelationship {
            target,
            expected,
            action,
            correction,
            correction_expected,
        } => production::Operation::ActOnRelationship {
            target: *target,
            expected: convert_expected(expected),
            action: convert_action(*action),
            correction: correction.as_ref().map(convert_relationship),
            correction_expected: correction_expected.as_ref().map(convert_expected),
        },
        reference::Operation::DeleteEntity {
            target,
            expected,
            policy,
        } => production::Operation::DeleteEntity {
            target: *target,
            expected: convert_expected(expected),
            policy: match policy {
                reference::DeletePolicy::Reject => production::DeletePolicy::Reject,
                reference::DeletePolicy::CascadeAndRetract { maximum_affected } => {
                    production::DeletePolicy::CascadeAndRetract {
                        maximum_affected: u32::try_from(*maximum_affected).unwrap(),
                    }
                }
            },
            affected: Vec::new(),
        },
    }
}

fn convert_new_record(record: &reference::NewRecord) -> production::NewRecord {
    match record {
        reference::NewRecord::Entity(entity) => {
            production::NewRecord::Entity(production::NewEntity {
                id: entity.id,
                entity_type: text("oracle-entity"),
                schema_version: 1,
                properties: entity.properties.clone(),
            })
        }
        reference::NewRecord::Evidence(evidence) => {
            production::NewRecord::Evidence(production::NewEvidence {
                id: evidence.id,
                digest: evidence.digest,
                locator: evidence.locator.clone(),
            })
        }
        reference::NewRecord::Assertion(assertion) => {
            production::NewRecord::Assertion(convert_assertion(assertion))
        }
        reference::NewRecord::Relationship(relationship) => {
            production::NewRecord::Relationship(convert_relationship(relationship))
        }
    }
}

fn convert_assertion(assertion: &reference::NewAssertion) -> production::NewAssertion {
    production::NewAssertion {
        id: assertion.id,
        subject: assertion.subject,
        predicate: assertion.predicate.clone(),
        object: assertion.object.clone(),
        evidence: assertion.evidence.clone(),
        valid_time: convert_time(assertion.valid_time),
    }
}

fn convert_relationship(relationship: &reference::NewRelationship) -> production::NewRelationship {
    production::NewRelationship {
        id: relationship.id,
        from: relationship.from,
        to: relationship.to,
        relationship_type: relationship.relationship_type.clone(),
        properties: relationship.properties.clone(),
        evidence: relationship.evidence.clone(),
        valid_time: convert_time(relationship.valid_time),
    }
}

fn convert_expected(expected: &reference::Expected) -> production::Expected {
    match expected {
        reference::Expected::Absent => production::Expected::Absent,
        reference::Expected::Version(version) => {
            production::Expected::Version(production::RecordVersion::new(version.get()).unwrap())
        }
        reference::Expected::ReadView {
            revision,
            predicate,
        } => production::Expected::ReadView {
            revision: *revision,
            predicate: match predicate {
                reference::Predicate::RecordAbsent(record) => {
                    production::Predicate::RecordAbsent(*record)
                }
                reference::Predicate::RecordVisible(record) => {
                    production::Predicate::RecordVisible(*record)
                }
                reference::Predicate::RecordVersion { record, version } => {
                    production::Predicate::RecordVersion {
                        record: *record,
                        version: production::RecordVersion::new(version.get()).unwrap(),
                    }
                }
            },
        },
    }
}

fn convert_action(action: reference::AssertionAction) -> production::AssertionAction {
    match action {
        reference::AssertionAction::Accept => production::AssertionAction::Accept,
        reference::AssertionAction::Reject => production::AssertionAction::Reject,
        reference::AssertionAction::Dispute => production::AssertionAction::Dispute,
        reference::AssertionAction::Supersede => production::AssertionAction::Supersede,
        reference::AssertionAction::Retract => production::AssertionAction::Retract,
        reference::AssertionAction::Expire => production::AssertionAction::Expire,
        reference::AssertionAction::Correct => production::AssertionAction::Correct,
        reference::AssertionAction::Purge => panic!("purge is not a graph lifecycle action"),
    }
}

fn convert_time(time: reference::ValidTime) -> production::ValidTime {
    match time {
        reference::ValidTime::Unknown => production::ValidTime::Unknown,
        reference::ValidTime::HalfOpen { start, end } => production::ValidTime::HalfOpen {
            start: convert_bound(start),
            end: convert_bound(end),
        },
    }
}

fn convert_bound(bound: reference::IntervalBound) -> production::IntervalBound {
    match bound {
        reference::IntervalBound::Unbounded => production::IntervalBound::Unbounded,
        reference::IntervalBound::Bounded(value) => production::IntervalBound::Bounded(value),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Normalized {
    Entity {
        version: u64,
        lifecycle: u8,
        properties: Value,
        created: u64,
        modified: u64,
    },
    Evidence {
        version: u64,
        digest: [u8; 32],
        locator: BoundedString,
        created: u64,
    },
    Assertion {
        version: u64,
        subject: RecordRef,
        predicate: BoundedString,
        object: Value,
        evidence: Vec<RecordRef>,
        status: u8,
        valid_time: NormalizedTime,
        correction_of: Option<RecordRef>,
        recorded: u64,
        modified: u64,
    },
    Relationship {
        version: u64,
        from: RecordRef,
        to: RecordRef,
        relationship_type: BoundedString,
        properties: Value,
        evidence: Vec<RecordRef>,
        status: u8,
        valid_time: NormalizedTime,
        correction_of: Option<RecordRef>,
        recorded: u64,
        modified: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NormalizedTime {
    Unknown,
    HalfOpen {
        start: Option<UtcInstant>,
        end: Option<UtcInstant>,
    },
}

fn normalize_production(record: &production::Record) -> Normalized {
    match record {
        production::Record::Entity(value) => Normalized::Entity {
            version: value.version.get(),
            lifecycle: value.lifecycle as u8,
            properties: value.properties.clone(),
            created: value.created_revision.get(),
            modified: value.modified_revision.get(),
        },
        production::Record::Evidence(value) => Normalized::Evidence {
            version: value.version.get(),
            digest: value.digest,
            locator: value.locator.clone(),
            created: value.created_revision.get(),
        },
        production::Record::Assertion(value) => Normalized::Assertion {
            version: value.version.get(),
            subject: value.subject,
            predicate: value.predicate.clone(),
            object: value.object.clone(),
            evidence: value.evidence.clone(),
            status: value.status as u8,
            valid_time: normalize_production_time(value.valid_time),
            correction_of: value.correction_of,
            recorded: value.recorded_revision.get(),
            modified: value.modified_revision.get(),
        },
        production::Record::Relationship(value) => Normalized::Relationship {
            version: value.version.get(),
            from: value.from,
            to: value.to,
            relationship_type: value.relationship_type.clone(),
            properties: value.properties.clone(),
            evidence: value.evidence.clone(),
            status: value.status as u8,
            valid_time: normalize_production_time(value.valid_time),
            correction_of: value.correction_of,
            recorded: value.recorded_revision.get(),
            modified: value.modified_revision.get(),
        },
    }
}

fn normalize_reference(record: &reference::Record) -> Normalized {
    match record {
        reference::Record::Entity(value) => Normalized::Entity {
            version: value.version.get(),
            lifecycle: value.lifecycle as u8,
            properties: value.properties.clone(),
            created: value.created_revision.get(),
            modified: value.modified_revision.get(),
        },
        reference::Record::Evidence(value) => Normalized::Evidence {
            version: value.version.get(),
            digest: value.digest,
            locator: value.locator.clone(),
            created: value.created_revision.get(),
        },
        reference::Record::Assertion(value) => Normalized::Assertion {
            version: value.version.get(),
            subject: value.subject,
            predicate: value.predicate.clone(),
            object: value.object.clone(),
            evidence: value.evidence.clone(),
            status: value.status as u8,
            valid_time: normalize_reference_time(value.valid_time),
            correction_of: value.correction_of,
            recorded: value.recorded_revision.get(),
            modified: value.modified_revision.get(),
        },
        reference::Record::Relationship(value) => Normalized::Relationship {
            version: value.version.get(),
            from: value.from,
            to: value.to,
            relationship_type: value.relationship_type.clone(),
            properties: value.properties.clone(),
            evidence: value.evidence.clone(),
            status: value.status as u8,
            valid_time: normalize_reference_time(value.valid_time),
            correction_of: value.correction_of,
            recorded: value.recorded_revision.get(),
            modified: value.modified_revision.get(),
        },
    }
}

fn normalize_production_time(time: production::ValidTime) -> NormalizedTime {
    match time {
        production::ValidTime::Unknown => NormalizedTime::Unknown,
        production::ValidTime::HalfOpen { start, end } => NormalizedTime::HalfOpen {
            start: production_bound(start),
            end: production_bound(end),
        },
    }
}

fn production_bound(bound: production::IntervalBound) -> Option<UtcInstant> {
    match bound {
        production::IntervalBound::Unbounded => None,
        production::IntervalBound::Bounded(value) => Some(value),
    }
}

fn normalize_reference_time(time: reference::ValidTime) -> NormalizedTime {
    match time {
        reference::ValidTime::Unknown => NormalizedTime::Unknown,
        reference::ValidTime::HalfOpen { start, end } => NormalizedTime::HalfOpen {
            start: reference_bound(start),
            end: reference_bound(end),
        },
    }
}

fn reference_bound(bound: reference::IntervalBound) -> Option<UtcInstant> {
    match bound {
        reference::IntervalBound::Unbounded => None,
        reference::IntervalBound::Bounded(value) => Some(value),
    }
}

fn text(value: &str) -> BoundedString {
    BoundedString::new(value.to_owned()).unwrap()
}
