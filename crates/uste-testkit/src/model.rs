//! Transactional state-machine oracle.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use uste_types::{CommitRevision, NamespaceRef, RecordRef, Value};

use crate::record::{
    AssertionAction, AssertionRecord, AssertionStatus, DeletePolicy, EntityLifecycle, EntityRecord,
    EvidenceRecord, NewAssertion, NewRecord, NewRelationship, Record, RecordVersion,
    RelationshipRecord,
};

/// Accepted `limits-v1` maximum operation count in one transaction.
pub const MAX_TRANSACTION_OPERATIONS: usize = 10_000;

/// Accepted `limits-v1` maximum record-reference occurrences in one transaction request.
///
/// The conservative oracle count includes operation targets, new record IDs, endpoint/evidence
/// references, read predicates, and every reference occurrence nested in a generic value.
pub const MAX_TRANSACTION_REFERENCES: usize = 100_000;

/// A declared predicate for a mutation based on an exact retained read view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Predicate {
    /// The record was absent at the read view and must remain absent.
    RecordAbsent(RecordRef),
    /// The record was visible at the read view and must remain visible.
    RecordVisible(RecordRef),
    /// The record had this exact version and must retain it.
    RecordVersion {
        record: RecordRef,
        version: RecordVersion,
    },
}

/// Mutation precondition, evaluated against the latest root before any operation is applied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Expected {
    /// The operation target must not exist.
    Absent,
    /// The operation target must have the exact logical version.
    Version(RecordVersion),
    /// A predicate true at an exact retained read view must still be true.
    ReadView {
        revision: CommitRevision,
        predicate: Predicate,
    },
}

/// One constrained state mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Operation {
    Create {
        expected: Expected,
        record: NewRecord,
    },
    ReplaceEntity {
        target: RecordRef,
        expected: Expected,
        properties: Value,
    },
    ActOnAssertion {
        target: RecordRef,
        expected: Expected,
        action: AssertionAction,
        correction: Option<NewAssertion>,
        correction_expected: Option<Expected>,
    },
    ActOnRelationship {
        target: RecordRef,
        expected: Expected,
        action: AssertionAction,
        correction: Option<NewRelationship>,
        correction_expected: Option<Expected>,
    },
    DeleteEntity {
        target: RecordRef,
        expected: Expected,
        policy: DeletePolicy,
    },
}

impl Operation {
    const fn target(&self) -> RecordRef {
        match self {
            Self::Create { record, .. } => record.id(),
            Self::ReplaceEntity { target, .. }
            | Self::ActOnAssertion { target, .. }
            | Self::ActOnRelationship { target, .. }
            | Self::DeleteEntity { target, .. } => *target,
        }
    }

    const fn expected(&self) -> &Expected {
        match self {
            Self::Create { expected, .. }
            | Self::ReplaceEntity { expected, .. }
            | Self::ActOnAssertion { expected, .. }
            | Self::ActOnRelationship { expected, .. }
            | Self::DeleteEntity { expected, .. } => expected,
        }
    }
}

/// A bounded atomic collection of constrained operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transaction {
    operations: Vec<Operation>,
}

impl Transaction {
    #[must_use]
    pub fn new(operations: Vec<Operation>) -> Self {
        Self { operations }
    }

    #[must_use]
    pub fn operations(&self) -> &[Operation] {
        &self.operations
    }
}

/// Deterministic result of a committed transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitReceipt {
    pub revision: CommitRevision,
    pub affected: Vec<RecordRef>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct State {
    records: BTreeMap<RecordRef, Record>,
}

/// Independent in-memory reference model for one explicit namespace scope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Model {
    scope: NamespaceRef,
    revision: Option<CommitRevision>,
    state: State,
    history: BTreeMap<CommitRevision, State>,
}

impl Model {
    /// Create an empty pre-commit model for one database/namespace scope.
    #[must_use]
    pub fn new(scope: NamespaceRef) -> Self {
        Self {
            scope,
            revision: None,
            state: State::default(),
            history: BTreeMap::new(),
        }
    }

