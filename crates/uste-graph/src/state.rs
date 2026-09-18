//! Production graph reducer with symmetric adjacency and provenance indexes.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};
use uste_policy::{
    Action, AuthorizationRequirement, AuthorizationRequirements, NamespacePolicy, PolicyVersion,
    Target,
};
use uste_storage::BlobInventory;
use uste_txn::{
    ApplyError, AuthorizedTransactionState, CheckpointState, CheckpointStateError,
    DurablePolicyChange, ExternallyPreparedTransactionState, TransactionState,
};
use uste_types::{
    CommitRevision, DatabaseId, NamespaceId, NamespaceRef, RecordId, RecordRef, Value,
};

use crate::codec::{
    decode_result_policy, decode_result_record, encode_result_policy, encode_result_record,
};
use crate::{
    AssertionAction, AssertionRecord, AssertionStatus, DeletePolicy, DurablePolicyMutation,
    EntityLifecycle, EntityRecord, EvidenceRecord, Expected, GraphTransaction, NewAssertion,
    NewRecord, NewRelationship, Operation, Predicate, Record, RecordVersion, RelationshipRecord,
    decode_transaction, encode_transaction,
};

pub const MAX_TRANSACTION_OPERATIONS: usize = 10_000;
pub const MAX_TRANSACTION_REFERENCES: usize = 100_000;
pub const MAX_TRAVERSAL_RESULTS: usize = 100_000;
pub const MAX_TRAVERSAL_VISITS: usize = 1_000_000;
pub const MAX_GRAPH_CHECKPOINT_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_GRAPH_CHECKPOINT_RECORDS: usize = 1_000_000;
pub const MAX_GRAPH_CHECKPOINT_VERSIONS: usize = 10_000_000;

const GRAPH_CHECKPOINT_MAGIC: &[u8; 8] = b"UGCP\0\x01\0\0";
const GRAPH_REDUCER_PROFILE: [u8; 32] = [
    0x7b, 0xfd, 0xb3, 0xd2, 0xd5, 0xda, 0x47, 0xb1, 0x80, 0x8d, 0x8b, 0xaf, 0x05, 0x60, 0x30, 0xc7,
    0x46, 0x61, 0x67, 0x7b, 0xdd, 0xc7, 0x50, 0x2f, 0x6f, 0x3a, 0x1f, 0xd8, 0x37, 0x23, 0xce, 0x78,
];

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
    pub(crate) records: BTreeMap<RecordRef, Record>,
    pub(crate) history: BTreeMap<RecordRef, Vec<Record>>,
    pub(crate) outgoing: BTreeMap<RecordRef, BTreeSet<RecordRef>>,
    pub(crate) incoming: BTreeMap<RecordRef, BTreeSet<RecordRef>>,
    pub(crate) provenance: BTreeMap<RecordRef, BTreeSet<RecordRef>>,
    pub(crate) reverse: BTreeMap<RecordRef, BTreeMap<RecordRef, ReverseReference>>,
    pub(crate) policy: Option<NamespacePolicy>,
    pub(crate) policy_history: BTreeMap<CommitRevision, NamespacePolicy>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReverseReference {
    pub(crate) owner_kind: u8,
    pub(crate) owner_state: u8,
    pub(crate) roles: u16,
    pub(crate) owner_version: RecordVersion,
    pub(crate) owner_revision: CommitRevision,
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
            && rebuilt.reverse == self.reverse
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
                reverse: BTreeMap::new(),
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

    /// Borrow the current immutable reducer snapshot without cloning its retained state.
    #[must_use]
    pub const fn current_snapshot(&self) -> &GraphSnapshot {
        &self.snapshot
    }

    /// Borrow the exact post-transaction record view for a plan prepared by this current state.
    ///
    /// The view overlays only changed records and allocates nothing. Callers must not mix a plan
    /// with another same-scope/same-revision reducer instance; touched before-values are rechecked
    /// defensively and the ordinary publish path retains the same exact-source contract.
    pub fn prepared_view<'a>(
        &'a self,
        prepared: &'a PreparedGraph,
    ) -> Result<PreparedGraphView<'a>, GraphError> {
        if prepared.scope != self.snapshot.scope
            || prepared.base_revision != self.snapshot.revision
            || prepared.base_policy_version
                != self.snapshot.policy.as_ref().map(NamespacePolicy::version)
            || prepared
                .changes
                .iter()
                .any(|change| self.snapshot.records.get(&change.id) != change.before.as_ref())
        {
            return Err(GraphError::PreparedStateMismatch);
        }
        Ok(PreparedGraphView {
            base: &self.snapshot,
            changes: &prepared.changes,
        })
    }

    /// Return whether a prepared delta still targets this exact reducer base.
    #[must_use]
    pub fn can_publish(&self, prepared: &PreparedGraph) -> bool {
        self.prepared_view(prepared).is_ok()
    }

    pub fn prepare_transaction(
        &self,
        transaction: &GraphTransaction,
        revision: CommitRevision,
    ) -> Result<PreparedGraph, GraphError> {
        self.prepare_transaction_with_digest(transaction, revision, None)
    }

    fn prepare_transaction_with_digest(
        &self,
        transaction: &GraphTransaction,
        revision: CommitRevision,
        request_digest: Option<[u8; 32]>,
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
        let request_digest =
            request_digest.map_or_else(|| graph_request_digest(transaction), Ok)?;
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

        let mut records = RecordOverlay::new(&self.snapshot.records, &self.snapshot.reverse);
        let mut affected = BTreeSet::new();
        let mut protected_correction_targets = BTreeSet::new();
        for operation in transaction.operations() {
            let correction_target = match operation {
                Operation::ActOnAssertion {
                    target,
                    action: AssertionAction::Correct,
                    ..
                }
                | Operation::ActOnRelationship {
                    target,
                    action: AssertionAction::Correct,
                    ..
                } => Some(*target),
                _ => None,
            };
            // Correction provenance is validated against the prior revision during restore, so a
            // target cannot also change earlier or later in this same revision.
            if let Some(target) = correction_target
                && affected.contains(&target)
            {
                return Err(GraphError::DuplicateMutation(target));
            }
            let mut operation_affected = BTreeSet::new();
            apply_operation(&mut records, operation, revision, &mut operation_affected)?;
            if let Some(duplicate) = operation_affected.iter().find(|record| {
                affected.contains(*record) || protected_correction_targets.contains(*record)
            }) {
                return Err(GraphError::DuplicateMutation(*duplicate));
            }
            affected.extend(operation_affected);
            if let Some(target) = correction_target {
                protected_correction_targets.insert(target);
            }
        }
        let policy_change = transaction
            .policy_mutation()
            .map(|mutation| match mutation {
                DurablePolicyMutation::Install { policy }
                | DurablePolicyMutation::Replace { policy, .. } => policy.clone(),
            });
        validate_changed_state(self.scope(), &records, &affected)?;
        let changes = records.into_changes();
        debug_assert_eq!(
            changes.iter().map(|change| change.id).collect::<Vec<_>>(),
            affected.into_iter().collect::<Vec<_>>()
        );
        let next_policy = policy_change.as_ref().or(self.snapshot.policy.as_ref());
        let result_digest = graph_result_digest(
            revision,
            changes.iter().map(|change| &change.after),
            next_policy,
        );
        Ok(PreparedGraph {
            scope: self.scope(),
            base_revision: self.snapshot.revision,
            base_policy_version: self.snapshot.policy.as_ref().map(NamespacePolicy::version),
            revision,
            changes,
            policy_change,
            request_digest,
            result_digest,
        })
    }
}

/// Transaction-local record view. Only records actually mutated by the request are cloned.
/// Untouched records remain borrowed from the last journal-certified state.
struct RecordOverlay<'a> {
    base: &'a BTreeMap<RecordRef, Record>,
    base_reverse: &'a BTreeMap<RecordRef, BTreeMap<RecordRef, ReverseReference>>,
    changed: BTreeMap<RecordRef, Record>,
}

impl<'a> RecordOverlay<'a> {
    fn new(
        base: &'a BTreeMap<RecordRef, Record>,
        base_reverse: &'a BTreeMap<RecordRef, BTreeMap<RecordRef, ReverseReference>>,
    ) -> Self {
        Self {
            base,
            base_reverse,
            changed: BTreeMap::new(),
        }
    }

    fn get(&self, id: &RecordRef) -> Option<&Record> {
        self.changed.get(id).or_else(|| self.base.get(id))
    }

    fn contains_key(&self, id: &RecordRef) -> bool {
        self.changed.contains_key(id) || self.base.contains_key(id)
    }

    fn get_mut(&mut self, id: &RecordRef) -> Option<&mut Record> {
        if !self.changed.contains_key(id) {
            self.changed.insert(*id, self.base.get(id)?.clone());
        }
        self.changed.get_mut(id)
    }

    fn insert(&mut self, id: RecordRef, record: Record) {
        self.changed.insert(id, record);
    }

    fn for_each_reverse_reference(
        &self,
        target: RecordRef,
        mut visitor: impl FnMut(RecordRef, ReverseReference),
    ) {
        let mut base = self
            .base_reverse
            .get(&target)
            .into_iter()
            .flat_map(|owners| owners.iter())
            .peekable();
        let mut changed = self.changed.iter().peekable();
        loop {
            let base_owner = base.peek().map(|(owner, _)| **owner);
            let changed_owner = changed.peek().map(|(owner, _)| **owner);
            match (base_owner, changed_owner) {
                (Some(base_owner), Some(changed_owner)) if base_owner < changed_owner => {
                    let (_, reference) = base.next().expect("peeked base reverse reference");
                    visitor(base_owner, *reference);
                }
                (Some(base_owner), Some(changed_owner)) if base_owner == changed_owner => {
                    let _ = base.next();
                    let (_, record) = changed.next().expect("peeked changed record");
                    if let Some(reference) = reverse_reference_for_target(record, target) {
                        visitor(changed_owner, reference);
                    }
                }
                (_, Some(changed_owner)) => {
                    let (_, record) = changed.next().expect("peeked changed record");
                    if let Some(reference) = reverse_reference_for_target(record, target) {
                        visitor(changed_owner, reference);
                    }
                }
                (Some(base_owner), None) => {
                    let (_, reference) = base.next().expect("peeked base reverse reference");
                    visitor(base_owner, *reference);
                }
                (None, None) => break,
            }
        }
    }

