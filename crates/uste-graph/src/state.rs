//! Production graph reducer with symmetric adjacency and provenance indexes.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};
use uste_policy::{
    Action, AuthorizationRequirement, AuthorizationRequirements, NamespacePolicy, Target,
};
use uste_storage::BlobInventory;
use uste_txn::{ApplyError, AuthorizedTransactionState, DurablePolicyChange, TransactionState};
use uste_types::{CommitRevision, NamespaceRef, RecordRef, Value};

use crate::codec::{encode_result_policy, encode_result_record};
use crate::{
    AssertionAction, AssertionRecord, AssertionStatus, DeletePolicy, DurablePolicyMutation,
    EntityLifecycle, EntityRecord, EvidenceRecord, Expected, GraphTransaction, NewAssertion,
    NewRecord, NewRelationship, Operation, Predicate, Record, RecordVersion, RelationshipRecord,
    decode_transaction,
};

pub const MAX_TRANSACTION_OPERATIONS: usize = 10_000;
pub const MAX_TRANSACTION_REFERENCES: usize = 100_000;
pub const MAX_TRAVERSAL_RESULTS: usize = 100_000;
pub const MAX_TRAVERSAL_VISITS: usize = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdjacencyDirection {
    Outgoing,
    Incoming,
    Either,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphSnapshot {
    scope: NamespaceRef,
    revision: Option<CommitRevision>,
    records: BTreeMap<RecordRef, Record>,
    history: BTreeMap<RecordRef, Vec<Record>>,
    outgoing: BTreeMap<RecordRef, BTreeSet<RecordRef>>,
    incoming: BTreeMap<RecordRef, BTreeSet<RecordRef>>,
    provenance: BTreeMap<RecordRef, BTreeSet<RecordRef>>,
    policy: Option<NamespacePolicy>,
    policy_history: BTreeMap<CommitRevision, NamespacePolicy>,
}

impl GraphSnapshot {
    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.scope
    }

    #[must_use]
    pub const fn revision(&self) -> Option<CommitRevision> {
        self.revision
    }

    #[must_use]
    pub fn record(&self, id: RecordRef) -> Option<&Record> {
        self.records.get(&id)
    }

    pub fn records(&self) -> impl ExactSizeIterator<Item = (&RecordRef, &Record)> {
        self.records.iter()
    }

    pub fn record_at(
        &self,
        revision: CommitRevision,
        id: RecordRef,
    ) -> Result<Option<&Record>, GraphError> {
        validate_scope(self.scope, id)?;
        if self.revision.is_none_or(|current| revision > current) {
            return Err(GraphError::UnknownReadView(revision));
        }
        Ok(record_at(&self.history, id, revision))
    }

    #[must_use]
    pub fn namespace_policy(&self) -> Option<&NamespacePolicy> {
        self.policy.as_ref()
    }

    pub fn namespace_policy_at(
        &self,
        revision: CommitRevision,
    ) -> Result<Option<&NamespacePolicy>, GraphError> {
        if self.revision.is_none_or(|current| revision > current) {
            return Err(GraphError::UnknownReadView(revision));
        }
        Ok(self
            .policy_history
            .range(..=revision)
            .next_back()
            .map(|(_, policy)| policy))
    }

    pub fn adjacent(
        &self,
        entity: RecordRef,
        direction: AdjacencyDirection,
        maximum: usize,
    ) -> Result<Vec<&RelationshipRecord>, GraphError> {
        validate_scope(self.scope, entity)?;
        if maximum > MAX_TRAVERSAL_RESULTS {
            return Err(GraphError::ResourceLimit);
        }
        let records = self.adjacent_candidates(entity, direction)?;
        if records.len() > maximum {
            return Err(GraphError::ResultLimit {
                actual: records.len(),
                maximum,
            });
        }
        Ok(records)
    }

    pub fn supported_by(
        &self,
        evidence: RecordRef,
        maximum: usize,
    ) -> Result<Vec<&Record>, GraphError> {
        validate_scope(self.scope, evidence)?;
        if maximum > MAX_TRAVERSAL_RESULTS {
            return Err(GraphError::ResourceLimit);
        }
        let records = self.supported_candidates(evidence)?;
        if records.len() > maximum {
            return Err(GraphError::ResultLimit {
                actual: records.len(),
                maximum,
            });
        }
        Ok(records)
    }

    pub(crate) fn adjacent_candidates(
        &self,
        entity: RecordRef,
        direction: AdjacencyDirection,
    ) -> Result<Vec<&RelationshipRecord>, GraphError> {
        validate_scope(self.scope, entity)?;
        let mut ids = BTreeSet::new();
        let mut visits = 0_usize;
        for candidates in [
            matches!(
                direction,
                AdjacencyDirection::Outgoing | AdjacencyDirection::Either
            )
            .then(|| self.outgoing.get(&entity))
            .flatten(),
            matches!(
                direction,
                AdjacencyDirection::Incoming | AdjacencyDirection::Either
            )
            .then(|| self.incoming.get(&entity))
            .flatten(),
        ]
        .into_iter()
        .flatten()
        {
            for id in candidates {
                visits = visits.saturating_add(1);
                if visits > MAX_TRAVERSAL_VISITS {
                    return Err(GraphError::ResourceLimit);
                }
                ids.insert(*id);
            }
        }
        ids.into_iter()
            .map(|id| match self.records.get(&id) {
                Some(Record::Relationship(relationship)) => Ok(relationship),
                _ => Err(GraphError::IndexCorrupt(id)),
            })
            .collect()
    }

    pub(crate) fn supported_candidates(
        &self,
        evidence: RecordRef,
    ) -> Result<Vec<&Record>, GraphError> {
        validate_scope(self.scope, evidence)?;
        let Some(ids) = self.provenance.get(&evidence) else {
            return Ok(Vec::new());
        };
        let mut records = Vec::new();
        for (index, id) in ids.iter().enumerate() {
            if index == MAX_TRAVERSAL_VISITS {
                return Err(GraphError::ResourceLimit);
            }
            records.push(self.records.get(id).ok_or(GraphError::IndexCorrupt(*id))?);
        }
        Ok(records)
    }

    /// Recompute every derived index and prove it matches the published snapshot.
    pub fn validate_derived_indexes(&self) -> Result<(), GraphError> {
        let mut rebuilt = self.clone();
        rebuild_indexes(&mut rebuilt);
        if rebuilt.outgoing == self.outgoing
            && rebuilt.incoming == self.incoming
            && rebuilt.provenance == self.provenance
        {
            Ok(())
        } else {
            Err(GraphError::DerivedIndexMismatch)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphState {
    snapshot: GraphSnapshot,
}

impl GraphState {
    #[must_use]
    pub fn new(scope: NamespaceRef) -> Self {
        Self {
            snapshot: GraphSnapshot {
                scope,
                revision: None,
                records: BTreeMap::new(),
                history: BTreeMap::new(),
                outgoing: BTreeMap::new(),
                incoming: BTreeMap::new(),
                provenance: BTreeMap::new(),
                policy: None,
                policy_history: BTreeMap::new(),
            },
        }
    }

    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.snapshot.scope
    }

    #[must_use]
    pub const fn current_revision(&self) -> Option<CommitRevision> {
        self.snapshot.revision
    }

    #[must_use]
    pub fn snapshot(&self) -> GraphSnapshot {
        self.snapshot.clone()
    }

    pub fn prepare_transaction(
        &self,
        transaction: &GraphTransaction,
        revision: CommitRevision,
    ) -> Result<PreparedGraph, GraphError> {
        if transaction.scope() != self.scope() {
            return Err(GraphError::TransactionScopeMismatch);
        }
        let expected_revision = match self.snapshot.revision {
            Some(current) => current
                .checked_next()
                .map_err(|_| GraphError::RevisionExhausted)?,
            None => CommitRevision::FIRST,
        };
        if revision != expected_revision {
            return Err(GraphError::RevisionMismatch {
                expected: expected_revision,
                actual: revision,
            });
        }
        validate_request_limits(transaction)?;
        for operation in transaction.operations() {
            validate_scope(self.scope(), operation.target())?;
            validate_expected(&self.snapshot, operation.target(), operation.expected())?;
            validate_correction_precondition(&self.snapshot, operation)?;
        }
        validate_policy_mutation(
            self.scope(),
            self.snapshot.policy.as_ref(),
            transaction.policy_mutation(),
        )?;

        let mut next = self.snapshot.clone();
        let mut affected = BTreeSet::new();
        for operation in transaction.operations() {
            apply_operation(&mut next.records, operation, revision, &mut affected)?;
        }
        if let Some(mutation) = transaction.policy_mutation() {
            let policy = match mutation {
                DurablePolicyMutation::Install { policy }
                | DurablePolicyMutation::Replace { policy, .. } => policy.clone(),
            };
            next.policy = Some(policy.clone());
            next.policy_history.insert(revision, policy);
        }
        validate_state(next.scope, &next.records)?;
        next.revision = Some(revision);
        rebuild_indexes(&mut next);
        for id in &affected {
            let record = next
                .records
                .get(id)
                .cloned()
                .ok_or(GraphError::IndexCorrupt(*id))?;
            next.history.entry(*id).or_default().push(record);
        }
        Ok(PreparedGraph {
            snapshot: next,
            affected: affected.into_iter().collect(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedGraph {
    snapshot: GraphSnapshot,
    affected: Vec<RecordRef>,
}

impl TransactionState for GraphState {
    type Prepared = PreparedGraph;
    type Snapshot = GraphSnapshot;

    fn prepare(
        &self,
        canonical_request: &[u8],
        blob_inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<Self::Prepared, ApplyError> {
        if blob_inventory.is_some() {
            return Err(ApplyError::InvalidRequest);
        }
        let transaction =
            decode_transaction(canonical_request).map_err(|_| ApplyError::InvalidRequest)?;
        if transaction.scope() != self.scope() {
            return Err(ApplyError::InvalidRequest);
        }
        self.prepare_transaction(&transaction, revision)
            .map_err(GraphError::into_apply_error)
    }

    fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
        let revision = prepared
            .snapshot
            .revision
            .expect("prepared graph always has a revision");
        let mut digest = Sha256::new();
        digest.update(b"USTE-GRAPH-RESULT-V1\0");
        digest.update(revision.get().to_be_bytes());
        digest.update((prepared.affected.len() as u64).to_be_bytes());
        for record in &prepared.affected {
            let record = prepared
                .snapshot
                .records
                .get(record)
                .expect("affected graph record exists in prepared snapshot");
            let encoded = encode_result_record(record)
                .expect("validated prepared graph record has a canonical encoding");
            digest.update((encoded.len() as u64).to_be_bytes());
            digest.update(encoded);
        }
        let policy = encode_result_policy(prepared.snapshot.policy.as_ref())
            .expect("validated prepared graph policy has a canonical encoding");
        digest.update((policy.len() as u64).to_be_bytes());
        digest.update(policy);
        digest.finalize().into()
    }

    fn publish(&mut self, prepared: Self::Prepared) {
        self.snapshot = prepared.snapshot;
    }

    fn snapshot(&self) -> Self::Snapshot {
        self.snapshot.clone()
    }
}

impl AuthorizedTransactionState for GraphState {
    const REQUIRES_DURABLE_POLICY: bool = true;

    fn authorization_requirements(
        canonical_request: &[u8],
        blob_inventory: Option<&BlobInventory>,
    ) -> Result<AuthorizationRequirements, ApplyError> {
        if blob_inventory.is_some() {
            return Err(ApplyError::InvalidRequest);
        }
        let transaction =
            decode_transaction(canonical_request).map_err(|_| ApplyError::InvalidRequest)?;
        validate_request_limits(&transaction).map_err(GraphError::into_apply_error)?;
        let mut requirements = BTreeSet::new();
        for operation in transaction.operations() {
            requirements.insert((Action::Commit, operation.target()));
            requirements.insert((Action::ReadRecord, operation.target()));
            collect_operation_references(operation, &mut requirements);
        }
        let record_requirements =
            requirements
                .into_iter()
                .map(|(action, record)| AuthorizationRequirement {
                    action,
                    target: Target::Record(record),
                });
        let policy_requirement = transaction
            .policy_mutation()
            .map(|_| AuthorizationRequirement {
                action: Action::ManagePolicy,
                target: Target::Namespace(transaction.scope()),
            });
        AuthorizationRequirements::new(record_requirements.chain(policy_requirement))
            .map_err(|_| ApplyError::ResourceLimit)
    }

    fn durable_namespace_policy(snapshot: &Self::Snapshot) -> Option<NamespacePolicy> {
        snapshot.policy.clone()
    }

    fn durable_policy_change(
        canonical_request: &[u8],
    ) -> Result<Option<DurablePolicyChange>, ApplyError> {
        let transaction =
            decode_transaction(canonical_request).map_err(|_| ApplyError::InvalidRequest)?;
        match transaction.policy_mutation() {
            Some(DurablePolicyMutation::Install { .. }) => Err(ApplyError::InvalidRequest),
            Some(DurablePolicyMutation::Replace { expected, policy }) => {
                Ok(Some(DurablePolicyChange {
                    expected: *expected,
                    next: policy.clone(),
                }))
            }
            _ => Ok(None),
        }
    }
}

fn collect_operation_references(operation: &Operation, output: &mut BTreeSet<(Action, RecordRef)>) {
    collect_expected(operation.expected(), output);
    match operation {
        Operation::Create { record, .. } => collect_new_record(record, output),
        Operation::ReplaceEntity { properties, .. } => collect_value(properties, output),
        Operation::ActOnAssertion {
            correction,
            correction_expected,
            ..
        } => {
            if let Some(correction) = correction {
                output.insert((Action::Commit, correction.id));
                output.insert((Action::ReadRecord, correction.id));
                collect_assertion(correction, output);
            }
            if let Some(expected) = correction_expected {
                collect_expected(expected, output);
            }
        }
        Operation::ActOnRelationship {
            correction,
            correction_expected,
            ..
        } => {
            if let Some(correction) = correction {
                output.insert((Action::Commit, correction.id));
                output.insert((Action::ReadRecord, correction.id));
                collect_relationship(correction, output);
            }
            if let Some(expected) = correction_expected {
                collect_expected(expected, output);
            }
        }
        Operation::DeleteEntity { affected, .. } => {
            output.extend(
                affected
                    .iter()
                    .copied()
                    .flat_map(|record| [(Action::ReadRecord, record), (Action::Commit, record)]),
            );
        }
    }
}

fn collect_expected(expected: &Expected, output: &mut BTreeSet<(Action, RecordRef)>) {
    if let Expected::ReadView { predicate, .. } = expected {
        let record = match predicate {
            Predicate::RecordAbsent(record)
            | Predicate::RecordVisible(record)
            | Predicate::RecordVersion { record, .. } => *record,
        };
        output.insert((Action::ReadRecord, record));
        output.insert((Action::ReadHistory, record));
    }
}

fn collect_new_record(record: &NewRecord, output: &mut BTreeSet<(Action, RecordRef)>) {
    match record {
        NewRecord::Entity(entity) => collect_value(&entity.properties, output),
        NewRecord::Evidence(_) => {}
        NewRecord::Assertion(assertion) => collect_assertion(assertion, output),
        NewRecord::Relationship(relationship) => collect_relationship(relationship, output),
    }
}

fn collect_assertion(assertion: &NewAssertion, output: &mut BTreeSet<(Action, RecordRef)>) {
    output.insert((Action::ReadRecord, assertion.subject));
    output.extend(
        assertion
            .evidence
            .iter()
            .copied()
            .map(|record| (Action::ReadRecord, record)),
    );
    collect_value(&assertion.object, output);
}

fn collect_relationship(
    relationship: &NewRelationship,
    output: &mut BTreeSet<(Action, RecordRef)>,
) {
    output.insert((Action::ReadRecord, relationship.from));
    output.insert((Action::ReadRecord, relationship.to));
    output.extend(
        relationship
            .evidence
            .iter()
            .copied()
            .map(|record| (Action::ReadRecord, record)),
    );
    collect_value(&relationship.properties, output);
}

fn collect_value(value: &Value, output: &mut BTreeSet<(Action, RecordRef)>) {
    match value {
        Value::RecordRef(record) => {
            output.insert((Action::ReadRecord, *record));
        }
        Value::List(values) => {
            for value in values.as_slice() {
                collect_value(value, output);
            }
        }
        Value::Map(values) => {
            for (_, value) in values.as_slice() {
                collect_value(value, output);
            }
        }
        _ => {}
    }
}

fn validate_request_limits(transaction: &GraphTransaction) -> Result<(), GraphError> {
    let count = transaction.operations().len();
    if count == 0 && transaction.policy_mutation().is_none() {
        return Err(GraphError::EmptyTransaction);
    }
    if count > MAX_TRANSACTION_OPERATIONS {
        return Err(GraphError::TooManyOperations {
            actual: count,
            maximum: MAX_TRANSACTION_OPERATIONS,
        });
    }
    let references = transaction
        .operations()
        .iter()
        .fold(0_usize, |count, operation| {
            count.saturating_add(operation_reference_count(operation))
        });
    if references > MAX_TRANSACTION_REFERENCES {
        return Err(GraphError::TooManyReferences {
            actual: references,
            maximum: MAX_TRANSACTION_REFERENCES,
        });
    }
    Ok(())
}

fn validate_policy_mutation(
    scope: NamespaceRef,
    current: Option<&NamespacePolicy>,
    mutation: Option<&DurablePolicyMutation>,
) -> Result<(), GraphError> {
    let Some(mutation) = mutation else {
        return Ok(());
    };
    let next = match mutation {
        DurablePolicyMutation::Install { policy } => {
            if current.is_some() {
                return Err(GraphError::PolicyConflict);
            }
            policy
        }
        DurablePolicyMutation::Replace { expected, policy } => {
            let Some(current) = current else {
                return Err(GraphError::PolicyConflict);
            };
            if current.version() != *expected || policy.version() <= *expected {
                return Err(GraphError::PolicyConflict);
            }
            policy
        }
    };
    if next.scope() != scope {
        return Err(GraphError::TransactionScopeMismatch);
    }
    Ok(())
}

fn operation_reference_count(operation: &Operation) -> usize {
    let expected = expected_reference_count(operation.expected());
    match operation {
        Operation::Create { record, .. } => {
            expected.saturating_add(new_record_reference_count(record))
        }
        Operation::ReplaceEntity { properties, .. } => expected
            .saturating_add(1)
            .saturating_add(value_reference_count(properties)),
        Operation::ActOnAssertion {
            correction,
            correction_expected,
            ..
        } => expected
            .saturating_add(1)
            .saturating_add(
                correction_expected
                    .as_ref()
                    .map_or(0, expected_reference_count),
            )
            .saturating_add(correction.as_ref().map_or(0, new_assertion_reference_count)),
        Operation::ActOnRelationship {
            correction,
            correction_expected,
            ..
        } => expected
            .saturating_add(1)
            .saturating_add(
                correction_expected
                    .as_ref()
                    .map_or(0, expected_reference_count),
            )
            .saturating_add(
                correction
                    .as_ref()
                    .map_or(0, new_relationship_reference_count),
            ),
        Operation::DeleteEntity { affected, .. } => {
            expected.saturating_add(1).saturating_add(affected.len())
        }
    }
}

fn expected_reference_count(expected: &Expected) -> usize {
    usize::from(matches!(expected, Expected::ReadView { .. }))
}

fn new_record_reference_count(record: &NewRecord) -> usize {
    match record {
        NewRecord::Entity(entity) => {
            1_usize.saturating_add(value_reference_count(&entity.properties))
        }
        NewRecord::Evidence(_) => 1,
        NewRecord::Assertion(assertion) => new_assertion_reference_count(assertion),
        NewRecord::Relationship(relationship) => new_relationship_reference_count(relationship),
    }
}

fn new_assertion_reference_count(assertion: &NewAssertion) -> usize {
    2_usize
        .saturating_add(assertion.evidence.len())
        .saturating_add(value_reference_count(&assertion.object))
}

fn new_relationship_reference_count(relationship: &NewRelationship) -> usize {
    3_usize
        .saturating_add(relationship.evidence.len())
        .saturating_add(value_reference_count(&relationship.properties))
}

fn value_reference_count(value: &Value) -> usize {
    match value {
        Value::RecordRef(_) => 1,
        Value::List(values) => values.as_slice().iter().fold(0_usize, |count, value| {
            count.saturating_add(value_reference_count(value))
        }),
        Value::Map(values) => values.as_slice().iter().fold(0_usize, |count, (_, value)| {
            count.saturating_add(value_reference_count(value))
        }),
        _ => 0,
    }
}

fn validate_expected(
    snapshot: &GraphSnapshot,
    target: RecordRef,
    expected: &Expected,
) -> Result<(), GraphError> {
    match expected {
        Expected::Absent if snapshot.records.contains_key(&target) => {
            Err(GraphError::PreconditionFailed(target))
        }
        Expected::Absent => Ok(()),
        Expected::Version(version) => match snapshot.records.get(&target) {
            Some(record) if record.version() == *version => Ok(()),
            _ => Err(GraphError::PreconditionFailed(target)),
        },
        Expected::ReadView {
            revision,
            predicate,
        } => {
            validate_predicate_scope(snapshot.scope, predicate)?;
            if snapshot.revision.is_none_or(|current| *revision > current) {
                return Err(GraphError::UnknownReadView(*revision));
            }
            if !evaluate_predicate_at(snapshot, *revision, predicate) {
                return Err(GraphError::PredicateWasFalse(*revision));
            }
            if !evaluate_predicate_current(snapshot, predicate) {
                return Err(GraphError::PredicateChanged(*revision));
            }
            Ok(())
        }
    }
}

fn validate_correction_precondition(
    snapshot: &GraphSnapshot,
    operation: &Operation,
) -> Result<(), GraphError> {
    let (action, correction, expected) = match operation {
        Operation::ActOnAssertion {
            action,
            correction,
            correction_expected,
            ..
        } => (
            *action,
            correction.as_ref().map(|value| value.id),
            correction_expected,
        ),
        Operation::ActOnRelationship {
            action,
            correction,
            correction_expected,
            ..
        } => (
            *action,
            correction.as_ref().map(|value| value.id),
            correction_expected,
        ),
        _ => return Ok(()),
    };
    if action == AssertionAction::Correct {
        let correction = correction.ok_or(GraphError::CorrectionRequired)?;
        let expected = expected
            .as_ref()
            .ok_or(GraphError::CorrectionPreconditionRequired)?;
        validate_scope(snapshot.scope, correction)?;
        validate_expected(snapshot, correction, expected)
    } else if correction.is_some() || expected.is_some() {
        Err(GraphError::UnexpectedCorrection)
    } else {
        Ok(())
    }
}

fn evaluate_predicate_at(
    snapshot: &GraphSnapshot,
    revision: CommitRevision,
    predicate: &Predicate,
) -> bool {
    let record = match predicate {
        Predicate::RecordAbsent(record)
        | Predicate::RecordVisible(record)
        | Predicate::RecordVersion { record, .. } => *record,
    };
    let candidate = record_at(&snapshot.history, record, revision);
    match predicate {
        Predicate::RecordAbsent(_) => candidate.is_none(),
        Predicate::RecordVisible(_) => candidate.is_some_and(is_visible_record),
        Predicate::RecordVersion { version, .. } => {
            candidate.is_some_and(|record| record.version() == *version)
        }
    }
}

fn evaluate_predicate_current(snapshot: &GraphSnapshot, predicate: &Predicate) -> bool {
    let record = match predicate {
        Predicate::RecordAbsent(record)
        | Predicate::RecordVisible(record)
        | Predicate::RecordVersion { record, .. } => *record,
    };
    let candidate = snapshot.records.get(&record);
    match predicate {
        Predicate::RecordAbsent(_) => candidate.is_none(),
        Predicate::RecordVisible(_) => candidate.is_some_and(is_visible_record),
        Predicate::RecordVersion { version, .. } => {
            candidate.is_some_and(|record| record.version() == *version)
        }
    }
}

fn apply_operation(
    records: &mut BTreeMap<RecordRef, Record>,
    operation: &Operation,
    revision: CommitRevision,
    affected: &mut BTreeSet<RecordRef>,
) -> Result<(), GraphError> {
    match operation {
        Operation::Create { record, .. } => create_record(records, record, revision, affected),
        Operation::ReplaceEntity {
            target, properties, ..
        } => {
            let Some(Record::Entity(entity)) = records.get_mut(target) else {
                return Err(wrong_kind_or_missing(records, *target, "entity"));
            };
            if entity.lifecycle != EntityLifecycle::Active {
                return Err(GraphError::NotVisible(*target));
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
            records,
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
            records,
            *target,
            *action,
            correction.as_ref(),
            revision,
            affected,
        ),
        Operation::DeleteEntity {
            target,
            policy,
            affected: declared,
            ..
        } => delete_entity(records, *target, *policy, declared, revision, affected),
    }
}

fn create_record(
    records: &mut BTreeMap<RecordRef, Record>,
    new: &NewRecord,
    revision: CommitRevision,
    affected: &mut BTreeSet<RecordRef>,
) -> Result<(), GraphError> {
    let id = new.id();
    if records.contains_key(&id) {
        return Err(GraphError::AlreadyExists(id));
    }
    let record = match new {
        NewRecord::Entity(entity) => Record::Entity(EntityRecord {
            id,
            version: RecordVersion::FIRST,
            lifecycle: EntityLifecycle::Active,
            entity_type: entity.entity_type.clone(),
            schema_version: entity.schema_version,
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
                return Err(GraphError::InvalidInterval(id));
            }
            Record::Assertion(assertion_record(assertion, None, revision))
        }
        NewRecord::Relationship(relationship) => {
            if !relationship.valid_time.is_valid() {
                return Err(GraphError::InvalidInterval(id));
            }
            Record::Relationship(relationship_record(relationship, None, revision))
        }
    };
    records.insert(id, record);
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
    records: &mut BTreeMap<RecordRef, Record>,
    target: RecordRef,
    action: AssertionAction,
    correction: Option<&NewAssertion>,
    revision: CommitRevision,
    affected: &mut BTreeSet<RecordRef>,
) -> Result<(), GraphError> {
    let status = match records.get(&target) {
        Some(Record::Assertion(assertion)) => assertion.status,
        _ => return Err(wrong_kind_or_missing(records, target, "assertion")),
    };
    if action == AssertionAction::Correct {
        if status != AssertionStatus::Accepted {
            return Err(GraphError::InvalidTransition { status, action });
        }
        let correction = correction.ok_or(GraphError::CorrectionRequired)?;
        if records.contains_key(&correction.id) {
            return Err(GraphError::AlreadyExists(correction.id));
        }
        if !correction.valid_time.is_valid() {
            return Err(GraphError::InvalidInterval(correction.id));
        }
        records.insert(
            correction.id,
            Record::Assertion(assertion_record(correction, Some(target), revision)),
        );
        affected.insert(correction.id);
        return Ok(());
    }
    if correction.is_some() {
        return Err(GraphError::UnexpectedCorrection);
    }
    let next = transition(status, action)?;
    let Some(Record::Assertion(assertion)) = records.get_mut(&target) else {
        unreachable!("assertion kind checked")
    };
    assertion.status = next;
    assertion.version = next_version(assertion.version, target)?;
    assertion.modified_revision = revision;
    affected.insert(target);
    Ok(())
}

fn apply_relationship_action(
    records: &mut BTreeMap<RecordRef, Record>,
    target: RecordRef,
    action: AssertionAction,
    correction: Option<&NewRelationship>,
    revision: CommitRevision,
    affected: &mut BTreeSet<RecordRef>,
) -> Result<(), GraphError> {
    let status = match records.get(&target) {
        Some(Record::Relationship(relationship)) => relationship.status,
        _ => return Err(wrong_kind_or_missing(records, target, "relationship")),
    };
    if action == AssertionAction::Correct {
        if status != AssertionStatus::Accepted {
            return Err(GraphError::InvalidTransition { status, action });
        }
        let correction = correction.ok_or(GraphError::CorrectionRequired)?;
        if records.contains_key(&correction.id) {
            return Err(GraphError::AlreadyExists(correction.id));
        }
        if !correction.valid_time.is_valid() {
            return Err(GraphError::InvalidInterval(correction.id));
        }
        records.insert(
            correction.id,
            Record::Relationship(relationship_record(correction, Some(target), revision)),
        );
        affected.insert(correction.id);
        return Ok(());
    }
    if correction.is_some() {
        return Err(GraphError::UnexpectedCorrection);
    }
    let next = transition(status, action)?;
    let Some(Record::Relationship(relationship)) = records.get_mut(&target) else {
        unreachable!("relationship kind checked")
    };
    relationship.status = next;
    relationship.version = next_version(relationship.version, target)?;
    relationship.modified_revision = revision;
    affected.insert(target);
    Ok(())
}

fn transition(
    status: AssertionStatus,
    action: AssertionAction,
) -> Result<AssertionStatus, GraphError> {
    match (status, action) {
        (AssertionStatus::Proposed, AssertionAction::Accept) => Ok(AssertionStatus::Accepted),
        (AssertionStatus::Proposed, AssertionAction::Reject) => Ok(AssertionStatus::Rejected),
        (AssertionStatus::Accepted, AssertionAction::Dispute) => Ok(AssertionStatus::Disputed),
        (AssertionStatus::Accepted, AssertionAction::Supersede) => Ok(AssertionStatus::Superseded),
        (AssertionStatus::Accepted, AssertionAction::Retract) => Ok(AssertionStatus::Retracted),
        (AssertionStatus::Accepted, AssertionAction::Expire) => Ok(AssertionStatus::Expired),
        _ => Err(GraphError::InvalidTransition { status, action }),
    }
}

fn delete_entity(
    records: &mut BTreeMap<RecordRef, Record>,
    target: RecordRef,
    policy: DeletePolicy,
    declared: &[RecordRef],
    revision: CommitRevision,
    affected: &mut BTreeSet<RecordRef>,
) -> Result<(), GraphError> {
    match records.get(&target) {
        Some(Record::Entity(entity)) if entity.lifecycle == EntityLifecycle::Active => {}
        Some(Record::Entity(_)) => return Err(GraphError::NotVisible(target)),
        _ => return Err(wrong_kind_or_missing(records, target, "entity")),
    }
    let accepted_relationships: Vec<RecordRef> = records
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
    let accepted_assertions: Vec<RecordRef> = records
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
    let proposed = records
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
    let entity_references = records
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
    let dependents = accepted_relationships
        .len()
        .saturating_add(accepted_assertions.len())
        .saturating_add(entity_references);

    match policy {
        DeletePolicy::Reject if dependents != 0 || proposed != 0 => {
            return Err(GraphError::DeleteRestricted {
                record: target,
                dependents: dependents.saturating_add(proposed),
            });
        }
        DeletePolicy::Reject => {
            if !declared.is_empty() {
                return Err(GraphError::InvalidCascadeDeclaration);
            }
        }
        DeletePolicy::CascadeAndRetract { maximum_affected } => {
            if declared.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(GraphError::InvalidCascadeDeclaration);
            }
            if proposed != 0 {
                return Err(GraphError::ProposedClaimBlocksDelete {
                    record: target,
                    dependents: proposed,
                });
            }
            if entity_references != 0 {
                return Err(GraphError::UncascadeableReferences {
                    record: target,
                    dependents: entity_references,
                });
            }
            let maximum = maximum_affected as usize;
            if dependents > maximum {
                return Err(GraphError::CascadeLimitExceeded {
                    actual: dependents,
                    maximum,
                });
            }
            let actual: BTreeSet<RecordRef> = accepted_relationships
                .iter()
                .chain(&accepted_assertions)
                .copied()
                .collect();
            let declared_set: BTreeSet<RecordRef> = declared.iter().copied().collect();
            if declared.len() != declared_set.len() || declared_set != actual {
                return Err(GraphError::CascadeDeclarationChanged);
            }
            for id in accepted_relationships {
                let Some(Record::Relationship(record)) = records.get_mut(&id) else {
                    unreachable!("collected relationship")
                };
                record.status = AssertionStatus::Retracted;
                record.version = next_version(record.version, id)?;
                record.modified_revision = revision;
                affected.insert(id);
            }
            for id in accepted_assertions {
                let Some(Record::Assertion(record)) = records.get_mut(&id) else {
                    unreachable!("collected assertion")
                };
                record.status = AssertionStatus::Retracted;
                record.version = next_version(record.version, id)?;
                record.modified_revision = revision;
                affected.insert(id);
            }
        }
    }
    let Some(Record::Entity(entity)) = records.get_mut(&target) else {
        unreachable!("entity checked")
    };
    entity.lifecycle = EntityLifecycle::Deleted;
    entity.version = next_version(entity.version, target)?;
    entity.modified_revision = revision;
    affected.insert(target);
    Ok(())
}

fn validate_state(
    scope: NamespaceRef,
    records: &BTreeMap<RecordRef, Record>,
) -> Result<(), GraphError> {
    for (id, record) in records {
        validate_scope(scope, *id)?;
        match record {
            Record::Entity(entity) => {
                if entity.schema_version == 0 {
                    return Err(GraphError::InvalidSchemaVersion(entity.id));
                }
                validate_value_scope(scope, &entity.properties)?;
                if entity.lifecycle == EntityLifecycle::Active {
                    validate_value(records, scope, &entity.properties)?;
                }
            }
            Record::Evidence(_) => {}
            Record::Assertion(assertion) => {
                validate_scope(scope, assertion.subject)?;
                validate_value_scope(scope, &assertion.object)?;
                validate_evidence(records, scope, assertion.id, &assertion.evidence)?;
                if !assertion.valid_time.is_valid() {
                    return Err(GraphError::InvalidInterval(assertion.id));
                }
                if matches!(
                    assertion.status,
                    AssertionStatus::Proposed | AssertionStatus::Accepted
                ) {
                    require_active_entity(records, assertion.subject)?;
                    validate_value(records, scope, &assertion.object)?;
                }
                if let Some(previous) = assertion.correction_of
                    && !matches!(records.get(&previous), Some(Record::Assertion(_)))
                {
                    return Err(GraphError::ReferenceNotVisible(previous));
                }
            }
            Record::Relationship(relationship) => {
                validate_scope(scope, relationship.from)?;
                validate_scope(scope, relationship.to)?;
                validate_value_scope(scope, &relationship.properties)?;
                validate_evidence(records, scope, relationship.id, &relationship.evidence)?;
                if !relationship.valid_time.is_valid() {
                    return Err(GraphError::InvalidInterval(relationship.id));
                }
                if matches!(
                    relationship.status,
                    AssertionStatus::Proposed | AssertionStatus::Accepted
                ) {
                    require_active_entity(records, relationship.from)?;
                    require_active_entity(records, relationship.to)?;
                    validate_value(records, scope, &relationship.properties)?;
                }
                if let Some(previous) = relationship.correction_of
                    && !matches!(records.get(&previous), Some(Record::Relationship(_)))
                {
                    return Err(GraphError::ReferenceNotVisible(previous));
                }
            }
        }
    }
    Ok(())
}

fn validate_evidence(
    records: &BTreeMap<RecordRef, Record>,
    scope: NamespaceRef,
    owner: RecordRef,
    evidence: &[RecordRef],
) -> Result<(), GraphError> {
    if evidence.is_empty() {
        return Err(GraphError::EvidenceRequired(owner));
    }
    let mut unique = BTreeSet::new();
    for evidence in evidence {
        validate_scope(scope, *evidence)?;
        if !unique.insert(*evidence) {
            return Err(GraphError::DuplicateEvidence(*evidence));
        }
        if !matches!(records.get(evidence), Some(Record::Evidence(_))) {
            return Err(GraphError::MissingEvidence(*evidence));
        }
    }
    Ok(())
}

fn validate_value(
    records: &BTreeMap<RecordRef, Record>,
    scope: NamespaceRef,
    value: &Value,
) -> Result<(), GraphError> {
    match value {
        Value::RecordRef(record) => {
            validate_scope(scope, *record)?;
            if !records.get(record).is_some_and(is_visible_record) {
                return Err(GraphError::ReferenceNotVisible(*record));
            }
        }
        Value::List(values) => {
            for value in values.as_slice() {
                validate_value(records, scope, value)?;
            }
        }
        Value::Map(values) => {
            for (_, value) in values.as_slice() {
                validate_value(records, scope, value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_value_scope(scope: NamespaceRef, value: &Value) -> Result<(), GraphError> {
    match value {
        Value::RecordRef(record) => validate_scope(scope, *record)?,
        Value::List(values) => {
            for value in values.as_slice() {
                validate_value_scope(scope, value)?;
            }
        }
        Value::Map(values) => {
            for (_, value) in values.as_slice() {
                validate_value_scope(scope, value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn require_active_entity(
    records: &BTreeMap<RecordRef, Record>,
    id: RecordRef,
) -> Result<(), GraphError> {
    if matches!(records.get(&id), Some(Record::Entity(entity)) if entity.lifecycle == EntityLifecycle::Active)
    {
        Ok(())
    } else {
        Err(GraphError::ReferenceNotVisible(id))
    }
}

fn rebuild_indexes(snapshot: &mut GraphSnapshot) {
    snapshot.outgoing.clear();
    snapshot.incoming.clear();
    snapshot.provenance.clear();
    for record in snapshot.records.values() {
        match record {
            Record::Relationship(relationship) => {
                if relationship.status == AssertionStatus::Accepted {
                    snapshot
                        .outgoing
                        .entry(relationship.from)
                        .or_default()
                        .insert(relationship.id);
                    snapshot
                        .incoming
                        .entry(relationship.to)
                        .or_default()
                        .insert(relationship.id);
                }
                for evidence in &relationship.evidence {
                    snapshot
                        .provenance
                        .entry(*evidence)
                        .or_default()
                        .insert(relationship.id);
                }
            }
            Record::Assertion(assertion) => {
                for evidence in &assertion.evidence {
                    snapshot
                        .provenance
                        .entry(*evidence)
                        .or_default()
                        .insert(assertion.id);
                }
            }
            _ => {}
        }
    }
}

fn record_at(
    history: &BTreeMap<RecordRef, Vec<Record>>,
    id: RecordRef,
    revision: CommitRevision,
) -> Option<&Record> {
    history
        .get(&id)?
        .iter()
        .rev()
        .find(|record| record.modified_revision() <= revision)
}

fn validate_predicate_scope(scope: NamespaceRef, predicate: &Predicate) -> Result<(), GraphError> {
    let record = match predicate {
        Predicate::RecordAbsent(record)
        | Predicate::RecordVisible(record)
        | Predicate::RecordVersion { record, .. } => *record,
    };
    validate_scope(scope, record)
}

fn validate_scope(scope: NamespaceRef, record: RecordRef) -> Result<(), GraphError> {
    if record.database() == scope.database() && record.namespace() == scope.namespace() {
        Ok(())
    } else {
        Err(GraphError::ScopeMismatch(record))
    }
}

fn is_visible_record(record: &Record) -> bool {
    match record {
        Record::Entity(entity) => entity.lifecycle == EntityLifecycle::Active,
        _ => true,
    }
}

fn value_contains(value: &Value, target: RecordRef) -> bool {
    match value {
        Value::RecordRef(record) => *record == target,
        Value::List(values) => values
            .as_slice()
            .iter()
            .any(|value| value_contains(value, target)),
        Value::Map(values) => values
            .as_slice()
            .iter()
            .any(|(_, value)| value_contains(value, target)),
        _ => false,
    }
}

fn next_version(version: RecordVersion, record: RecordRef) -> Result<RecordVersion, GraphError> {
    version
        .checked_next()
        .map_err(|_| GraphError::RecordVersionExhausted(record))
}

fn wrong_kind_or_missing(
    records: &BTreeMap<RecordRef, Record>,
    target: RecordRef,
    expected: &'static str,
) -> GraphError {
    if records.contains_key(&target) {
        GraphError::WrongRecordKind { target, expected }
    } else {
        GraphError::NotFound(target)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphError {
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
    TransactionScopeMismatch,
    PreconditionFailed(RecordRef),
    UnknownReadView(CommitRevision),
    PredicateWasFalse(CommitRevision),
    PredicateChanged(CommitRevision),
    RevisionExhausted,
    RevisionMismatch {
        expected: CommitRevision,
        actual: CommitRevision,
    },
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
    InvalidCascadeDeclaration,
    CascadeDeclarationChanged,
    InvalidSchemaVersion(RecordRef),
    PolicyConflict,
    ResultLimit {
        actual: usize,
        maximum: usize,
    },
    IndexCorrupt(RecordRef),
    DerivedIndexMismatch,
    ResourceLimit,
}

impl GraphError {
    const fn into_apply_error(self) -> ApplyError {
        match self {
            Self::PreconditionFailed(_)
            | Self::PredicateWasFalse(_)
            | Self::PredicateChanged(_)
            | Self::AlreadyExists(_)
            | Self::NotFound(_)
            | Self::NotVisible(_)
            | Self::DeleteRestricted { .. }
            | Self::ProposedClaimBlocksDelete { .. }
            | Self::UncascadeableReferences { .. }
            | Self::CascadeLimitExceeded { .. }
            | Self::CascadeDeclarationChanged
            | Self::PolicyConflict
            | Self::RevisionMismatch { .. } => ApplyError::Conflict,
            Self::TooManyOperations { .. }
            | Self::TooManyReferences { .. }
            | Self::ResultLimit { .. }
            | Self::ResourceLimit => ApplyError::ResourceLimit,
            Self::UnknownReadView(_) => ApplyError::UnsupportedPredicate,
            _ => ApplyError::InvalidRequest,
        }
    }
}

impl fmt::Display for GraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for GraphError {}