    /// The latest committed revision, or `None` before the first transaction.
    #[must_use]
    pub const fn current_revision(&self) -> Option<CommitRevision> {
        self.revision
    }

    /// Read a record from the latest state, including terminal/tombstone records.
    #[must_use]
    pub fn record(&self, id: RecordRef) -> Option<&Record> {
        self.state.records.get(&id)
    }

    /// Read a record exactly as it existed at a retained committed revision.
    pub fn record_at(
        &self,
        revision: CommitRevision,
        id: RecordRef,
    ) -> Result<Option<&Record>, ModelError> {
        self.validate_scope(id)?;
        self.history
            .get(&revision)
            .map(|state| state.records.get(&id))
            .ok_or(ModelError::UnknownReadView(revision))
    }

    /// Iterate over latest records in stable identity order.
    pub fn records(&self) -> impl ExactSizeIterator<Item = (&RecordRef, &Record)> {
        self.state.records.iter()
    }

    /// Apply a transaction atomically. Every failure leaves state, revision and history unchanged.
    pub fn apply(&mut self, transaction: &Transaction) -> Result<CommitReceipt, ModelError> {
        let count = transaction.operations.len();
        if count == 0 {
            return Err(ModelError::EmptyTransaction);
        }
        if count > MAX_TRANSACTION_OPERATIONS {
            return Err(ModelError::TooManyOperations {
                actual: count,
                maximum: MAX_TRANSACTION_OPERATIONS,
            });
        }
        let reference_count = transaction
            .operations
            .iter()
            .fold(0_usize, |count, operation| {
                count.saturating_add(operation_reference_count(operation))
            });
        if reference_count > MAX_TRANSACTION_REFERENCES {
            return Err(ModelError::TooManyReferences {
                actual: reference_count,
                maximum: MAX_TRANSACTION_REFERENCES,
            });
        }

        for operation in &transaction.operations {
            self.validate_scope(operation.target())?;
            self.validate_expected(operation.target(), operation.expected())?;
            self.validate_correction_precondition(operation)?;
        }

        let revision = match self.revision {
            Some(current) => current
                .checked_next()
                .map_err(|_| ModelError::RevisionExhausted)?,
            None => CommitRevision::FIRST,
        };
        let mut next = self.state.clone();
        let mut affected = BTreeSet::new();
        for operation in &transaction.operations {
            apply_operation(&mut next, operation, revision, &mut affected)?;
        }
        validate_state(self.scope, &next)?;

        self.state = next;
        self.revision = Some(revision);
        self.history.insert(revision, self.state.clone());
        Ok(CommitReceipt {
            revision,
            affected: affected.into_iter().collect(),
        })
    }

    fn validate_scope(&self, record: RecordRef) -> Result<(), ModelError> {
        if record.database() != self.scope.database()
            || record.namespace() != self.scope.namespace()
        {
            return Err(ModelError::ScopeMismatch(record));
        }
        Ok(())
    }

    fn validate_expected(&self, target: RecordRef, expected: &Expected) -> Result<(), ModelError> {
        match expected {
            Expected::Absent => {
                if self.state.records.contains_key(&target) {
                    Err(ModelError::PreconditionFailed(target))
                } else {
                    Ok(())
                }
            }
            Expected::Version(version) => match self.state.records.get(&target) {
                Some(record) if record.version() == *version => Ok(()),
                _ => Err(ModelError::PreconditionFailed(target)),
            },
            Expected::ReadView {
                revision,
                predicate,
            } => {
                self.validate_predicate_scope(predicate)?;
                let read_state = self
                    .history
                    .get(revision)
                    .ok_or(ModelError::UnknownReadView(*revision))?;
                if !evaluate_predicate(read_state, predicate) {
                    return Err(ModelError::PredicateWasFalse(*revision));
                }
                if !evaluate_predicate(&self.state, predicate) {
                    return Err(ModelError::PredicateChanged(*revision));
                }
                Ok(())
            }
        }
    }