    fn into_changes(self) -> Vec<RecordChange> {
        self.changed
            .into_iter()
            .map(|(id, after)| RecordChange {
                id,
                before: self.base.get(&id).cloned(),
                after,
            })
            .collect()
    }
}

fn graph_result_digest<'a>(
    revision: CommitRevision,
    records: impl ExactSizeIterator<Item = &'a Record>,
    policy: Option<&NamespacePolicy>,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"USTE-GRAPH-RESULT-V1\0");
    digest.update(revision.get().to_be_bytes());
    digest.update((records.len() as u64).to_be_bytes());
    for record in records {
        let encoded = encode_result_record(record)
            .expect("validated prepared graph record has a canonical encoding");
        digest.update((encoded.len() as u64).to_be_bytes());
        digest.update(encoded);
    }
    let policy = encode_result_policy(policy)
        .expect("validated prepared graph policy has a canonical encoding");
    digest.update((policy.len() as u64).to_be_bytes());
    digest.update(policy);
    digest.finalize().into()
}

#[derive(Debug, Eq, PartialEq)]
pub struct PreparedGraph {
    pub(crate) scope: NamespaceRef,
    pub(crate) base_revision: Option<CommitRevision>,
    pub(crate) base_policy_version: Option<PolicyVersion>,
    pub(crate) revision: CommitRevision,
    pub(crate) changes: Vec<RecordChange>,
    pub(crate) policy_change: Option<NamespacePolicy>,
    request_digest: [u8; 32],
    pub(crate) result_digest: [u8; 32],
}

impl PreparedGraph {
    #[must_use]
    pub fn change_count(&self) -> usize {
        self.changes.len()
    }
}

/// Allocation-free current-record overlay for one exact prepared graph transaction.
pub struct PreparedGraphView<'a> {
    base: &'a GraphSnapshot,
    changes: &'a [RecordChange],
}

impl fmt::Debug for PreparedGraphView<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedGraphView")
            .field("scope", &"[REDACTED]")
            .field("base_revision", &self.base.revision)
            .field("change_count", &self.changes.len())
            .finish()
    }
}

impl PreparedGraphView<'_> {
    #[must_use]
    pub fn record(&self, id: RecordRef) -> Option<&Record> {
        self.changes
            .binary_search_by_key(&id, |change| change.id)
            .ok()
            .map(|index| &self.changes[index].after)
            .or_else(|| self.base.records.get(&id))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RecordChange {
    pub(crate) id: RecordRef,
    pub(crate) before: Option<Record>,
    pub(crate) after: Record,
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
        self.prepare_transaction_with_digest(
            &transaction,
            revision,
            Some(Sha256::digest(canonical_request).into()),
        )
        .map_err(GraphError::into_apply_error)
    }

    fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
        prepared.result_digest
    }

    fn publish(&mut self, prepared: Self::Prepared) {
        assert!(
            self.can_publish(&prepared),
            "prepared graph delta must publish on its exact base state"
        );
        for change in prepared.changes {
            if let Some(before) = &change.before {
                remove_index_contributions(&mut self.snapshot, before);
            }
            add_index_contributions(&mut self.snapshot, &change.after);
            self.snapshot
                .history
                .entry(change.id)
                .or_default()
                .push(change.after.clone());
            self.snapshot.records.insert(change.id, change.after);
        }
        if let Some(policy) = prepared.policy_change {
            self.snapshot.policy = Some(policy.clone());
            self.snapshot
                .policy_history
                .insert(prepared.revision, policy);
        }
        self.snapshot.revision = Some(prepared.revision);
    }

    fn snapshot(&self) -> Self::Snapshot {
        self.snapshot.clone()
    }
}

impl ExternallyPreparedTransactionState for GraphState {
    fn validate_external_prepared(
        &self,
        canonical_request: &[u8],
        blob_inventory: Option<&BlobInventory>,
        revision: CommitRevision,
        prepared: &Self::Prepared,
    ) -> Result<(), ApplyError> {
        let request_digest: [u8; 32] = Sha256::digest(canonical_request).into();
        if blob_inventory.is_some() || prepared.request_digest != request_digest {
            return Err(ApplyError::InvalidRequest);
        }
        if prepared.revision != revision || !self.can_publish(prepared) {
            return Err(ApplyError::Conflict);
        }
        Ok(())
    }
}

fn graph_request_digest(transaction: &GraphTransaction) -> Result<[u8; 32], GraphError> {
    let encoded = encode_transaction(transaction).map_err(|_| GraphError::ResourceLimit)?;
    Ok(Sha256::digest(encoded).into())
}

impl CheckpointState for GraphState {
    const REDUCER_PROFILE: [u8; 32] = GRAPH_REDUCER_PROFILE;

    fn checkpoint_scope(snapshot: &Self::Snapshot) -> NamespaceRef {
        snapshot.scope
    }

    fn checkpoint_revision(snapshot: &Self::Snapshot) -> Option<CommitRevision> {
        snapshot.revision
    }

    fn logical_state_digest(snapshot: &Self::Snapshot) -> Result<[u8; 32], CheckpointStateError> {
        digest_graph_snapshot(snapshot)
    }

    fn encode_checkpoint(snapshot: &Self::Snapshot) -> Result<Vec<u8>, CheckpointStateError> {
        encode_graph_checkpoint(snapshot)
    }

    fn encode_checkpoint_into(
        snapshot: &Self::Snapshot,
        sink: &mut dyn FnMut(&[u8]) -> Result<(), CheckpointStateError>,
    ) -> Result<(), CheckpointStateError> {
        encode_graph_checkpoint_into(snapshot, sink)
    }

    fn decode_checkpoint(
        scope: NamespaceRef,
        revision: CommitRevision,
        encoded: &[u8],
    ) -> Result<Self, CheckpointStateError> {
        decode_graph_checkpoint(scope, revision, encoded)
    }

    fn current_checkpoint_scope(&self) -> NamespaceRef {
        self.snapshot.scope
    }

    fn current_checkpoint_revision(&self) -> Option<CommitRevision> {
        self.snapshot.revision
    }

    fn current_logical_state_digest(&self) -> Result<[u8; 32], CheckpointStateError> {
        digest_graph_snapshot(&self.snapshot)
    }

    fn encode_current_checkpoint(&self) -> Result<Vec<u8>, CheckpointStateError> {
        encode_graph_checkpoint(&self.snapshot)
    }

    fn encode_current_checkpoint_into(
        &self,
        sink: &mut dyn FnMut(&[u8]) -> Result<(), CheckpointStateError>,
    ) -> Result<(), CheckpointStateError> {
        encode_graph_checkpoint_into(&self.snapshot, sink)
    }
}

fn digest_graph_snapshot(snapshot: &GraphSnapshot) -> Result<[u8; 32], CheckpointStateError> {
    let mut digest = Sha256::new();
    digest.update(b"USTE-GRAPH-LOGICAL-STATE-V1\0");
    digest.update(snapshot.scope.database().as_bytes());
    digest.update(snapshot.scope.namespace().as_bytes());
    digest.update(
        snapshot
            .revision
            .map_or(0, CommitRevision::get)
            .to_be_bytes(),
    );
    digest.update(
        u64::try_from(snapshot.records.len())
            .map_err(|_| CheckpointStateError::ResourceLimit)?
            .to_be_bytes(),
    );
    for (id, record) in &snapshot.records {
        digest.update(id.record().as_bytes());
        digest_graph_frame(
            &mut digest,
            &encode_result_record(record).map_err(checkpoint_codec_error)?,
        )?;
    }
    digest.update(
        u64::try_from(snapshot.history.len())
            .map_err(|_| CheckpointStateError::ResourceLimit)?
            .to_be_bytes(),
    );
    for (id, versions) in &snapshot.history {
        digest.update(id.record().as_bytes());
        digest.update(
            u64::try_from(versions.len())
                .map_err(|_| CheckpointStateError::ResourceLimit)?
                .to_be_bytes(),
        );
        for record in versions {
            digest_graph_frame(
                &mut digest,
                &encode_result_record(record).map_err(checkpoint_codec_error)?,
            )?;
        }
    }
    digest_graph_frame(
        &mut digest,
        &encode_result_policy(snapshot.policy.as_ref()).map_err(checkpoint_codec_error)?,
    )?;
    digest.update(
        u64::try_from(snapshot.policy_history.len())
            .map_err(|_| CheckpointStateError::ResourceLimit)?
            .to_be_bytes(),
    );
    for (revision, policy) in &snapshot.policy_history {
        digest.update(revision.get().to_be_bytes());
        digest_graph_frame(
            &mut digest,
            &encode_result_policy(Some(policy)).map_err(checkpoint_codec_error)?,
        )?;
    }
    Ok(digest.finalize().into())
}

fn digest_graph_frame(digest: &mut Sha256, value: &[u8]) -> Result<(), CheckpointStateError> {
    digest.update(
        u64::try_from(value.len())
            .map_err(|_| CheckpointStateError::ResourceLimit)?
            .to_be_bytes(),
    );
    digest.update(value);
    Ok(())
}

fn encode_graph_checkpoint(snapshot: &GraphSnapshot) -> Result<Vec<u8>, CheckpointStateError> {
    let mut output = Vec::new();
    encode_graph_checkpoint_into(snapshot, &mut |bytes| {
        output
            .try_reserve(bytes.len())
            .map_err(|_| CheckpointStateError::ResourceLimit)?;
        output.extend_from_slice(bytes);
        Ok(())
    })?;
    Ok(output)
}

