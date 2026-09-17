//! Deterministic valid histories for differential tests.

use core::fmt;

use uste_types::{BoundedString, NamespaceRef, RecordId, RecordRef, Value};

use crate::{
    AssertionAction, Expected, NewAssertion, NewEntity, NewEvidence, NewRecord, NewRelationship,
    Operation, RecordVersion, Transaction, ValidTime,
};

/// Maximum graph groups emitted by one generator call.
pub const MAX_GENERATED_GRAPH_GROUPS: usize = 10_000;

/// Maximum entity transactions emitted by one generator call.
pub const MAX_GENERATED_ENTITY_TRANSACTIONS: usize = 100_000;

/// A deterministic sequence of transactions intended for a fresh model of the named scope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedHistory {
    pub seed: u64,
    pub transactions: Vec<Transaction>,
}

/// Generate a mix of creates and exact-version replacements with stable identities.
///
/// Every group of three transactions creates an entity and replaces it twice. This deliberately
/// avoids random libraries and wall-clock state so failures reproduce from `(scope, seed, count)`.
pub fn generate_entity_history(
    scope: NamespaceRef,
    seed: u64,
    transaction_count: usize,
) -> Result<GeneratedHistory, GenerationError> {
    if transaction_count > MAX_GENERATED_ENTITY_TRANSACTIONS {
        return Err(GenerationError::TooManyTransactions {
            actual: transaction_count,
            maximum: MAX_GENERATED_ENTITY_TRANSACTIONS,
        });
    }
    let mut random = seed;
    let mut transactions = Vec::with_capacity(transaction_count);
    for index in 0..transaction_count {
        let entity_index = index / 3;
        let id = record_ref(scope, seed, entity_index as u64);
        let phase = index % 3;
        let operation = if phase == 0 {
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id,
                    properties: Value::Unsigned(next_random(&mut random) as u128),
                }),
            }
        } else {
            Operation::ReplaceEntity {
                target: id,
                expected: Expected::Version(
                    RecordVersion::new(phase as u64).expect("phase is nonzero"),
                ),
                properties: Value::Unsigned(next_random(&mut random) as u128),
            }
        };
        transactions.push(Transaction::new(vec![operation]));
    }
    Ok(GeneratedHistory { seed, transactions })
}