    fn validate_predicate_scope(&self, predicate: &Predicate) -> Result<(), ModelError> {
        match predicate {
            Predicate::RecordAbsent(record)
            | Predicate::RecordVisible(record)
            | Predicate::RecordVersion { record, .. } => self.validate_scope(*record),
        }
    }

    fn validate_correction_precondition(&self, operation: &Operation) -> Result<(), ModelError> {
        let (action, correction_id, correction_expected) = match operation {
            Operation::ActOnAssertion {
                action,
                correction,
                correction_expected,
                ..
            } => (
                *action,
                correction.as_ref().map(|record| record.id),
                correction_expected,
            ),
            Operation::ActOnRelationship {
                action,
                correction,
                correction_expected,
                ..
            } => (
                *action,
                correction.as_ref().map(|record| record.id),
                correction_expected,
            ),
            _ => return Ok(()),
        };
        if action == AssertionAction::Correct {
            let correction_id = correction_id.ok_or(ModelError::CorrectionRequired)?;
            let expected = correction_expected
                .as_ref()
                .ok_or(ModelError::CorrectionPreconditionRequired)?;
            self.validate_scope(correction_id)?;
            self.validate_expected(correction_id, expected)
        } else if correction_id.is_some() || correction_expected.is_some() {
            Err(ModelError::UnexpectedCorrection)
        } else {
            Ok(())
        }
    }
}

fn operation_reference_count(operation: &Operation) -> usize {
    let expected_count = expected_reference_count(operation.expected());
    match operation {
        Operation::Create { record, .. } => {
            expected_count.saturating_add(new_record_references(record))
        }
        Operation::ReplaceEntity { properties, .. } => expected_count
            .saturating_add(1)
            .saturating_add(value_reference_count(properties)),
        Operation::ActOnAssertion {
            correction,
            correction_expected,
            ..
        } => expected_count
            .saturating_add(1)
            .saturating_add(
                correction_expected
                    .as_ref()
                    .map_or(0, expected_reference_count),
            )
            .saturating_add(correction.as_ref().map_or(0, new_assertion_references)),
        Operation::ActOnRelationship {
            correction,
            correction_expected,
            ..
        } => expected_count
            .saturating_add(1)
            .saturating_add(
                correction_expected
                    .as_ref()
                    .map_or(0, expected_reference_count),
            )
            .saturating_add(correction.as_ref().map_or(0, new_relationship_references)),
        Operation::DeleteEntity { .. } => expected_count.saturating_add(1),
    }
}

fn expected_reference_count(expected: &Expected) -> usize {
    match expected {
        Expected::Absent | Expected::Version(_) => 0,
        Expected::ReadView { predicate, .. } => match predicate {
            Predicate::RecordAbsent(_)
            | Predicate::RecordVisible(_)
            | Predicate::RecordVersion { .. } => 1,
        },
    }
}

fn new_record_references(record: &NewRecord) -> usize {
    match record {
        NewRecord::Entity(entity) => {
            1_usize.saturating_add(value_reference_count(&entity.properties))
        }
        NewRecord::Evidence(_) => 1,
        NewRecord::Assertion(assertion) => new_assertion_references(assertion),
        NewRecord::Relationship(relationship) => new_relationship_references(relationship),
    }
}

fn new_assertion_references(assertion: &NewAssertion) -> usize {
    2_usize
        .saturating_add(assertion.evidence.len())
        .saturating_add(value_reference_count(&assertion.object))
}

fn new_relationship_references(relationship: &NewRelationship) -> usize {
    3_usize
        .saturating_add(relationship.evidence.len())
        .saturating_add(value_reference_count(&relationship.properties))
}

fn value_reference_count(value: &Value) -> usize {
    match value {
        Value::RecordRef(_) => 1,
        Value::List(list) => list.as_slice().iter().fold(0_usize, |count, child| {
            count.saturating_add(value_reference_count(child))
        }),
        Value::Map(map) => map.as_slice().iter().fold(0_usize, |count, (_, child)| {
            count.saturating_add(value_reference_count(child))
        }),
        _ => 0,
    }
}