fn encode_graph_checkpoint_into(
    snapshot: &GraphSnapshot,
    sink: &mut dyn FnMut(&[u8]) -> Result<(), CheckpointStateError>,
) -> Result<(), CheckpointStateError> {
    let revision = snapshot.revision.ok_or(CheckpointStateError::Invalid)?;
    if snapshot.records.len() > MAX_GRAPH_CHECKPOINT_RECORDS
        || snapshot.history.len() > MAX_GRAPH_CHECKPOINT_RECORDS
        || snapshot.policy_history.len() > MAX_GRAPH_CHECKPOINT_RECORDS
    {
        return Err(CheckpointStateError::ResourceLimit);
    }
    let version_count = snapshot
        .history
        .values()
        .try_fold(0_usize, |total, versions| total.checked_add(versions.len()))
        .ok_or(CheckpointStateError::ResourceLimit)?;
    if version_count > MAX_GRAPH_CHECKPOINT_VERSIONS {
        return Err(CheckpointStateError::ResourceLimit);
    }

    let mut output = CheckpointOutput::new(sink);
    output.extend(GRAPH_CHECKPOINT_MAGIC)?;
    output.extend(snapshot.scope.database().as_bytes())?;
    output.extend(snapshot.scope.namespace().as_bytes())?;
    output.extend(&revision.get().to_be_bytes())?;
    output.usize(snapshot.records.len())?;
    for (id, record) in &snapshot.records {
        output.extend(id.record().as_bytes())?;
        output.frame(&encode_result_record(record).map_err(checkpoint_codec_error)?)?;
    }
    output.usize(snapshot.history.len())?;
    for (id, versions) in &snapshot.history {
        output.extend(id.record().as_bytes())?;
        output.usize(versions.len())?;
        for record in versions {
            output.frame(&encode_result_record(record).map_err(checkpoint_codec_error)?)?;
        }
    }
    output
        .frame(&encode_result_policy(snapshot.policy.as_ref()).map_err(checkpoint_codec_error)?)?;
    output.usize(snapshot.policy_history.len())?;
    for (policy_revision, policy) in &snapshot.policy_history {
        output.extend(&policy_revision.get().to_be_bytes())?;
        output.frame(&encode_result_policy(Some(policy)).map_err(checkpoint_codec_error)?)?;
    }
    Ok(())
}

fn decode_graph_checkpoint(
    expected_scope: NamespaceRef,
    expected_revision: CommitRevision,
    encoded: &[u8],
) -> Result<GraphState, CheckpointStateError> {
    if encoded.len() > MAX_GRAPH_CHECKPOINT_BYTES {
        return Err(CheckpointStateError::ResourceLimit);
    }
    let mut cursor = CheckpointCursor::new(encoded);
    if cursor.read_array::<8>()? != *GRAPH_CHECKPOINT_MAGIC {
        return Err(CheckpointStateError::UnsupportedProfile);
    }
    let scope = NamespaceRef::new(
        DatabaseId::from_bytes(cursor.read_array()?),
        NamespaceId::from_bytes(cursor.read_array()?),
    );
    let revision =
        CommitRevision::new(cursor.read_u64()?).map_err(|_| CheckpointStateError::Invalid)?;
    if scope != expected_scope || revision != expected_revision {
        return Err(CheckpointStateError::Invalid);
    }

    let record_count = cursor.read_count(MAX_GRAPH_CHECKPOINT_RECORDS)?;
    let mut records = BTreeMap::new();
    let mut previous = None;
    for _ in 0..record_count {
        let id = checkpoint_record(scope, cursor.read_array()?);
        if previous.is_some_and(|prior| prior >= id) {
            return Err(CheckpointStateError::Invalid);
        }
        previous = Some(id);
        let record = decode_result_record(cursor.read_frame()?).map_err(checkpoint_codec_error)?;
        if record.id() != id || record.modified_revision() > revision {
            return Err(CheckpointStateError::Invalid);
        }
        records.insert(id, record);
    }

    let history_count = cursor.read_count(MAX_GRAPH_CHECKPOINT_RECORDS)?;
    let mut history = BTreeMap::new();
    previous = None;
    let mut total_versions = 0_usize;
    for _ in 0..history_count {
        let id = checkpoint_record(scope, cursor.read_array()?);
        if previous.is_some_and(|prior| prior >= id) {
            return Err(CheckpointStateError::Invalid);
        }
        previous = Some(id);
        let count = cursor.read_count(MAX_GRAPH_CHECKPOINT_VERSIONS)?;
        if count == 0 {
            return Err(CheckpointStateError::Invalid);
        }
        total_versions = total_versions
            .checked_add(count)
            .ok_or(CheckpointStateError::ResourceLimit)?;
        if total_versions > MAX_GRAPH_CHECKPOINT_VERSIONS {
            return Err(CheckpointStateError::ResourceLimit);
        }
        let mut versions = Vec::new();
        for index in 0..count {
            versions
                .try_reserve(1)
                .map_err(|_| CheckpointStateError::ResourceLimit)?;
            let record =
                decode_result_record(cursor.read_frame()?).map_err(checkpoint_codec_error)?;
            let expected_version = u64::try_from(index)
                .ok()
                .and_then(|value| value.checked_add(1))
                .ok_or(CheckpointStateError::ResourceLimit)?;
            if record.id() != id
                || record.version().get() != expected_version
                || record.modified_revision() > revision
                || versions.last().is_some_and(|prior: &Record| {
                    prior.modified_revision() >= record.modified_revision()
                })
            {
                return Err(CheckpointStateError::Invalid);
            }
            versions.push(record);
        }
        validate_record_history(scope, &versions)?;
        if records.get(&id) != versions.last() {
            return Err(CheckpointStateError::Invalid);
        }
        history.insert(id, versions);
    }
    let policy = decode_result_policy(cursor.read_frame()?).map_err(checkpoint_codec_error)?;
    if policy.as_ref().is_some_and(|value| value.scope() != scope) {
        return Err(CheckpointStateError::Invalid);
    }
    let policy_count = cursor.read_count(MAX_GRAPH_CHECKPOINT_RECORDS)?;
    let mut policy_history = BTreeMap::new();
    let mut previous_revision = None;
    let mut previous_version = None;
    for _ in 0..policy_count {
        let policy_revision =
            CommitRevision::new(cursor.read_u64()?).map_err(|_| CheckpointStateError::Invalid)?;
        let decoded = decode_result_policy(cursor.read_frame()?)
            .map_err(checkpoint_codec_error)?
            .ok_or(CheckpointStateError::Invalid)?;
        if policy_revision > revision
            || previous_revision.is_some_and(|value| value >= policy_revision)
            || decoded.scope() != scope
            || previous_version.is_some_and(|value| value >= decoded.version())
        {
            return Err(CheckpointStateError::Invalid);
        }
        previous_revision = Some(policy_revision);
        previous_version = Some(decoded.version());
        policy_history.insert(policy_revision, decoded);
    }
    if !cursor.is_empty() {
        return Err(CheckpointStateError::Invalid);
    }

    GraphState::from_persisted_parts(scope, revision, records, history, policy, policy_history)
}

impl GraphState {
    /// Validate complete persisted reducer families before constructing a privately staged state.
    /// Callers remain responsible for authenticating their transport and enforcing its budgets.
    pub(crate) fn from_persisted_parts(
        scope: NamespaceRef,
        revision: CommitRevision,
        records: BTreeMap<RecordRef, Record>,
        history: BTreeMap<RecordRef, Vec<Record>>,
        policy: Option<NamespacePolicy>,
        policy_history: BTreeMap<CommitRevision, NamespacePolicy>,
    ) -> Result<Self, CheckpointStateError> {
        Self::from_persisted_parts_inner(
            scope,
            revision,
            records,
            history,
            policy,
            policy_history,
            None,
        )
    }