/// Generate evidence-backed assertion and relationship lifecycles for differential tests.
///
/// Each group has five transactions: create support records; propose an assertion and
/// relationship; accept both; create linked corrections; then accept the corrections.
pub fn generate_graph_history(
    scope: NamespaceRef,
    seed: u64,
    group_count: usize,
) -> Result<GeneratedHistory, GenerationError> {
    if group_count > MAX_GENERATED_GRAPH_GROUPS {
        return Err(GenerationError::TooManyGroups {
            actual: group_count,
            maximum: MAX_GENERATED_GRAPH_GROUPS,
        });
    }
    let mut random = seed;
    let mut transactions = Vec::with_capacity(group_count.saturating_mul(5));
    for group in 0..group_count {
        let base = (group as u64) * 8;
        let left = record_ref(scope, seed, base);
        let right = record_ref(scope, seed, base + 1);
        let evidence = record_ref(scope, seed, base + 2);
        let assertion = record_ref(scope, seed, base + 3);
        let relationship = record_ref(scope, seed, base + 4);
        let assertion_correction = record_ref(scope, seed, base + 5);
        let relationship_correction = record_ref(scope, seed, base + 6);
        let relation_type = bounded("generated-relation");
        let predicate = bounded("generated-assertion");

        transactions.push(Transaction::new(vec![
            create_entity(left, next_random(&mut random)),
            create_entity(right, next_random(&mut random)),
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Evidence(NewEvidence {
                    id: evidence,
                    digest: digest(next_random(&mut random)),
                    locator: bounded("generated-source"),
                }),
            },
        ]));
        transactions.push(Transaction::new(vec![
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Assertion(NewAssertion {
                    id: assertion,
                    subject: left,
                    predicate: predicate.clone(),
                    object: Value::RecordRef(right),
                    evidence: vec![evidence],
                    valid_time: ValidTime::Unknown,
                }),
            },
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Relationship(NewRelationship {
                    id: relationship,
                    from: left,
                    to: right,
                    relationship_type: relation_type.clone(),
                    properties: Value::Unsigned(next_random(&mut random) as u128),
                    evidence: vec![evidence],
                    valid_time: ValidTime::Unknown,
                }),
            },
        ]));
        transactions.push(Transaction::new(vec![
            action_assertion(assertion, AssertionAction::Accept),
            action_relationship(relationship, AssertionAction::Accept),
        ]));
        transactions.push(Transaction::new(vec![
            Operation::ActOnAssertion {
                target: assertion,
                expected: Expected::Version(RecordVersion::new(2).expect("nonzero")),
                action: AssertionAction::Correct,
                correction: Some(NewAssertion {
                    id: assertion_correction,
                    subject: left,
                    predicate,
                    object: Value::Unsigned(next_random(&mut random) as u128),
                    evidence: vec![evidence],
                    valid_time: ValidTime::Unknown,
                }),
                correction_expected: Some(Expected::Absent),
            },
            Operation::ActOnRelationship {
                target: relationship,
                expected: Expected::Version(RecordVersion::new(2).expect("nonzero")),
                action: AssertionAction::Correct,
                correction: Some(NewRelationship {
                    id: relationship_correction,
                    from: right,
                    to: left,
                    relationship_type: relation_type,
                    properties: Value::Unsigned(next_random(&mut random) as u128),
                    evidence: vec![evidence],
                    valid_time: ValidTime::Unknown,
                }),
                correction_expected: Some(Expected::Absent),
            },
        ]));
        transactions.push(Transaction::new(vec![
            action_assertion(assertion_correction, AssertionAction::Accept),
            action_relationship(relationship_correction, AssertionAction::Accept),
        ]));
    }
    Ok(GeneratedHistory { seed, transactions })
}

/// Invalid generator request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GenerationError {
    TooManyTransactions { actual: usize, maximum: usize },
    TooManyGroups { actual: usize, maximum: usize },
}

impl fmt::Display for GenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for GenerationError {}

fn create_entity(id: RecordRef, value: u64) -> Operation {
    Operation::Create {
        expected: Expected::Absent,
        record: NewRecord::Entity(NewEntity {
            id,
            properties: Value::Unsigned(value as u128),
        }),
    }
}

fn action_assertion(target: RecordRef, action: AssertionAction) -> Operation {
    Operation::ActOnAssertion {
        target,
        expected: Expected::Version(RecordVersion::FIRST),
        action,
        correction: None,
        correction_expected: None,
    }
}

fn action_relationship(target: RecordRef, action: AssertionAction) -> Operation {
    Operation::ActOnRelationship {
        target,
        expected: Expected::Version(RecordVersion::FIRST),
        action,
        correction: None,
        correction_expected: None,
    }
}

fn bounded(value: &str) -> BoundedString {
    BoundedString::new(value.to_owned()).expect("fixed generator strings are bounded")
}

fn digest(value: u64) -> [u8; 32] {
    let mut digest = [0_u8; 32];
    for (index, chunk) in digest.chunks_exact_mut(8).enumerate() {
        chunk.copy_from_slice(&value.wrapping_add(index as u64).to_le_bytes());
    }
    digest
}

fn record_ref(scope: NamespaceRef, seed: u64, counter: u64) -> RecordRef {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&seed.to_le_bytes());
    bytes[8..].copy_from_slice(&counter.to_le_bytes());
    RecordRef::new(
        scope.database(),
        scope.namespace(),
        RecordId::from_bytes(bytes),
    )
}

fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}