fn evaluate_predicate(state: &State, predicate: &Predicate) -> bool {
    match predicate {
        Predicate::RecordAbsent(record) => !state.records.contains_key(record),
        Predicate::RecordVisible(record) => is_visible(state, *record),
        Predicate::RecordVersion { record, version } => state
            .records
            .get(record)
            .is_some_and(|candidate| candidate.version() == *version),
    }
}

fn apply_operation(
    state: &mut State,
    operation: &Operation,
    revision: CommitRevision,
    affected: &mut BTreeSet<RecordRef>,
) -> Result<(), ModelError> {
    match operation {
        Operation::Create { record, .. } => create_record(state, record, revision, affected),
        Operation::ReplaceEntity {
            target, properties, ..
        } => {
            let Some(Record::Entity(entity)) = state.records.get_mut(target) else {
                return Err(wrong_kind_or_missing(state, *target, "entity"));
            };
            if entity.lifecycle != EntityLifecycle::Active {
                return Err(ModelError::NotVisible(*target));
            }
            entity.version = next_version(entity.version, *target)?;
            entity.properties = properties.clone();
            entity.modified_revision = revision;
            affected.insert(*target);
            Ok(())
        }
        Operation::ActOnAssertion {
            target,
            action,
            correction,
            ..
        } => apply_assertion_action(
            state,
            *target,
            *action,
            correction.as_ref(),
            revision,
            affected,
        ),
        Operation::ActOnRelationship {
            target,
            action,
            correction,
            ..
        } => apply_relationship_action(
            state,
            *target,
            *action,
            correction.as_ref(),
            revision,
            affected,
        ),
        Operation::DeleteEntity { target, policy, .. } => {
            delete_entity(state, *target, *policy, revision, affected)
        }
    }
}

fn create_record(
    state: &mut State,
    new: &NewRecord,
    revision: CommitRevision,
    affected: &mut BTreeSet<RecordRef>,
) -> Result<(), ModelError> {
    let id = new.id();
    if state.records.contains_key(&id) {
        return Err(ModelError::AlreadyExists(id));
    }
    let record = match new {
        NewRecord::Entity(entity) => Record::Entity(EntityRecord {
            id,
            version: RecordVersion::FIRST,
            lifecycle: EntityLifecycle::Active,
            properties: entity.properties.clone(),
            created_revision: revision,
            modified_revision: revision,
        }),
        NewRecord::Evidence(evidence) => Record::Evidence(EvidenceRecord {
            id,
            version: RecordVersion::FIRST,
            digest: evidence.digest,
            locator: evidence.locator.clone(),
            created_revision: revision,
        }),
        NewRecord::Assertion(assertion) => {
            if !assertion.valid_time.is_valid() {
                return Err(ModelError::InvalidInterval(assertion.id));
            }
            Record::Assertion(assertion_record(assertion, None, revision))
        }
        NewRecord::Relationship(relationship) => {
            if !relationship.valid_time.is_valid() {
                return Err(ModelError::InvalidInterval(relationship.id));
            }
            Record::Relationship(relationship_record(relationship, None, revision))
        }
    };
    state.records.insert(id, record);
    affected.insert(id);
    Ok(())
}

fn assertion_record(
    assertion: &NewAssertion,
    correction_of: Option<RecordRef>,
    revision: CommitRevision,
) -> AssertionRecord {
    AssertionRecord {
        id: assertion.id,
        version: RecordVersion::FIRST,
        subject: assertion.subject,
        predicate: assertion.predicate.clone(),
        object: assertion.object.clone(),
        evidence: assertion.evidence.clone(),
        status: AssertionStatus::Proposed,
        valid_time: assertion.valid_time,
        correction_of,
        recorded_revision: revision,
        modified_revision: revision,
    }
}