    pub(crate) fn from_persisted_parts_with_derived_counts(
        scope: NamespaceRef,
        revision: CommitRevision,
        records: BTreeMap<RecordRef, Record>,
        history: BTreeMap<RecordRef, Vec<Record>>,
        policy: Option<NamespacePolicy>,
        policy_history: BTreeMap<CommitRevision, NamespacePolicy>,
        expected_derived_counts: [u64; 4],
    ) -> Result<Self, CheckpointStateError> {
        Self::from_persisted_parts_inner(
            scope,
            revision,
            records,
            history,
            policy,
            policy_history,
            Some(expected_derived_counts),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn from_persisted_parts_inner(
        scope: NamespaceRef,
        revision: CommitRevision,
        records: BTreeMap<RecordRef, Record>,
        history: BTreeMap<RecordRef, Vec<Record>>,
        policy: Option<NamespacePolicy>,
        policy_history: BTreeMap<CommitRevision, NamespacePolicy>,
        expected_derived_counts: Option<[u64; 4]>,
    ) -> Result<Self, CheckpointStateError> {
        if records.len() != history.len()
            || !records
                .keys()
                .zip(history.keys())
                .all(|(left, right)| left == right)
        {
            return Err(CheckpointStateError::Invalid);
        }
        if records
            .iter()
            .any(|(id, record)| record.id() != *id || record.modified_revision() > revision)
        {
            return Err(CheckpointStateError::Invalid);
        }
        for (id, versions) in &history {
            if versions.is_empty() {
                return Err(CheckpointStateError::Invalid);
            }
            for (index, record) in versions.iter().enumerate() {
                let expected_version = u64::try_from(index)
                    .ok()
                    .and_then(|value| value.checked_add(1))
                    .ok_or(CheckpointStateError::ResourceLimit)?;
                if record.id() != *id
                    || record.version().get() != expected_version
                    || record.modified_revision() > revision
                    || index > 0
                        && versions[index - 1].modified_revision() >= record.modified_revision()
                {
                    return Err(CheckpointStateError::Invalid);
                }
            }
            validate_record_history(scope, versions)?;
            if records.get(id) != versions.last() {
                return Err(CheckpointStateError::Invalid);
            }
        }
        validate_historical_reference_closure(&history)?;

        if policy.as_ref().is_some_and(|value| value.scope() != scope) {
            return Err(CheckpointStateError::Invalid);
        }
        let mut previous_version = None;
        for (policy_revision, historical) in &policy_history {
            if *policy_revision > revision
                || historical.scope() != scope
                || previous_version.is_some_and(|value| value >= historical.version())
            {
                return Err(CheckpointStateError::Invalid);
            }
            previous_version = Some(historical.version());
        }
        if policy_history.values().next_back() != policy.as_ref() {
            return Err(CheckpointStateError::Invalid);
        }

        validate_state(scope, &records).map_err(|_| CheckpointStateError::Invalid)?;
        if let Some(expected) = expected_derived_counts {
            validate_derived_entry_counts(&records, expected)?;
        }
        let mut snapshot = GraphSnapshot {
            scope,
            revision: Some(revision),
            records,
            history,
            outgoing: BTreeMap::new(),
            incoming: BTreeMap::new(),
            provenance: BTreeMap::new(),
            reverse: BTreeMap::new(),
            policy,
            policy_history,
        };
        rebuild_indexes(&mut snapshot);
        Ok(GraphState { snapshot })
    }
}

fn validate_derived_entry_counts(
    records: &BTreeMap<RecordRef, Record>,
    expected: [u64; 4],
) -> Result<(), CheckpointStateError> {
    let [
        expected_outgoing,
        expected_incoming,
        expected_provenance,
        expected_reverse,
    ] = expected;
    let mut outgoing = 0_u64;
    let mut incoming = 0_u64;
    let mut provenance = 0_u64;
    let mut reverse = 0_u64;
    for record in records.values() {
        match record {
            Record::Relationship(relationship) => {
                if relationship.status == AssertionStatus::Accepted {
                    outgoing = outgoing
                        .checked_add(1)
                        .ok_or(CheckpointStateError::ResourceLimit)?;
                    incoming = incoming
                        .checked_add(1)
                        .ok_or(CheckpointStateError::ResourceLimit)?;
                }
                provenance = provenance
                    .checked_add(
                        u64::try_from(relationship.evidence.len())
                            .map_err(|_| CheckpointStateError::ResourceLimit)?,
                    )
                    .ok_or(CheckpointStateError::ResourceLimit)?;
            }
            Record::Assertion(assertion) => {
                provenance = provenance
                    .checked_add(
                        u64::try_from(assertion.evidence.len())
                            .map_err(|_| CheckpointStateError::ResourceLimit)?,
                    )
                    .ok_or(CheckpointStateError::ResourceLimit)?;
            }
            Record::Entity(_) | Record::Evidence(_) => {}
        }
        if outgoing > expected_outgoing
            || incoming > expected_incoming
            || provenance > expected_provenance
        {
            return Err(CheckpointStateError::Invalid);
        }

        let mut owner_targets = BTreeSet::new();
        let mut overflow = false;
        visit_record_references(record, &mut |target, _| {
            if owner_targets.contains(&target) {
                return;
            }
            if reverse == expected_reverse {
                overflow = true;
                return;
            }
            owner_targets.insert(target);
            reverse += 1;
        });
        if overflow {
            return Err(CheckpointStateError::Invalid);
        }
    }
    if [outgoing, incoming, provenance, reverse] != expected {
        return Err(CheckpointStateError::Invalid);
    }
    Ok(())
}

pub(crate) fn validate_record_history(
    scope: NamespaceRef,
    versions: &[Record],
) -> Result<(), CheckpointStateError> {
    let first = versions.first().ok_or(CheckpointStateError::Invalid)?;
    validate_history_first(scope, first)?;
    for pair in versions.windows(2) {
        let [previous, next] = pair else {
            unreachable!("windows of two")
        };
        validate_history_successor(scope, previous, next)?;
    }
    Ok(())
}

pub(crate) fn validate_history_first(
    scope: NamespaceRef,
    record: &Record,
) -> Result<(), CheckpointStateError> {
    validate_historical_record_shape(scope, record)?;
    match record {
        Record::Entity(record)
            if record.lifecycle == EntityLifecycle::Active
                && record.created_revision == record.modified_revision => {}
        Record::Evidence(record) if record.version == RecordVersion::FIRST => {}
        Record::Assertion(record)
            if record.status == AssertionStatus::Proposed
                && record.recorded_revision == record.modified_revision => {}
        Record::Relationship(record)
            if record.status == AssertionStatus::Proposed
                && record.recorded_revision == record.modified_revision => {}
        _ => return Err(CheckpointStateError::Invalid),
    }
    Ok(())
}

pub(crate) fn validate_history_successor(
    scope: NamespaceRef,
    previous: &Record,
    next: &Record,
) -> Result<(), CheckpointStateError> {
    validate_historical_record_shape(scope, next)?;
    match (previous, next) {
        (Record::Entity(previous), Record::Entity(next))
            if previous.id == next.id
                && previous.entity_type == next.entity_type
                && previous.schema_version == next.schema_version
                && previous.created_revision == next.created_revision
                && previous.lifecycle == EntityLifecycle::Active
                && matches!(
                    next.lifecycle,
                    EntityLifecycle::Active | EntityLifecycle::Deleted
                )
                && (next.lifecycle == EntityLifecycle::Active
                    || previous.properties == next.properties) => {}
        (Record::Assertion(previous), Record::Assertion(next))
            if previous.id == next.id
                && previous.subject == next.subject
                && previous.predicate == next.predicate
                && previous.object == next.object
                && previous.evidence == next.evidence
                && previous.valid_time == next.valid_time
                && previous.correction_of == next.correction_of
                && previous.recorded_revision == next.recorded_revision
                && valid_checkpoint_status_transition(previous.status, next.status) => {}
        (Record::Relationship(previous), Record::Relationship(next))
            if previous.id == next.id
                && previous.from == next.from
                && previous.to == next.to
                && previous.relationship_type == next.relationship_type
                && previous.properties == next.properties
                && previous.evidence == next.evidence
                && previous.valid_time == next.valid_time
                && previous.correction_of == next.correction_of
                && previous.recorded_revision == next.recorded_revision
                && valid_checkpoint_status_transition(previous.status, next.status) => {}
        _ => return Err(CheckpointStateError::Invalid),
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RequiredRecordKind {
    Visible,
    ActiveEntity,
    Evidence,
    Assertion,
    Relationship,
    AcceptedAssertion,
    AcceptedRelationship,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReferenceRequirement {
    pub target: RecordRef,
    pub revision: CommitRevision,
    pub kind: RequiredRecordKind,
}

pub(crate) enum ReferenceRequirementVisitError<E> {
    State(CheckpointStateError),
    Visitor(E),
}

pub(crate) fn visit_history_first_reference_requirements<E>(
    record: &Record,
    visitor: &mut impl FnMut(ReferenceRequirement) -> Result<(), E>,
) -> Result<(), ReferenceRequirementVisitError<E>> {
    match record {
        Record::Entity(entity) => visit_value_reference_requirements(
            &entity.properties,
            entity.modified_revision,
            RequiredRecordKind::Visible,
            visitor,
        )?,
        Record::Evidence(_) => {}
        Record::Assertion(assertion) => {
            emit_reference_requirement(
                ReferenceRequirement {
                    target: assertion.subject,
                    revision: assertion.recorded_revision,
                    kind: RequiredRecordKind::ActiveEntity,
                },
                visitor,
            )?;
            visit_value_reference_requirements(
                &assertion.object,
                assertion.recorded_revision,
                RequiredRecordKind::Visible,
                visitor,
            )?;
            visit_evidence_requirements(&assertion.evidence, assertion.recorded_revision, visitor)?;
            if let Some(target) = assertion.correction_of {
                let revision = preceding_revision(assertion.recorded_revision)
                    .map_err(ReferenceRequirementVisitError::State)?;
                emit_reference_requirement(
                    ReferenceRequirement {
                        target,
                        revision,
                        kind: RequiredRecordKind::AcceptedAssertion,
                    },
                    visitor,
                )?;
            }
        }
        Record::Relationship(relationship) => {
            for target in [relationship.from, relationship.to] {
                emit_reference_requirement(
                    ReferenceRequirement {
                        target,
                        revision: relationship.recorded_revision,
                        kind: RequiredRecordKind::ActiveEntity,
                    },
                    visitor,
                )?;
            }
            visit_value_reference_requirements(
                &relationship.properties,
                relationship.recorded_revision,
                RequiredRecordKind::Visible,
                visitor,
            )?;
            visit_evidence_requirements(
                &relationship.evidence,
                relationship.recorded_revision,
                visitor,
            )?;
            if let Some(target) = relationship.correction_of {
                let revision = preceding_revision(relationship.recorded_revision)
                    .map_err(ReferenceRequirementVisitError::State)?;
                emit_reference_requirement(
                    ReferenceRequirement {
                        target,
                        revision,
                        kind: RequiredRecordKind::AcceptedRelationship,
                    },
                    visitor,
                )?;
            }
        }
    }
    Ok(())
}

pub(crate) fn visit_history_successor_reference_requirements<E>(
    record: &Record,
    visitor: &mut impl FnMut(ReferenceRequirement) -> Result<(), E>,
) -> Result<(), ReferenceRequirementVisitError<E>> {
    if let Record::Entity(entity) = record
        && entity.lifecycle == EntityLifecycle::Active
    {
        visit_value_reference_requirements(
            &entity.properties,
            entity.modified_revision,
            RequiredRecordKind::Visible,
            visitor,
        )?;
    }
    Ok(())
}

pub(crate) fn visit_current_reference_requirements<E>(
    record: &Record,
    revision: CommitRevision,
    visitor: &mut impl FnMut(ReferenceRequirement) -> Result<(), E>,
) -> Result<(), ReferenceRequirementVisitError<E>> {
    match record {
        Record::Entity(entity) if entity.lifecycle == EntityLifecycle::Active => {
            visit_value_reference_requirements(
                &entity.properties,
                revision,
                RequiredRecordKind::Visible,
                visitor,
            )?;
        }
        Record::Entity(_) | Record::Evidence(_) => {}
        Record::Assertion(assertion) => {
            visit_evidence_requirements(&assertion.evidence, revision, visitor)?;
            if matches!(
                assertion.status,
                AssertionStatus::Proposed | AssertionStatus::Accepted
            ) {
                emit_reference_requirement(
                    ReferenceRequirement {
                        target: assertion.subject,
                        revision,
                        kind: RequiredRecordKind::ActiveEntity,
                    },
                    visitor,
                )?;
                visit_value_reference_requirements(
                    &assertion.object,
                    revision,
                    RequiredRecordKind::Visible,
                    visitor,
                )?;
            }
            if let Some(target) = assertion.correction_of {
                emit_reference_requirement(
                    ReferenceRequirement {
                        target,
                        revision,
                        kind: RequiredRecordKind::Assertion,
                    },
                    visitor,
                )?;
            }
        }
        Record::Relationship(relationship) => {
            visit_evidence_requirements(&relationship.evidence, revision, visitor)?;
            if matches!(
                relationship.status,
                AssertionStatus::Proposed | AssertionStatus::Accepted
            ) {
                for target in [relationship.from, relationship.to] {
                    emit_reference_requirement(
                        ReferenceRequirement {
                            target,
                            revision,
                            kind: RequiredRecordKind::ActiveEntity,
                        },
                        visitor,
                    )?;
                }
                visit_value_reference_requirements(
                    &relationship.properties,
                    revision,
                    RequiredRecordKind::Visible,
                    visitor,
                )?;
            }
            if let Some(target) = relationship.correction_of {
                emit_reference_requirement(
                    ReferenceRequirement {
                        target,
                        revision,
                        kind: RequiredRecordKind::Relationship,
                    },
                    visitor,
                )?;
            }
        }
    }
    Ok(())
}

pub(crate) fn reference_requirement_matches(
    requirement: ReferenceRequirement,
    record: Option<&Record>,
) -> bool {
    match (requirement.kind, record) {
        (RequiredRecordKind::Visible, Some(record)) => is_visible_record(record),
        (RequiredRecordKind::ActiveEntity, Some(Record::Entity(entity))) => {
            entity.lifecycle == EntityLifecycle::Active
        }
        (RequiredRecordKind::Evidence, Some(Record::Evidence(_)))
        | (RequiredRecordKind::Assertion, Some(Record::Assertion(_)))
        | (RequiredRecordKind::Relationship, Some(Record::Relationship(_))) => true,
        (RequiredRecordKind::AcceptedAssertion, Some(Record::Assertion(assertion))) => {
            assertion.status == AssertionStatus::Accepted
        }
        (RequiredRecordKind::AcceptedRelationship, Some(Record::Relationship(relationship))) => {
            relationship.status == AssertionStatus::Accepted
        }
        _ => false,
    }
}

fn emit_reference_requirement<E>(
    requirement: ReferenceRequirement,
    visitor: &mut impl FnMut(ReferenceRequirement) -> Result<(), E>,
) -> Result<(), ReferenceRequirementVisitError<E>> {
    visitor(requirement).map_err(ReferenceRequirementVisitError::Visitor)
}

fn visit_evidence_requirements<E>(
    evidence: &[RecordRef],
    revision: CommitRevision,
    visitor: &mut impl FnMut(ReferenceRequirement) -> Result<(), E>,
) -> Result<(), ReferenceRequirementVisitError<E>> {
    for target in evidence {
        emit_reference_requirement(
            ReferenceRequirement {
                target: *target,
                revision,
                kind: RequiredRecordKind::Evidence,
            },
            visitor,
        )?;
    }
    Ok(())
}

fn visit_value_reference_requirements<E>(
    value: &Value,
    revision: CommitRevision,
    kind: RequiredRecordKind,
    visitor: &mut impl FnMut(ReferenceRequirement) -> Result<(), E>,
) -> Result<(), ReferenceRequirementVisitError<E>> {
    match value {
        Value::RecordRef(target) => emit_reference_requirement(
            ReferenceRequirement {
                target: *target,
                revision,
                kind,
            },
            visitor,
        )?,
        Value::List(values) => {
            for value in values.as_slice() {
                visit_value_reference_requirements(value, revision, kind, visitor)?;
            }
        }
        Value::Map(values) => {
            for (_, value) in values.as_slice() {
                visit_value_reference_requirements(value, revision, kind, visitor)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn preceding_revision(revision: CommitRevision) -> Result<CommitRevision, CheckpointStateError> {
    revision
        .get()
        .checked_sub(1)
        .and_then(|value| CommitRevision::new(value).ok())
        .ok_or(CheckpointStateError::Invalid)
}

fn validate_historical_reference_closure(
    history: &BTreeMap<RecordRef, Vec<Record>>,
) -> Result<(), CheckpointStateError> {
    for versions in history.values() {
        let first = versions.first().ok_or(CheckpointStateError::Invalid)?;
        visit_history_first_reference_requirements(first, &mut |requirement| {
            validate_reference_requirement(history, requirement)
        })
        .map_err(flatten_checkpoint_requirement_error)?;
        for record in versions.iter().skip(1) {
            visit_history_successor_reference_requirements(record, &mut |requirement| {
                validate_reference_requirement(history, requirement)
            })
            .map_err(flatten_checkpoint_requirement_error)?;
        }
    }
    Ok(())
}

fn validate_reference_requirement(
    history: &BTreeMap<RecordRef, Vec<Record>>,
    requirement: ReferenceRequirement,
) -> Result<(), CheckpointStateError> {
    if !reference_requirement_matches(
        requirement,
        record_at(history, requirement.target, requirement.revision),
    ) {
        return Err(CheckpointStateError::Invalid);
    }
    Ok(())
}

fn flatten_checkpoint_requirement_error(
    error: ReferenceRequirementVisitError<CheckpointStateError>,
) -> CheckpointStateError {
    match error {
        ReferenceRequirementVisitError::State(error)
        | ReferenceRequirementVisitError::Visitor(error) => error,
    }
}

fn validate_historical_record_shape(
    scope: NamespaceRef,
    record: &Record,
) -> Result<(), CheckpointStateError> {
    validate_scope(scope, record.id()).map_err(|_| CheckpointStateError::Invalid)?;
    match record {
        Record::Entity(entity) => {
            if entity.schema_version == 0 || entity.created_revision > entity.modified_revision {
                return Err(CheckpointStateError::Invalid);
            }
            validate_value_scope(scope, &entity.properties)
                .map_err(|_| CheckpointStateError::Invalid)?;
        }
        Record::Evidence(_) => {}
        Record::Assertion(assertion) => {
            if assertion.recorded_revision > assertion.modified_revision
                || assertion.evidence.is_empty()
                || !assertion.valid_time.is_valid()
            {
                return Err(CheckpointStateError::Invalid);
            }
            validate_scope(scope, assertion.subject).map_err(|_| CheckpointStateError::Invalid)?;
            validate_value_scope(scope, &assertion.object)
                .map_err(|_| CheckpointStateError::Invalid)?;
            validate_historical_references(scope, &assertion.evidence)?;
            if let Some(correction) = assertion.correction_of {
                validate_scope(scope, correction).map_err(|_| CheckpointStateError::Invalid)?;
            }
        }
        Record::Relationship(relationship) => {
            if relationship.recorded_revision > relationship.modified_revision
                || relationship.evidence.is_empty()
                || !relationship.valid_time.is_valid()
            {
                return Err(CheckpointStateError::Invalid);
            }
            validate_scope(scope, relationship.from).map_err(|_| CheckpointStateError::Invalid)?;
            validate_scope(scope, relationship.to).map_err(|_| CheckpointStateError::Invalid)?;
            validate_value_scope(scope, &relationship.properties)
                .map_err(|_| CheckpointStateError::Invalid)?;
            validate_historical_references(scope, &relationship.evidence)?;
            if let Some(correction) = relationship.correction_of {
                validate_scope(scope, correction).map_err(|_| CheckpointStateError::Invalid)?;
            }
        }
    }
    Ok(())
}

fn validate_historical_references(
    scope: NamespaceRef,
    references: &[RecordRef],
) -> Result<(), CheckpointStateError> {
    let mut unique = BTreeSet::new();
    for reference in references {
        validate_scope(scope, *reference).map_err(|_| CheckpointStateError::Invalid)?;
        if !unique.insert(*reference) {
            return Err(CheckpointStateError::Invalid);
        }
    }
    Ok(())
}

const fn valid_checkpoint_status_transition(
    previous: AssertionStatus,
    next: AssertionStatus,
) -> bool {
    matches!(
        (previous, next),
        (AssertionStatus::Proposed, AssertionStatus::Accepted)
            | (AssertionStatus::Proposed, AssertionStatus::Rejected)
            | (AssertionStatus::Accepted, AssertionStatus::Disputed)
            | (AssertionStatus::Accepted, AssertionStatus::Superseded)
            | (AssertionStatus::Accepted, AssertionStatus::Retracted)
            | (AssertionStatus::Accepted, AssertionStatus::Expired)
    )
}

fn checkpoint_codec_error(error: crate::GraphCodecError) -> CheckpointStateError {
    if matches!(error, crate::GraphCodecError::ResourceLimit) {
        CheckpointStateError::ResourceLimit
    } else {
        CheckpointStateError::Invalid
    }
}

fn checkpoint_record(scope: NamespaceRef, record: [u8; 16]) -> RecordRef {
    RecordRef::new(
        scope.database(),
        scope.namespace(),
        RecordId::from_bytes(record),
    )
}

struct CheckpointOutput<'a> {
    sink: &'a mut dyn FnMut(&[u8]) -> Result<(), CheckpointStateError>,
    written: usize,
}

impl<'a> CheckpointOutput<'a> {
    fn new(sink: &'a mut dyn FnMut(&[u8]) -> Result<(), CheckpointStateError>) -> Self {
        Self { sink, written: 0 }
    }

    fn usize(&mut self, value: usize) -> Result<(), CheckpointStateError> {
        let value = u64::try_from(value).map_err(|_| CheckpointStateError::ResourceLimit)?;
        self.extend(&value.to_be_bytes())
    }

    fn frame(&mut self, value: &[u8]) -> Result<(), CheckpointStateError> {
        self.usize(value.len())?;
        self.extend(value)
    }

    fn extend(&mut self, value: &[u8]) -> Result<(), CheckpointStateError> {
        let next = self
            .written
            .checked_add(value.len())
            .ok_or(CheckpointStateError::ResourceLimit)?;
        if next > MAX_GRAPH_CHECKPOINT_BYTES {
            return Err(CheckpointStateError::ResourceLimit);
        }
        (self.sink)(value)?;
        self.written = next;
        Ok(())
    }
}

struct CheckpointCursor<'a> {
    remaining: &'a [u8],
}

impl<'a> CheckpointCursor<'a> {
    const fn new(encoded: &'a [u8]) -> Self {
        Self { remaining: encoded }
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], CheckpointStateError> {
        self.read(N)?
            .try_into()
            .map_err(|_| CheckpointStateError::Invalid)
    }

    fn read_u64(&mut self) -> Result<u64, CheckpointStateError> {
        Ok(u64::from_be_bytes(self.read_array()?))
    }

    fn read_count(&mut self, maximum: usize) -> Result<usize, CheckpointStateError> {
        let count =
            usize::try_from(self.read_u64()?).map_err(|_| CheckpointStateError::ResourceLimit)?;
        if count > maximum {
            return Err(CheckpointStateError::ResourceLimit);
        }
        Ok(count)
    }

    fn read_frame(&mut self) -> Result<&'a [u8], CheckpointStateError> {
        let length =
            usize::try_from(self.read_u64()?).map_err(|_| CheckpointStateError::ResourceLimit)?;
        self.read(length)
    }

    fn read(&mut self, length: usize) -> Result<&'a [u8], CheckpointStateError> {
        let (value, remaining) = self
            .remaining
            .split_at_checked(length)
            .ok_or(CheckpointStateError::Invalid)?;
        self.remaining = remaining;
        Ok(value)
    }

    const fn is_empty(&self) -> bool {
        self.remaining.is_empty()
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

pub(crate) fn validate_request_limits(transaction: &GraphTransaction) -> Result<(), GraphError> {
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
    records: &mut RecordOverlay<'_>,
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
    records: &mut RecordOverlay<'_>,
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
    records: &mut RecordOverlay<'_>,
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
    records: &mut RecordOverlay<'_>,
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
    records: &mut RecordOverlay<'_>,
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
    let mut accepted = 0_usize;
    let mut proposed = 0_usize;
    let mut entity_references = 0_usize;
    let mut accepted_are_declared = true;
    records.for_each_reverse_reference(target, |owner, reference| {
        let claim_roles = match reference.owner_kind {
            REVERSE_KIND_ASSERTION => {
                REVERSE_ROLE_ASSERTION_SUBJECT | REVERSE_ROLE_ASSERTION_OBJECT
            }
            REVERSE_KIND_RELATIONSHIP => {
                REVERSE_ROLE_RELATIONSHIP_FROM
                    | REVERSE_ROLE_RELATIONSHIP_TO
                    | REVERSE_ROLE_RELATIONSHIP_PROPERTY
            }
            _ => 0,
        };
        let is_claim_dependency = reference.roles & claim_roles != 0;
        let is_accepted = is_claim_dependency && reference.owner_state == REVERSE_STATE_ACCEPTED;
        let is_proposed = is_claim_dependency && reference.owner_state == REVERSE_STATE_PROPOSED;
        if reference.owner_kind == REVERSE_KIND_ENTITY
            && reference.owner_state == REVERSE_STATE_ACTIVE
            && reference.roles & REVERSE_ROLE_ENTITY_PROPERTY != 0
            && owner != target
        {
            entity_references = entity_references.saturating_add(1);
        }
        if is_accepted {
            accepted = accepted.saturating_add(1);
            accepted_are_declared &= declared.binary_search(&owner).is_ok();
        }
        proposed = proposed.saturating_add(usize::from(is_proposed));
    });
    let dependents = accepted.saturating_add(entity_references);

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
            if declared.len() != accepted || !accepted_are_declared {
                return Err(GraphError::CascadeDeclarationChanged);
            }
            // Preserve the format-1.0 reducer's deterministic failure order: relationships first,
            // then assertions, with identifiers ordered inside each kind.
            for relationships in [true, false] {
                for id in declared {
                    let matches_pass = matches!(
                        (relationships, records.get(id)),
                        (true, Some(Record::Relationship(_))) | (false, Some(Record::Assertion(_)))
                    );
                    if !matches_pass {
                        continue;
                    }
                    let record = records
                        .get_mut(id)
                        .expect("declared record was verified during dependency scan");
                    match record {
                        Record::Assertion(assertion) => {
                            assertion.status = AssertionStatus::Retracted;
                            assertion.version = next_version(assertion.version, *id)?;
                            assertion.modified_revision = revision;
                        }
                        Record::Relationship(relationship) => {
                            relationship.status = AssertionStatus::Retracted;
                            relationship.version = next_version(relationship.version, *id)?;
                            relationship.modified_revision = revision;
                        }
                        _ => unreachable!("declared dependency kind was verified during scan"),
                    }
                    affected.insert(*id);
                }
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
        validate_record(scope, *id, record, records)?;
    }
    Ok(())
}

fn validate_changed_state(
    scope: NamespaceRef,
    records: &RecordOverlay<'_>,
    affected: &BTreeSet<RecordRef>,
) -> Result<(), GraphError> {
    for id in affected {
        let record = records.get(id).ok_or(GraphError::IndexCorrupt(*id))?;
        validate_record(scope, *id, record, records)?;
    }
    Ok(())
}

trait RecordLookup {
    fn lookup(&self, id: &RecordRef) -> Option<&Record>;
}

impl RecordLookup for BTreeMap<RecordRef, Record> {
    fn lookup(&self, id: &RecordRef) -> Option<&Record> {
        self.get(id)
    }
}

impl RecordLookup for RecordOverlay<'_> {
    fn lookup(&self, id: &RecordRef) -> Option<&Record> {
        self.get(id)
    }
}

fn validate_record(
    scope: NamespaceRef,
    id: RecordRef,
    record: &Record,
    records: &impl RecordLookup,
) -> Result<(), GraphError> {
    validate_scope(scope, id)?;
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
                && !matches!(records.lookup(&previous), Some(Record::Assertion(_)))
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
                && !matches!(records.lookup(&previous), Some(Record::Relationship(_)))
            {
                return Err(GraphError::ReferenceNotVisible(previous));
            }
        }
    }
    Ok(())
}

fn validate_evidence(
    records: &impl RecordLookup,
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
        if !matches!(records.lookup(evidence), Some(Record::Evidence(_))) {
            return Err(GraphError::MissingEvidence(*evidence));
        }
    }
    Ok(())
}

fn validate_value(
    records: &impl RecordLookup,
    scope: NamespaceRef,
    value: &Value,
) -> Result<(), GraphError> {
    match value {
        Value::RecordRef(record) => {
            validate_scope(scope, *record)?;
            if !records.lookup(record).is_some_and(is_visible_record) {
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

fn require_active_entity(records: &impl RecordLookup, id: RecordRef) -> Result<(), GraphError> {
    if matches!(records.lookup(&id), Some(Record::Entity(entity)) if entity.lifecycle == EntityLifecycle::Active)
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
    snapshot.reverse.clear();
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
        add_reverse_contributions(&mut snapshot.reverse, record);
    }
}

fn remove_index_contributions(snapshot: &mut GraphSnapshot, record: &Record) {
    match record {
        Record::Relationship(relationship) => {
            if relationship.status == AssertionStatus::Accepted {
                remove_index_value(&mut snapshot.outgoing, relationship.from, relationship.id);
                remove_index_value(&mut snapshot.incoming, relationship.to, relationship.id);
            }
            for evidence in &relationship.evidence {
                remove_index_value(&mut snapshot.provenance, *evidence, relationship.id);
            }
        }
        Record::Assertion(assertion) => {
            for evidence in &assertion.evidence {
                remove_index_value(&mut snapshot.provenance, *evidence, assertion.id);
            }
        }
        _ => {}
    }
    remove_reverse_contributions(&mut snapshot.reverse, record);
}

fn add_index_contributions(snapshot: &mut GraphSnapshot, record: &Record) {
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
    add_reverse_contributions(&mut snapshot.reverse, record);
}

pub(crate) const REVERSE_KIND_ENTITY: u8 = 1;
pub(crate) const REVERSE_KIND_ASSERTION: u8 = 2;
pub(crate) const REVERSE_KIND_RELATIONSHIP: u8 = 3;

pub(crate) const REVERSE_STATE_ACTIVE: u8 = 1;
pub(crate) const REVERSE_STATE_DELETED: u8 = 2;
pub(crate) const REVERSE_STATE_PROPOSED: u8 = 1;
pub(crate) const REVERSE_STATE_ACCEPTED: u8 = 2;
pub(crate) const REVERSE_STATE_REJECTED: u8 = 3;
pub(crate) const REVERSE_STATE_DISPUTED: u8 = 4;
pub(crate) const REVERSE_STATE_SUPERSEDED: u8 = 5;
pub(crate) const REVERSE_STATE_RETRACTED: u8 = 6;
pub(crate) const REVERSE_STATE_EXPIRED: u8 = 7;

pub(crate) const REVERSE_ROLE_ENTITY_PROPERTY: u16 = 1 << 0;
pub(crate) const REVERSE_ROLE_ASSERTION_SUBJECT: u16 = 1 << 1;
pub(crate) const REVERSE_ROLE_ASSERTION_OBJECT: u16 = 1 << 2;
pub(crate) const REVERSE_ROLE_RELATIONSHIP_FROM: u16 = 1 << 3;
pub(crate) const REVERSE_ROLE_RELATIONSHIP_TO: u16 = 1 << 4;
pub(crate) const REVERSE_ROLE_RELATIONSHIP_PROPERTY: u16 = 1 << 5;
pub(crate) const REVERSE_ROLE_EVIDENCE: u16 = 1 << 6;
pub(crate) const REVERSE_ROLE_CORRECTION_OF: u16 = 1 << 7;

pub(crate) fn record_reverse_references(record: &Record) -> BTreeMap<RecordRef, ReverseReference> {
    let mut references = BTreeMap::new();
    visit_record_references(record, &mut |target, roles| {
        references
            .entry(target)
            .and_modify(|reference: &mut ReverseReference| reference.roles |= roles)
            .or_insert_with(|| reverse_reference(record, roles));
    });
    references
}

fn reverse_reference_for_target(record: &Record, expected: RecordRef) -> Option<ReverseReference> {
    let mut roles = 0_u16;
    visit_record_references(record, &mut |target, role| {
        if target == expected {
            roles |= role;
        }
    });
    (roles != 0).then(|| reverse_reference(record, roles))
}

pub(crate) fn reverse_reference(record: &Record, roles: u16) -> ReverseReference {
    let (owner_kind, owner_state) = match record {
        Record::Entity(entity) => (
            REVERSE_KIND_ENTITY,
            match entity.lifecycle {
                EntityLifecycle::Active => REVERSE_STATE_ACTIVE,
                EntityLifecycle::Deleted => REVERSE_STATE_DELETED,
            },
        ),
        Record::Assertion(assertion) => (
            REVERSE_KIND_ASSERTION,
            reverse_assertion_state(assertion.status),
        ),
        Record::Relationship(relationship) => (
            REVERSE_KIND_RELATIONSHIP,
            reverse_assertion_state(relationship.status),
        ),
        Record::Evidence(_) => unreachable!("evidence records have no graph references"),
    };
    ReverseReference {
        owner_kind,
        owner_state,
        roles,
        owner_version: record.version(),
        owner_revision: record.modified_revision(),
    }
}

const fn reverse_assertion_state(status: AssertionStatus) -> u8 {
    match status {
        AssertionStatus::Proposed => REVERSE_STATE_PROPOSED,
        AssertionStatus::Accepted => REVERSE_STATE_ACCEPTED,
        AssertionStatus::Rejected => REVERSE_STATE_REJECTED,
        AssertionStatus::Disputed => REVERSE_STATE_DISPUTED,
        AssertionStatus::Superseded => REVERSE_STATE_SUPERSEDED,
        AssertionStatus::Retracted => REVERSE_STATE_RETRACTED,
        AssertionStatus::Expired => REVERSE_STATE_EXPIRED,
    }
}

pub(crate) fn visit_record_references(record: &Record, visitor: &mut impl FnMut(RecordRef, u16)) {
    let result: Result<(), core::convert::Infallible> =
        try_visit_record_references(record, &mut |id, role| {
            visitor(id, role);
            Ok(())
        });
    match result {
        Ok(()) => {}
        Err(error) => match error {},
    }
}

pub(crate) fn try_visit_record_references<E>(
    record: &Record,
    visitor: &mut impl FnMut(RecordRef, u16) -> Result<(), E>,
) -> Result<(), E> {
    match record {
        Record::Entity(entity) => {
            try_visit_value_references(&entity.properties, REVERSE_ROLE_ENTITY_PROPERTY, visitor)?;
        }
        Record::Evidence(_) => {}
        Record::Assertion(assertion) => {
            visitor(assertion.subject, REVERSE_ROLE_ASSERTION_SUBJECT)?;
            try_visit_value_references(&assertion.object, REVERSE_ROLE_ASSERTION_OBJECT, visitor)?;
            for evidence in &assertion.evidence {
                visitor(*evidence, REVERSE_ROLE_EVIDENCE)?;
            }
            if let Some(previous) = assertion.correction_of {
                visitor(previous, REVERSE_ROLE_CORRECTION_OF)?;
            }
        }
        Record::Relationship(relationship) => {
            visitor(relationship.from, REVERSE_ROLE_RELATIONSHIP_FROM)?;
            visitor(relationship.to, REVERSE_ROLE_RELATIONSHIP_TO)?;
            try_visit_value_references(
                &relationship.properties,
                REVERSE_ROLE_RELATIONSHIP_PROPERTY,
                visitor,
            )?;
            for evidence in &relationship.evidence {
                visitor(*evidence, REVERSE_ROLE_EVIDENCE)?;
            }
            if let Some(previous) = relationship.correction_of {
                visitor(previous, REVERSE_ROLE_CORRECTION_OF)?;
            }
        }
    }
    Ok(())
}

/// Prepare against complete current, history and reverse proofs needed by one disk transaction.
pub(crate) fn prepare_from_complete_disk_proofs(
    scope: NamespaceRef,
    base_revision: CommitRevision,
    records: BTreeMap<RecordRef, Record>,
    history: BTreeMap<RecordRef, Vec<Record>>,
    reverse: BTreeMap<RecordRef, BTreeMap<RecordRef, ReverseReference>>,
    policy: Option<NamespacePolicy>,
    transaction: &GraphTransaction,
) -> Result<(PreparedGraph, Option<NamespacePolicy>), GraphError> {
    let state = GraphState {
        snapshot: GraphSnapshot {
            scope,
            revision: Some(base_revision),
            records,
            history,
            outgoing: BTreeMap::new(),
            incoming: BTreeMap::new(),
            provenance: BTreeMap::new(),
            reverse,
            policy,
            policy_history: BTreeMap::new(),
        },
    };
    let prepared = state.prepare_transaction(
        transaction,
        base_revision
            .checked_next()
            .map_err(|_| GraphError::RevisionExhausted)?,
    )?;
    Ok((prepared, state.snapshot.policy))
}

fn try_visit_value_references<E>(
    value: &Value,
    role: u16,
    visitor: &mut impl FnMut(RecordRef, u16) -> Result<(), E>,
) -> Result<(), E> {
    match value {
        Value::RecordRef(record) => visitor(*record, role),
        Value::List(values) => {
            for value in values.as_slice() {
                try_visit_value_references(value, role, visitor)?;
            }
            Ok(())
        }
        Value::Map(values) => {
            for (_, value) in values.as_slice() {
                try_visit_value_references(value, role, visitor)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn add_reverse_contributions(
    reverse: &mut BTreeMap<RecordRef, BTreeMap<RecordRef, ReverseReference>>,
    record: &Record,
) {
    for (target, reference) in record_reverse_references(record) {
        reverse
            .entry(target)
            .or_default()
            .insert(record.id(), reference);
    }
}

fn remove_reverse_contributions(
    reverse: &mut BTreeMap<RecordRef, BTreeMap<RecordRef, ReverseReference>>,
    record: &Record,
) {
    for target in record_reverse_references(record).into_keys() {
        let remove_target = if let Some(owners) = reverse.get_mut(&target) {
            owners.remove(&record.id());
            owners.is_empty()
        } else {
            false
        };
        if remove_target {
            reverse.remove(&target);
        }
    }
}

fn remove_index_value(
    index: &mut BTreeMap<RecordRef, BTreeSet<RecordRef>>,
    key: RecordRef,
    value: RecordRef,
) {
    let remove_key = if let Some(values) = index.get_mut(&key) {
        values.remove(&value);
        values.is_empty()
    } else {
        false
    };
    if remove_key {
        index.remove(&key);
    }
}

fn record_at(
    history: &BTreeMap<RecordRef, Vec<Record>>,
    id: RecordRef,
    revision: CommitRevision,
) -> Option<&Record> {
    let versions = history.get(&id)?;
    let insertion = versions.partition_point(|record| record.modified_revision() <= revision);
    insertion
        .checked_sub(1)
        .and_then(|index| versions.get(index))
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

fn next_version(version: RecordVersion, record: RecordRef) -> Result<RecordVersion, GraphError> {
    version
        .checked_next()
        .map_err(|_| GraphError::RecordVersionExhausted(record))
}

fn wrong_kind_or_missing(
    records: &RecordOverlay<'_>,
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
    DuplicateMutation(RecordRef),
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
    PreparedStateMismatch,
    ResourceLimit,
}

impl GraphError {
    const fn into_apply_error(self) -> ApplyError {
        match self {
            Self::PreconditionFailed(_)
            | Self::PredicateWasFalse(_)
            | Self::PredicateChanged(_)
            | Self::AlreadyExists(_)
            | Self::DuplicateMutation(_)
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

#[cfg(test)]
mod delta_tests {
    use super::*;
    use crate::{NewEntity, NewEvidence};
    use uste_types::BoundedString;

    fn scope() -> NamespaceRef {
        NamespaceRef::new(
            DatabaseId::from_bytes([91; 16]),
            NamespaceId::from_bytes([92; 16]),
        )
    }

    fn id(value: u64) -> RecordRef {
        let mut bytes = [0_u8; 16];
        bytes[8..].copy_from_slice(&value.to_be_bytes());
        let scope = scope();
        RecordRef::new(
            scope.database(),
            scope.namespace(),
            RecordId::from_bytes(bytes),
        )
    }

    fn entity(id: RecordRef) -> Operation {
        Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Entity(NewEntity {
                id,
                entity_type: BoundedString::new("delta-fixture".to_owned()).unwrap(),
                schema_version: 1,
                properties: Value::Null,
            }),
        }
    }

    fn full_rebuild_publish(
        mut snapshot: GraphSnapshot,
        prepared: &PreparedGraph,
    ) -> GraphSnapshot {
        for change in &prepared.changes {
            snapshot.records.insert(change.id, change.after.clone());
            snapshot
                .history
                .entry(change.id)
                .or_default()
                .push(change.after.clone());
        }
        if let Some(policy) = &prepared.policy_change {
            snapshot.policy = Some(policy.clone());
            snapshot
                .policy_history
                .insert(prepared.revision, policy.clone());
        }
        snapshot.revision = Some(prepared.revision);
        rebuild_indexes(&mut snapshot);
        snapshot
    }

    fn legacy_result_digest(snapshot: &GraphSnapshot, prepared: &PreparedGraph) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(b"USTE-GRAPH-RESULT-V1\0");
        digest.update(prepared.revision.get().to_be_bytes());
        digest.update((prepared.changes.len() as u64).to_be_bytes());
        for change in &prepared.changes {
            let record = snapshot.records.get(&change.id).unwrap();
            let encoded = encode_result_record(record).unwrap();
            digest.update((encoded.len() as u64).to_be_bytes());
            digest.update(encoded);
        }
        let policy = encode_result_policy(snapshot.policy.as_ref()).unwrap();
        digest.update((policy.len() as u64).to_be_bytes());
        digest.update(policy);
        digest.finalize().into()
    }

    #[test]
    fn one_record_update_prepares_one_sorted_delta_and_matches_full_rebuild() {
        let mut state = GraphState::new(scope());
        let create = GraphTransaction::new(
            scope(),
            (1..=1_024).map(|value| entity(id(value))).collect(),
        );
        let prepared = state
            .prepare_transaction(&create, CommitRevision::FIRST)
            .unwrap();
        TransactionState::publish(&mut state, prepared);

        let target = id(512);
        let update = GraphTransaction::new(
            scope(),
            vec![Operation::ReplaceEntity {
                target,
                expected: Expected::Version(RecordVersion::FIRST),
                properties: Value::Unsigned(7),
            }],
        );
        let prepared = state
            .prepare_transaction(&update, CommitRevision::new(2).unwrap())
            .unwrap();
        assert_eq!(prepared.changes.len(), 1);
        assert_eq!(prepared.changes[0].id, target);
        assert!(prepared.changes[0].before.is_some());
        assert_eq!(prepared.changes[0].after.id(), target);

        let expected = full_rebuild_publish(state.snapshot(), &prepared);
        assert_eq!(
            prepared.result_digest,
            legacy_result_digest(&expected, &prepared)
        );
        TransactionState::publish(&mut state, prepared);
        assert_eq!(state.snapshot(), expected);
        state.snapshot().validate_derived_indexes().unwrap();
    }

    #[test]
    fn stale_prepared_delta_panics_before_mutating_live_state() {
        let mut state = GraphState::new(scope());
        let prepared = state
            .prepare_transaction(
                &GraphTransaction::new(scope(), vec![entity(id(1)), entity(id(2))]),
                CommitRevision::FIRST,
            )
            .unwrap();
        TransactionState::publish(&mut state, prepared);

        let replace = |target, value| {
            GraphTransaction::new(
                scope(),
                vec![Operation::ReplaceEntity {
                    target,
                    expected: Expected::Version(RecordVersion::FIRST),
                    properties: Value::Unsigned(value),
                }],
            )
        };
        let first = state
            .prepare_transaction(&replace(id(1), 11), CommitRevision::new(2).unwrap())
            .unwrap();
        let stale = state
            .prepare_transaction(&replace(id(2), 22), CommitRevision::new(2).unwrap())
            .unwrap();
        TransactionState::publish(&mut state, first);
        let published = state.snapshot();

        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            TransactionState::publish(&mut state, stale);
        }));
        assert!(rejected.is_err());
        assert_eq!(state.snapshot(), published);
    }

    #[test]
    fn reverse_references_aggregate_roles_and_track_owner_state() {
        let target = id(1);
        let other = id(2);
        let evidence = id(3);
        let relationship = id(4);
        let mut state = GraphState::new(scope());
        let properties =
            Value::list(vec![Value::RecordRef(target), Value::RecordRef(target)]).unwrap();
        let prepared = state
            .prepare_transaction(
                &GraphTransaction::new(
                    scope(),
                    vec![
                        entity(target),
                        entity(other),
                        Operation::Create {
                            expected: Expected::Absent,
                            record: NewRecord::Evidence(NewEvidence {
                                id: evidence,
                                digest: [0x94; 32],
                                locator: BoundedString::new("reverse-source".to_owned()).unwrap(),
                            }),
                        },
                        Operation::Create {
                            expected: Expected::Absent,
                            record: NewRecord::Relationship(NewRelationship {
                                id: relationship,
                                from: target,
                                to: other,
                                relationship_type: BoundedString::new("reverse-edge".to_owned())
                                    .unwrap(),
                                properties,
                                evidence: vec![evidence],
                                valid_time: crate::ValidTime::Unknown,
                            }),
                        },
                    ],
                ),
                CommitRevision::FIRST,
            )
            .unwrap();
        TransactionState::publish(&mut state, prepared);

        let bucket = state.snapshot.reverse.get(&target).unwrap();
        assert_eq!(bucket.len(), 1);
        let reference = bucket.get(&relationship).unwrap();
        assert_eq!(reference.owner_kind, REVERSE_KIND_RELATIONSHIP);
        assert_eq!(reference.owner_state, REVERSE_STATE_PROPOSED);
        assert_eq!(
            reference.roles,
            REVERSE_ROLE_RELATIONSHIP_FROM | REVERSE_ROLE_RELATIONSHIP_PROPERTY
        );
        assert_eq!(reference.owner_version, RecordVersion::FIRST);

        let prepared = state
            .prepare_transaction(
                &GraphTransaction::new(
                    scope(),
                    vec![Operation::ActOnRelationship {
                        target: relationship,
                        expected: Expected::Version(RecordVersion::FIRST),
                        action: AssertionAction::Accept,
                        correction: None,
                        correction_expected: None,
                    }],
                ),
                CommitRevision::new(2).unwrap(),
            )
            .unwrap();
        TransactionState::publish(&mut state, prepared);
        let reference = state.snapshot.reverse[&target][&relationship];
        assert_eq!(reference.owner_state, REVERSE_STATE_ACCEPTED);
        assert_eq!(reference.owner_version, RecordVersion::new(2).unwrap());
        assert_eq!(reference.owner_revision, CommitRevision::new(2).unwrap());
    }
}

#[cfg(test)]
mod checkpoint_history_tests {
    use super::*;
    use uste_types::BoundedString;

    fn scope() -> NamespaceRef {
        NamespaceRef::new(
            DatabaseId::from_bytes([71; 16]),
            NamespaceId::from_bytes([72; 16]),
        )
    }

    fn id(value: u8) -> RecordRef {
        let scope = scope();
        RecordRef::new(
            scope.database(),
            scope.namespace(),
            RecordId::from_bytes([value; 16]),
        )
    }

    fn snapshot(
        revision: CommitRevision,
        records: BTreeMap<RecordRef, Record>,
        history: BTreeMap<RecordRef, Vec<Record>>,
    ) -> GraphSnapshot {
        GraphSnapshot {
            scope: scope(),
            revision: Some(revision),
            records,
            history,
            outgoing: BTreeMap::new(),
            incoming: BTreeMap::new(),
            provenance: BTreeMap::new(),
            reverse: BTreeMap::new(),
            policy: None,
            policy_history: BTreeMap::new(),
        }
    }

    #[test]
    fn checkpoint_rejects_references_that_did_not_exist_at_recorded_revision() {
        let first = CommitRevision::FIRST;
        let second = CommitRevision::new(2).unwrap();
        let subject = Record::Entity(EntityRecord {
            id: id(1),
            version: RecordVersion::FIRST,
            lifecycle: EntityLifecycle::Active,
            entity_type: BoundedString::new("late-subject".to_owned()).unwrap(),
            schema_version: 1,
            properties: Value::Null,
            created_revision: second,
            modified_revision: second,
        });
        let evidence = Record::Evidence(EvidenceRecord {
            id: id(2),
            version: RecordVersion::FIRST,
            digest: [73; 32],
            locator: BoundedString::new("source:temporal".to_owned()).unwrap(),
            created_revision: first,
        });
        let proposed = AssertionRecord {
            id: id(3),
            version: RecordVersion::FIRST,
            subject: id(1),
            predicate: BoundedString::new("has".to_owned()).unwrap(),
            object: Value::Null,
            evidence: vec![id(2)],
            status: AssertionStatus::Proposed,
            valid_time: crate::ValidTime::Unknown,
            correction_of: None,
            recorded_revision: first,
            modified_revision: first,
        };
        let mut rejected = proposed.clone();
        rejected.version = RecordVersion::new(2).unwrap();
        rejected.status = AssertionStatus::Rejected;
        rejected.modified_revision = second;
        let rejected = Record::Assertion(rejected);
        let records = BTreeMap::from([
            (id(1), subject.clone()),
            (id(2), evidence.clone()),
            (id(3), rejected.clone()),
        ]);
        let history = BTreeMap::from([
            (id(1), vec![subject]),
            (id(2), vec![evidence]),
            (id(3), vec![Record::Assertion(proposed), rejected]),
        ]);
        let encoded = encode_graph_checkpoint(&snapshot(second, records, history)).unwrap();
        assert_eq!(
            decode_graph_checkpoint(scope(), second, &encoded),
            Err(CheckpointStateError::Invalid)
        );
    }

    #[test]
    fn checkpoint_rejects_property_changes_hidden_inside_deletion() {
        let first = CommitRevision::FIRST;
        let second = CommitRevision::new(2).unwrap();
        let active = EntityRecord {
            id: id(1),
            version: RecordVersion::FIRST,
            lifecycle: EntityLifecycle::Active,
            entity_type: BoundedString::new("entity".to_owned()).unwrap(),
            schema_version: 1,
            properties: Value::Unsigned(1),
            created_revision: first,
            modified_revision: first,
        };
        let mut deleted = active.clone();
        deleted.version = RecordVersion::new(2).unwrap();
        deleted.lifecycle = EntityLifecycle::Deleted;
        deleted.properties = Value::Unsigned(2);
        deleted.modified_revision = second;
        let current = Record::Entity(deleted);
        let encoded = encode_graph_checkpoint(&snapshot(
            second,
            BTreeMap::from([(id(1), current.clone())]),
            BTreeMap::from([(id(1), vec![Record::Entity(active), current])]),
        ))
        .unwrap();
        assert_eq!(
            decode_graph_checkpoint(scope(), second, &encoded),
            Err(CheckpointStateError::Invalid)
        );
    }

    #[test]
    fn historical_lookup_binary_searches_exact_and_between_revision_boundaries() {
        let revisions = [1_u64, 3, 5];
        let versions = revisions
            .into_iter()
            .enumerate()
            .map(|(index, revision)| {
                Record::Entity(EntityRecord {
                    id: id(1),
                    version: RecordVersion::new(u64::try_from(index + 1).unwrap()).unwrap(),
                    lifecycle: EntityLifecycle::Active,
                    entity_type: BoundedString::new("entity".to_owned()).unwrap(),
                    schema_version: 1,
                    properties: Value::Unsigned(u128::from(revision)),
                    created_revision: CommitRevision::FIRST,
                    modified_revision: CommitRevision::new(revision).unwrap(),
                })
            })
            .collect();
        let history = BTreeMap::from([(id(1), versions)]);
        for (revision, expected_version) in [(1, 1), (2, 1), (3, 2), (4, 2), (5, 3)] {
            assert_eq!(
                record_at(&history, id(1), CommitRevision::new(revision).unwrap())
                    .unwrap()
                    .version()
                    .get(),
                expected_version
            );
        }
    }
}