fn relationship_record(
    relationship: &NewRelationship,
    correction_of: Option<RecordRef>,
    revision: CommitRevision,
) -> RelationshipRecord {
    RelationshipRecord {
        id: relationship.id,
        version: RecordVersion::FIRST,
        from: relationship.from,
        to: relationship.to,
        relationship_type: relationship.relationship_type.clone(),
        properties: relationship.properties.clone(),
        evidence: relationship.evidence.clone(),
        status: AssertionStatus::Proposed,
        valid_time: relationship.valid_time,
        correction_of,
        recorded_revision: revision,
        modified_revision: revision,
    }
}

fn apply_assertion_action(
    state: &mut State,
    target: RecordRef,
    action: AssertionAction,
    correction: Option<&NewAssertion>,
    revision: CommitRevision,
    affected: &mut BTreeSet<RecordRef>,
) -> Result<(), ModelError> {
    let status = match state.records.get(&target) {
        Some(Record::Assertion(assertion)) => assertion.status,
        _ => return Err(wrong_kind_or_missing(state, target, "assertion")),
    };

    if action == AssertionAction::Correct {
        if status != AssertionStatus::Accepted {
            return Err(ModelError::InvalidTransition { status, action });
        }
        let correction = correction.ok_or(ModelError::CorrectionRequired)?;
        if state.records.contains_key(&correction.id) {
            return Err(ModelError::AlreadyExists(correction.id));
        }
        if !correction.valid_time.is_valid() {
            return Err(ModelError::InvalidInterval(correction.id));
        }
        state.records.insert(
            correction.id,
            Record::Assertion(assertion_record(correction, Some(target), revision)),
        );
        affected.insert(correction.id);
        return Ok(());
    }
    if correction.is_some() {
        return Err(ModelError::UnexpectedCorrection);
    }

    let next_status = transition(status, action)?;
    let Some(Record::Assertion(assertion)) = state.records.get_mut(&target) else {
        unreachable!("assertion kind checked above")
    };
    assertion.status = next_status;
    assertion.version = next_version(assertion.version, target)?;
    assertion.modified_revision = revision;
    affected.insert(target);
    Ok(())
}

fn apply_relationship_action(
    state: &mut State,
    target: RecordRef,
    action: AssertionAction,
    correction: Option<&NewRelationship>,
    revision: CommitRevision,
    affected: &mut BTreeSet<RecordRef>,
) -> Result<(), ModelError> {
    let status = match state.records.get(&target) {
        Some(Record::Relationship(relationship)) => relationship.status,
        _ => return Err(wrong_kind_or_missing(state, target, "relationship")),
    };

    if action == AssertionAction::Correct {
        if status != AssertionStatus::Accepted {
            return Err(ModelError::InvalidTransition { status, action });
        }
        let correction = correction.ok_or(ModelError::CorrectionRequired)?;
        if state.records.contains_key(&correction.id) {
            return Err(ModelError::AlreadyExists(correction.id));
        }
        if !correction.valid_time.is_valid() {
            return Err(ModelError::InvalidInterval(correction.id));
        }
        state.records.insert(
            correction.id,
            Record::Relationship(relationship_record(correction, Some(target), revision)),
        );
        affected.insert(correction.id);
        return Ok(());
    }
    if correction.is_some() {
        return Err(ModelError::UnexpectedCorrection);
    }

    let next_status = transition(status, action)?;
    let Some(Record::Relationship(relationship)) = state.records.get_mut(&target) else {
        unreachable!("relationship kind checked above")
    };
    relationship.status = next_status;
    relationship.version = next_version(relationship.version, target)?;
    relationship.modified_revision = revision;
    affected.insert(target);
    Ok(())
}

fn transition(
    status: AssertionStatus,
    action: AssertionAction,
) -> Result<AssertionStatus, ModelError> {
    match (status, action) {
        (AssertionStatus::Proposed, AssertionAction::Accept) => Ok(AssertionStatus::Accepted),
        (AssertionStatus::Proposed, AssertionAction::Reject) => Ok(AssertionStatus::Rejected),
        (AssertionStatus::Accepted, AssertionAction::Dispute) => Ok(AssertionStatus::Disputed),
        (AssertionStatus::Accepted, AssertionAction::Supersede) => Ok(AssertionStatus::Superseded),
        (AssertionStatus::Accepted, AssertionAction::Retract) => Ok(AssertionStatus::Retracted),
        (AssertionStatus::Accepted, AssertionAction::Expire) => Ok(AssertionStatus::Expired),
        _ => Err(ModelError::InvalidTransition { status, action }),
    }
}

fn delete_entity(
    state: &mut State,
    target: RecordRef,
    policy: DeletePolicy,
    revision: CommitRevision,
    affected: &mut BTreeSet<RecordRef>,
) -> Result<(), ModelError> {
    match state.records.get(&target) {
        Some(Record::Entity(entity)) if entity.lifecycle == EntityLifecycle::Active => {}
        Some(Record::Entity(_)) => return Err(ModelError::NotVisible(target)),
        _ => return Err(wrong_kind_or_missing(state, target, "entity")),
    }

    let accepted_relationships: Vec<RecordRef> = state
        .records
        .values()
        .filter_map(|record| match record {
            Record::Relationship(relationship)
                if relationship.status == AssertionStatus::Accepted
                    && (relationship.from == target
                        || relationship.to == target
                        || value_contains(&relationship.properties, target)) =>
            {
                Some(relationship.id)
            }
            _ => None,
        })
        .collect();
    let accepted_assertions: Vec<RecordRef> = state
        .records
        .values()
        .filter_map(|record| match record {
            Record::Assertion(assertion)
                if assertion.status == AssertionStatus::Accepted
                    && (assertion.subject == target
                        || value_contains(&assertion.object, target)) =>
            {
                Some(assertion.id)
            }
            _ => None,
        })
        .collect();
    let proposed_references = state
        .records
        .values()
        .filter(|record| match record {
            Record::Assertion(assertion) => {
                assertion.status == AssertionStatus::Proposed
                    && (assertion.subject == target || value_contains(&assertion.object, target))
            }
            Record::Relationship(relationship) => {
                relationship.status == AssertionStatus::Proposed
                    && (relationship.from == target
                        || relationship.to == target
                        || value_contains(&relationship.properties, target))
            }
            _ => false,
        })
        .count();
    let entity_property_references = state
        .records
        .values()
        .filter(|record| match record {
            Record::Entity(entity) => {
                entity.id != target
                    && entity.lifecycle == EntityLifecycle::Active
                    && value_contains(&entity.properties, target)
            }
            _ => false,
        })
        .count();
    let dependent_count =
        accepted_relationships.len() + accepted_assertions.len() + entity_property_references;

    match policy {
        DeletePolicy::Reject if dependent_count != 0 || proposed_references != 0 => {
            return Err(ModelError::DeleteRestricted {
                record: target,
                dependents: dependent_count.saturating_add(proposed_references),
            });
        }
        DeletePolicy::Reject => {}
        DeletePolicy::CascadeAndRetract { maximum_affected } => {
            if proposed_references != 0 {
                return Err(ModelError::ProposedClaimBlocksDelete {
                    record: target,
                    dependents: proposed_references,
                });
            }
            if entity_property_references != 0 {
                return Err(ModelError::UncascadeableReferences {
                    record: target,
                    dependents: entity_property_references,
                });
            }
            if dependent_count > maximum_affected {
                return Err(ModelError::CascadeLimitExceeded {
                    actual: dependent_count,
                    maximum: maximum_affected,
                });
            }
            for relationship_id in accepted_relationships {
                let Some(Record::Relationship(relationship)) =
                    state.records.get_mut(&relationship_id)
                else {
                    unreachable!("collected from relationships")
                };
                relationship.status = AssertionStatus::Retracted;
                relationship.version = next_version(relationship.version, relationship_id)?;
                relationship.modified_revision = revision;
                affected.insert(relationship_id);
            }
            for assertion_id in accepted_assertions {
                let Some(Record::Assertion(assertion)) = state.records.get_mut(&assertion_id)
                else {
                    unreachable!("collected from assertions")
                };
                assertion.status = AssertionStatus::Retracted;
                assertion.version = next_version(assertion.version, assertion_id)?;
                assertion.modified_revision = revision;
                affected.insert(assertion_id);
            }
        }
    }

    let Some(Record::Entity(entity)) = state.records.get_mut(&target) else {
        unreachable!("entity kind checked above")
    };
    entity.lifecycle = EntityLifecycle::Deleted;
    entity.version = next_version(entity.version, target)?;
    entity.modified_revision = revision;
    affected.insert(target);
    Ok(())
}

fn validate_state(scope: NamespaceRef, state: &State) -> Result<(), ModelError> {
    for (id, record) in &state.records {
        validate_ref_scope(scope, *id)?;
        match record {
            Record::Entity(entity) => {
                validate_value_scope(scope, &entity.properties)?;
                if entity.lifecycle == EntityLifecycle::Active {
                    validate_value(scope, state, &entity.properties)?;
                }
            }
            Record::Evidence(_) => {}
            Record::Assertion(assertion) => {
                validate_ref_scope(scope, assertion.subject)?;
                validate_value_scope(scope, &assertion.object)?;
                if !assertion.valid_time.is_valid() {
                    return Err(ModelError::InvalidInterval(assertion.id));
                }
                if assertion.evidence.is_empty() {
                    return Err(ModelError::EvidenceRequired(assertion.id));
                }
                let mut unique = BTreeSet::new();
                for evidence in &assertion.evidence {
                    validate_ref_scope(scope, *evidence)?;
                    if !unique.insert(*evidence) {
                        return Err(ModelError::DuplicateEvidence(*evidence));
                    }
                    if !matches!(state.records.get(evidence), Some(Record::Evidence(_))) {
                        return Err(ModelError::MissingEvidence(*evidence));
                    }
                }
                if matches!(
                    assertion.status,
                    AssertionStatus::Proposed | AssertionStatus::Accepted
                ) {
                    require_active_entity(state, assertion.subject)?;
                    validate_value(scope, state, &assertion.object)?;
                }
                if let Some(previous) = assertion.correction_of {
                    validate_ref_scope(scope, previous)?;
                    if !matches!(state.records.get(&previous), Some(Record::Assertion(_))) {
                        return Err(ModelError::ReferenceNotVisible(previous));
                    }
                }
            }
            Record::Relationship(relationship) => {
                validate_ref_scope(scope, relationship.from)?;
                validate_ref_scope(scope, relationship.to)?;
                validate_value_scope(scope, &relationship.properties)?;
                validate_evidence(state, scope, relationship.id, &relationship.evidence)?;
                if !relationship.valid_time.is_valid() {
                    return Err(ModelError::InvalidInterval(relationship.id));
                }
                if matches!(
                    relationship.status,
                    AssertionStatus::Proposed | AssertionStatus::Accepted
                ) {
                    require_active_entity(state, relationship.from)?;
                    require_active_entity(state, relationship.to)?;
                    validate_value(scope, state, &relationship.properties)?;
                }
                if let Some(previous) = relationship.correction_of {
                    validate_ref_scope(scope, previous)?;
                    if !matches!(state.records.get(&previous), Some(Record::Relationship(_))) {
                        return Err(ModelError::ReferenceNotVisible(previous));
                    }
                }
            }
        }
    }
    Ok(())
}

fn validate_evidence(
    state: &State,
    scope: NamespaceRef,
    owner: RecordRef,
    evidence: &[RecordRef],
) -> Result<(), ModelError> {
    if evidence.is_empty() {
        return Err(ModelError::EvidenceRequired(owner));
    }
    let mut unique = BTreeSet::new();
    for evidence in evidence {
        validate_ref_scope(scope, *evidence)?;
        if !unique.insert(*evidence) {
            return Err(ModelError::DuplicateEvidence(*evidence));
        }
        if !matches!(state.records.get(evidence), Some(Record::Evidence(_))) {
            return Err(ModelError::MissingEvidence(*evidence));
        }
    }
    Ok(())
}

fn validate_value(scope: NamespaceRef, state: &State, value: &Value) -> Result<(), ModelError> {
    match value {
        Value::RecordRef(record) => {
            validate_ref_scope(scope, *record)?;
            if !is_visible(state, *record) {
                return Err(ModelError::ReferenceNotVisible(*record));
            }
        }
        Value::List(list) => {
            for child in list.as_slice() {
                validate_value(scope, state, child)?;
            }
        }
        Value::Map(map) => {
            for (_, child) in map.as_slice() {
                validate_value(scope, state, child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_value_scope(scope: NamespaceRef, value: &Value) -> Result<(), ModelError> {
    match value {
        Value::RecordRef(record) => validate_ref_scope(scope, *record)?,
        Value::List(list) => {
            for child in list.as_slice() {
                validate_value_scope(scope, child)?;
            }
        }
        Value::Map(map) => {
            for (_, child) in map.as_slice() {
                validate_value_scope(scope, child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn value_contains(value: &Value, target: RecordRef) -> bool {
    match value {
        Value::RecordRef(record) => *record == target,
        Value::List(list) => list
            .as_slice()
            .iter()
            .any(|child| value_contains(child, target)),
        Value::Map(map) => map
            .as_slice()
            .iter()
            .any(|(_, child)| value_contains(child, target)),
        _ => false,
    }
}

fn require_active_entity(state: &State, id: RecordRef) -> Result<(), ModelError> {
    match state.records.get(&id) {
        Some(Record::Entity(entity)) if entity.lifecycle == EntityLifecycle::Active => Ok(()),
        _ => Err(ModelError::ReferenceNotVisible(id)),
    }
}

fn is_visible(state: &State, id: RecordRef) -> bool {
    match state.records.get(&id) {
        Some(Record::Entity(entity)) => entity.lifecycle == EntityLifecycle::Active,
        Some(_) => true,
        None => false,
    }
}

fn validate_ref_scope(scope: NamespaceRef, record: RecordRef) -> Result<(), ModelError> {
    if record.database() == scope.database() && record.namespace() == scope.namespace() {
        Ok(())
    } else {
        Err(ModelError::ScopeMismatch(record))
    }
}

fn next_version(version: RecordVersion, record: RecordRef) -> Result<RecordVersion, ModelError> {
    version
        .checked_next()
        .map_err(|_| ModelError::RecordVersionExhausted(record))
}

fn wrong_kind_or_missing(state: &State, target: RecordRef, expected: &'static str) -> ModelError {
    if state.records.contains_key(&target) {
        ModelError::WrongRecordKind { target, expected }
    } else {
        ModelError::NotFound(target)
    }
}

/// Typed rejection from the reference state machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelError {
    EmptyTransaction,
    TooManyOperations {
        actual: usize,
        maximum: usize,
    },
    TooManyReferences {
        actual: usize,
        maximum: usize,
    },
    ScopeMismatch(RecordRef),
    PreconditionFailed(RecordRef),
    UnknownReadView(CommitRevision),
    PredicateWasFalse(CommitRevision),
    PredicateChanged(CommitRevision),
    RevisionExhausted,
    RecordVersionExhausted(RecordRef),
    AlreadyExists(RecordRef),
    NotFound(RecordRef),
    NotVisible(RecordRef),
    WrongRecordKind {
        target: RecordRef,
        expected: &'static str,
    },
    InvalidInterval(RecordRef),
    InvalidTransition {
        status: AssertionStatus,
        action: AssertionAction,
    },
    CorrectionRequired,
    CorrectionPreconditionRequired,
    UnexpectedCorrection,
    EvidenceRequired(RecordRef),
    DuplicateEvidence(RecordRef),
    MissingEvidence(RecordRef),
    ReferenceNotVisible(RecordRef),
    DeleteRestricted {
        record: RecordRef,
        dependents: usize,
    },
    ProposedClaimBlocksDelete {
        record: RecordRef,
        dependents: usize,
    },
    UncascadeableReferences {
        record: RecordRef,
        dependents: usize,
    },
    CascadeLimitExceeded {
        actual: usize,
        maximum: usize,
    },
}

impl fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ModelError {}
