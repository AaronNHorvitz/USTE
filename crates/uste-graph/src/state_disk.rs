//! Certificate-anchored complete graph-state derived cache.
//!
//! This profile remains optional and read-only. The authenticated journal is the only commit
//! authority; reconstructed state becomes usable only through exact-paired seeded recovery.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use uste_crypto::EntropySource;
use uste_storage::{
    DurableIndexRoot, IndexDelta, IndexEntry, IndexRootAnchor, IndexRootInput, IndexRunDescriptor,
    IndexRunMergeLimits, IndexRunMergeReport, IndexRunReadLimits, IndexRunReadReport,
    IndexRunVisitor, IndexScrubReport, MAX_INDEX_DELTA_LOGICAL_BYTES, MAX_INDEX_ENTRIES_PER_RUN,
    MAX_INDEX_PAGES_PER_RUN, MAX_INDEX_RUN_LOGICAL_BYTES, OwnershipFileSystem, PageCache,
    RecoveredIndexRoot,
    journal::{DurableKeyEnvelope, StorageError},
};
use uste_txn::{
    AuthenticatedIndexRecovery, CheckpointState, CheckpointStateError, CommitCoordinator,
    CoordinatorMetadataCandidate, CoordinatorMetadataLoadLimits, CoordinatorMetadataLoadReport,
    CoordinatorRecoverySeed, TransactionError, TransactionOutcome,
    reconstruct_coordinator_metadata_seed_for_recovery,
};
use uste_types::{CommitRevision, RecordId, RecordRef};

use crate::codec::{decode_result_policy, encode_result_policy};
use crate::{
    GraphCodecError, GraphDiskError, GraphSnapshot, GraphState, GraphTransaction, Record,
    decode_stored_record, encode_stored_record,
    state::{PreparedGraph, ReverseReference, record_reverse_references},
};

pub const GRAPH_STATE_PROFILE_V1: [u8; 32] = [
    0x31, 0x3f, 0x9c, 0x9b, 0xe8, 0xa4, 0x20, 0x6d, 0xb1, 0x11, 0x8a, 0xf6, 0x2c, 0xe2, 0x08, 0x61,
    0x78, 0x44, 0x4c, 0x03, 0xdc, 0x22, 0xa0, 0xab, 0xba, 0xc7, 0xfb, 0x87, 0x14, 0xc6, 0x6d, 0x52,
];

const FAMILY_METADATA: u8 = 1;
const FAMILY_CURRENT_RECORD: u8 = 2;
const FAMILY_RECORD_HISTORY: u8 = 3;
const FAMILY_OUTGOING: u8 = 4;
const FAMILY_INCOMING: u8 = 5;
const FAMILY_PROVENANCE: u8 = 6;
const FAMILY_REVERSE: u8 = 7;
const FAMILY_POLICY: u8 = 8;
const FAMILY_COUNT: u8 = 8;

pub struct DerivedGraphStateRoot {
    root: RecoveredIndexRoot,
}

/// Authenticated, journal-anchored root that has not yet been semantically reconstructed.
///
/// This is intentionally distinct from `DerivedGraphStateRoot`, whose bytes have already been
/// compared with a live reducer snapshot.
pub struct GraphStateRootCandidate {
    root: RecoveredIndexRoot,
}

impl core::fmt::Debug for GraphStateRootCandidate {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("GraphStateRootCandidate")
            .field("revision", &self.root.revision())
            .field("generation", &self.root.generation())
            .finish_non_exhaustive()
    }
}

impl GraphStateRootCandidate {
    #[must_use]
    pub const fn anchor(&self) -> IndexRootAnchor {
        self.root.anchor()
    }

    #[must_use]
    pub const fn revision(&self) -> CommitRevision {
        self.root.revision()
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.root.generation()
    }
}

/// Caller-selected aggregate reconstruction bounds. These are intentionally independent of the
/// legacy monolithic checkpoint caps; the accepted state-root format permits larger runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphStateLoadLimits {
    maximum_records: u64,
    maximum_versions: u64,
    maximum_policy_history: u64,
    maximum_total_entries: u64,
    maximum_total_pages: u64,
    maximum_logical_bytes: u64,
}

impl GraphStateLoadLimits {
    pub const fn new(
        maximum_records: u64,
        maximum_versions: u64,
        maximum_policy_history: u64,
        maximum_total_entries: u64,
        maximum_total_pages: u64,
        maximum_logical_bytes: u64,
    ) -> Result<Self, GraphDiskError> {
        if maximum_records > MAX_INDEX_ENTRIES_PER_RUN
            || maximum_versions > MAX_INDEX_ENTRIES_PER_RUN
            || maximum_policy_history > MAX_INDEX_ENTRIES_PER_RUN
            || maximum_total_entries == 0
            || maximum_total_entries > MAX_INDEX_ENTRIES_PER_RUN * FAMILY_COUNT as u64
            || maximum_total_pages == 0
            || maximum_total_pages > MAX_INDEX_PAGES_PER_RUN * FAMILY_COUNT as u64
            || maximum_logical_bytes == 0
            || maximum_logical_bytes > MAX_INDEX_RUN_LOGICAL_BYTES * FAMILY_COUNT as u64
        {
            return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
        }
        Ok(Self {
            maximum_records,
            maximum_versions,
            maximum_policy_history,
            maximum_total_entries,
            maximum_total_pages,
            maximum_logical_bytes,
        })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphStateLoadReport {
    pub runs: u64,
    pub entries: u64,
    pub logical_bytes: u64,
    pub pages_read: u64,
}

/// Aggregate bounds for retaining one transaction's exact graph-state family deltas.
///
/// The budget is debited while family maps are formed. The preceding graph transaction prepare
/// and each record's temporary canonical reference coalescing remain governed by the graph
/// operation/reference limits rather than these derived-cache limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphStateDeltaLimits {
    maximum_deltas: u64,
    maximum_logical_bytes: u64,
}

impl GraphStateDeltaLimits {
    pub const fn new(
        maximum_deltas: u64,
        maximum_logical_bytes: u64,
    ) -> Result<Self, GraphDiskError> {
        if maximum_deltas == 0
            || maximum_deltas > MAX_INDEX_ENTRIES_PER_RUN * FAMILY_COUNT as u64
            || maximum_logical_bytes == 0
            || maximum_logical_bytes > MAX_INDEX_DELTA_LOGICAL_BYTES * FAMILY_COUNT as u64
        {
            return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
        }
        Ok(Self {
            maximum_deltas,
            maximum_logical_bytes,
        })
    }
}

/// Opaque, bounded exact family changes prepared against one journal-certified graph revision.
pub struct GraphStateRootDelta {
    scope: uste_types::NamespaceRef,
    base_anchor: IndexRootAnchor,
    base_revision: CommitRevision,
    revision: CommitRevision,
    result_digest: [u8; 32],
    families: [Vec<IndexDelta>; FAMILY_COUNT as usize],
    deltas: u64,
    logical_bytes: u64,
}

impl core::fmt::Debug for GraphStateRootDelta {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("GraphStateRootDelta")
            .field("scope", &"[REDACTED]")
            .field("base_revision", &self.base_revision)
            .field("revision", &self.revision)
            .field(
                "family_delta_counts",
                &self.families.each_ref().map(Vec::len),
            )
            .field("deltas", &self.deltas)
            .field("logical_bytes", &self.logical_bytes)
            .finish()
    }
}

impl GraphStateRootDelta {
    #[must_use]
    pub const fn base_revision(&self) -> CommitRevision {
        self.base_revision
    }

    #[must_use]
    pub const fn revision(&self) -> CommitRevision {
        self.revision
    }

    #[must_use]
    pub const fn delta_count(&self) -> u64 {
        self.deltas
    }

    #[must_use]
    pub const fn logical_bytes(&self) -> u64 {
        self.logical_bytes
    }
}

/// Per-family storage merge budgets in graph-state family order 1 through 8.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphStateRootMergeLimits {
    families: [IndexRunMergeLimits; FAMILY_COUNT as usize],
}

impl GraphStateRootMergeLimits {
    #[must_use]
    pub const fn new(families: [IndexRunMergeLimits; FAMILY_COUNT as usize]) -> Self {
        Self { families }
    }

    #[must_use]
    pub const fn uniform(limit: IndexRunMergeLimits) -> Self {
        Self {
            families: [limit; FAMILY_COUNT as usize],
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphStateRootMergeReport {
    pub runs: u64,
    pub base_entries: u64,
    pub base_logical_bytes: u64,
    pub deltas: u64,
    pub delta_logical_bytes: u64,
    pub insertions: u64,
    pub replacements: u64,
    pub deletions: u64,
    pub output_entries: u64,
    pub output_logical_bytes: u64,
    pub pages_read: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphRecoverySeedReport {
    pub graph_state: GraphStateLoadReport,
    pub coordinator_metadata: CoordinatorMetadataLoadReport,
}

trait GraphStateIndexReader<F>
where
    F: OwnershipFileSystem,
{
    fn reader_scope(&self) -> uste_types::NamespaceRef;

    fn reader_load_roots(
        &self,
        filesystem: &mut F,
        profile: [u8; 32],
    ) -> Result<Vec<RecoveredIndexRoot>, TransactionError>;

    fn reader_visit_run(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        limits: IndexRunReadLimits,
        visitor: &mut IndexRunVisitor<'_>,
    ) -> Result<IndexRunReadReport, TransactionError>;
}

impl<F, W, E, I> GraphStateIndexReader<F> for CommitCoordinator<GraphState, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn reader_scope(&self) -> uste_types::NamespaceRef {
        self.scope()
    }

    fn reader_load_roots(
        &self,
        filesystem: &mut F,
        profile: [u8; 32],
    ) -> Result<Vec<RecoveredIndexRoot>, TransactionError> {
        self.load_index_roots(filesystem, profile)
    }

    fn reader_visit_run(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        limits: IndexRunReadLimits,
        visitor: &mut IndexRunVisitor<'_>,
    ) -> Result<IndexRunReadReport, TransactionError> {
        self.visit_index_run(filesystem, root, family, limits, visitor)
    }
}

impl<F, W, E, I> GraphStateIndexReader<F> for AuthenticatedIndexRecovery<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn reader_scope(&self) -> uste_types::NamespaceRef {
        self.scope()
    }

    fn reader_load_roots(
        &self,
        filesystem: &mut F,
        profile: [u8; 32],
    ) -> Result<Vec<RecoveredIndexRoot>, TransactionError> {
        self.load_index_roots(filesystem, profile)
    }

    fn reader_visit_run(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        limits: IndexRunReadLimits,
        visitor: &mut IndexRunVisitor<'_>,
    ) -> Result<IndexRunReadReport, TransactionError> {
        self.visit_index_run(filesystem, root, family, limits, visitor)
    }
}

impl core::fmt::Debug for DerivedGraphStateRoot {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("DerivedGraphStateRoot")
            .field("revision", &self.root.revision())
            .field("generation", &self.root.generation())
            .finish_non_exhaustive()
    }
}

impl DerivedGraphStateRoot {
    #[must_use]
    pub const fn revision(&self) -> CommitRevision {
        self.root.revision()
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.root.generation()
    }
}

/// Prepare exact, bounded family deltas before the corresponding graph transaction is committed.
///
/// The returned plan is opaque and bound to the exact admitted base root, target revision, scope,
/// and reducer result digest. Preparing it performs no durable writes.
pub fn prepare_graph_state_root_delta<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    base: &DerivedGraphStateRoot,
    transaction: &GraphTransaction,
    revision: CommitRevision,
    limits: GraphStateDeltaLimits,
) -> Result<GraphStateRootDelta, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    validate_current_root(coordinator, base)?;
    let state = coordinator.reducer_state_for_checkpoint()?;
    let snapshot = state.current_snapshot();
    if snapshot.scope() != transaction.scope()
        || snapshot.revision() != Some(base.root.revision())
        || GraphState::logical_state_digest(snapshot)
            .map_err(|_| GraphDiskError::RootStateMismatch)?
            != *base.root.logical_state_digest()
    {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let prepared = state.prepare_transaction(transaction, revision)?;
    build_graph_state_root_delta(snapshot, base.root.anchor(), prepared, limits)
}

/// Merge a previously prepared graph-state plan after its transaction has durably committed.
///
/// All merged runs remain invisible until their exact descriptors have been compared with the
/// live reducer and a certificate-bound root is atomically published. An error never rolls back or
/// weakens the already-authoritative journal commit. An in-process caller retaining the borrowed
/// plan may retry; after process loss, rebuild the cache from recovered live state.
pub fn publish_graph_state_root_delta<F, W, E, I>(
    coordinator: &mut CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    base: &DerivedGraphStateRoot,
    plan: &GraphStateRootDelta,
    outcome: TransactionOutcome,
    limits: GraphStateRootMergeLimits,
) -> Result<(DurableIndexRoot, GraphStateRootMergeReport), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if base.root.anchor() != plan.base_anchor
        || base.root.revision() != plan.base_revision
        || outcome.revision != plan.revision
        || outcome.result_digest != plan.result_digest
    {
        return Err(GraphDiskError::RootStateMismatch);
    }

    let snapshot = coordinator
        .reducer_state_for_checkpoint()?
        .current_snapshot();
    if snapshot.scope() != plan.scope || snapshot.revision() != Some(plan.revision) {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let (anchor_revision, certificate_digest) = coordinator
        .checkpoint_anchor()?
        .ok_or(GraphDiskError::RootStateMismatch)?;
    if anchor_revision != plan.revision {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let logical_state_digest = GraphState::logical_state_digest(snapshot)
        .map_err(|_| GraphDiskError::RootStateMismatch)?;
    let expected = expected_runs(snapshot, plan.revision)?;

    let mut report = GraphStateRootMergeReport::default();
    let mut runs = Vec::with_capacity(expected.len());
    for (index, deltas) in plan.families.iter().enumerate() {
        let family = u8::try_from(index + 1).map_err(|_| GraphDiskError::IndexCorrupt)?;
        let base_has_family = base.root.runs().any(|run| run.family() == family);
        let merged = coordinator.merge_index_run(
            filesystem,
            plan.revision,
            GRAPH_STATE_PROFILE_V1,
            family,
            base_has_family.then_some(&base.root),
            limits.families[index],
            deltas.iter().cloned().map(Ok),
        )?;
        let expected_run = expected.iter().find(|run| run.family == family).copied();
        match (merged.run, expected_run) {
            (Some(run), Some(expected_run)) if expected_run.matches(&run) => {
                runs.push(run);
                report.runs = checked_sum(report.runs, 1)?;
            }
            (None, None) => {}
            _ => return Err(GraphDiskError::IndexCorrupt),
        }
        add_merge_report(&mut report, &merged.report)?;
    }
    if report.deltas != plan.deltas || report.delta_logical_bytes != plan.logical_bytes {
        return Err(GraphDiskError::IndexCorrupt);
    }

    let root = coordinator.publish_index_root(
        filesystem,
        IndexRootInput {
            scope: plan.scope,
            revision: plan.revision,
            certificate_digest,
            reducer_profile: GraphState::REDUCER_PROFILE,
            logical_state_digest,
            index_profile: GRAPH_STATE_PROFILE_V1,
        },
        &runs,
    )?;
    Ok((root, report))
}

type DeltaMap = BTreeMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)>;
type FamilyEntryMap = BTreeMap<Vec<u8>, Vec<u8>>;

#[derive(Default)]
struct DeltaBudget {
    deltas: u64,
    logical_bytes: u64,
}

fn build_graph_state_root_delta(
    snapshot: &GraphSnapshot,
    base_anchor: IndexRootAnchor,
    prepared: PreparedGraph,
    limits: GraphStateDeltaLimits,
) -> Result<GraphStateRootDelta, GraphDiskError> {
    if prepared.scope != snapshot.scope() || prepared.base_revision != snapshot.revision() {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let mandatory_deltas = count(prepared.changes.len())?
        .checked_mul(2)
        .and_then(|value| value.checked_add(1))
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    if mandatory_deltas > limits.maximum_deltas {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }

    let mut families: [DeltaMap; FAMILY_COUNT as usize] = core::array::from_fn(|_| BTreeMap::new());
    let mut budget = DeltaBudget::default();
    for change in &prepared.changes {
        if change.after.id() != change.id
            || change.after.modified_revision() != prepared.revision
            || snapshot.records.get(&change.id) != change.before.as_ref()
        {
            return Err(GraphDiskError::IndexCorrupt);
        }

        insert_delta_side(
            &mut families[usize::from(FAMILY_CURRENT_RECORD - 1)],
            change.id.record().as_bytes().to_vec(),
            change
                .before
                .as_ref()
                .map(encode_stored_record)
                .transpose()?,
            Some(encode_stored_record(&change.after)?),
            &mut budget,
            limits,
        )?;
        insert_delta_side(
            &mut families[usize::from(FAMILY_RECORD_HISTORY - 1)],
            history_key(change.id, prepared.revision),
            None,
            Some(encode_stored_record(&change.after)?),
            &mut budget,
            limits,
        )?;

        let before = change
            .before
            .as_ref()
            .map(record_secondary_entries)
            .transpose()?
            .unwrap_or_else(|| core::array::from_fn(|_| BTreeMap::new()));
        let after = record_secondary_entries(&change.after)?;
        for offset in 0..4 {
            let family = usize::from(FAMILY_OUTGOING - 1) + offset;
            for (key, value) in &before[offset] {
                insert_delta_side(
                    &mut families[family],
                    key.clone(),
                    Some(value.clone()),
                    after[offset].get(key).cloned(),
                    &mut budget,
                    limits,
                )?;
            }
            for (key, value) in &after[offset] {
                if !before[offset].contains_key(key) {
                    insert_delta_side(
                        &mut families[family],
                        key.clone(),
                        None,
                        Some(value.clone()),
                        &mut budget,
                        limits,
                    )?;
                }
            }
        }
    }

    if let Some(policy) = &prepared.policy_change {
        let before = snapshot
            .policy
            .as_ref()
            .map(|policy| encode_result_policy(Some(policy)))
            .transpose()?;
        let after = encode_result_policy(Some(policy))?;
        let policy_family = &mut families[usize::from(FAMILY_POLICY - 1)];
        insert_delta_side(
            policy_family,
            vec![0],
            before,
            Some(after.clone()),
            &mut budget,
            limits,
        )?;
        let mut history_key = Vec::with_capacity(9);
        history_key.push(1);
        history_key.extend_from_slice(&prepared.revision.get().to_be_bytes());
        insert_delta_side(
            policy_family,
            history_key,
            None,
            Some(after),
            &mut budget,
            limits,
        )?;
    }

    let base_counts = metadata_counts(snapshot)?;
    let mut target_counts = base_counts;
    for family in FAMILY_CURRENT_RECORD..=FAMILY_REVERSE {
        for (before, after) in families[usize::from(family - 1)].values() {
            apply_count_delta(
                &mut target_counts[usize::from(family - FAMILY_CURRENT_RECORD)],
                before.is_some(),
                after.is_some(),
            )?;
        }
    }
    for (key, (before, after)) in &families[usize::from(FAMILY_POLICY - 1)] {
        let count_index = match key.first() {
            Some(0) => 7,
            Some(1) => 6,
            _ => return Err(GraphDiskError::IndexCorrupt),
        };
        apply_count_delta(
            &mut target_counts[count_index],
            before.is_some(),
            after.is_some(),
        )?;
    }
    insert_delta_side(
        &mut families[usize::from(FAMILY_METADATA - 1)],
        b"graph-state-v1".to_vec(),
        Some(metadata_value_from_counts(
            prepared
                .base_revision
                .ok_or(GraphDiskError::RootStateMismatch)?,
            base_counts,
        )),
        Some(metadata_value_from_counts(prepared.revision, target_counts)),
        &mut budget,
        limits,
    )?;

    let mut encoded_families: [Vec<IndexDelta>; FAMILY_COUNT as usize] =
        core::array::from_fn(|_| Vec::new());
    let mut deltas = 0_u64;
    let mut logical_bytes = 0_u64;
    for (index, family) in families.into_iter().enumerate() {
        let output = &mut encoded_families[index];
        output
            .try_reserve(family.len())
            .map_err(|_| GraphDiskError::Storage(StorageError::ResourceLimit))?;
        for (key, (before, after)) in family {
            if before == after {
                continue;
            }
            let before_bytes = before.as_ref().map_or(Ok(0), |value| count(value.len()))?;
            let after_bytes = after.as_ref().map_or(Ok(0), |value| count(value.len()))?;
            let bytes = count(key.len())?
                .checked_add(before_bytes)
                .and_then(|value| value.checked_add(after_bytes))
                .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
            deltas = checked_sum(deltas, 1)?;
            logical_bytes = checked_sum(logical_bytes, bytes)?;
            output.push(IndexDelta::new(key, before, after)?);
        }
    }
    if deltas != budget.deltas || logical_bytes != budget.logical_bytes {
        return Err(GraphDiskError::IndexCorrupt);
    }

    Ok(GraphStateRootDelta {
        scope: prepared.scope,
        base_anchor,
        base_revision: prepared
            .base_revision
            .ok_or(GraphDiskError::RootStateMismatch)?,
        revision: prepared.revision,
        result_digest: prepared.result_digest,
        families: encoded_families,
        deltas,
        logical_bytes,
    })
}

fn insert_delta_side(
    family: &mut DeltaMap,
    key: Vec<u8>,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
    budget: &mut DeltaBudget,
    limits: GraphStateDeltaLimits,
) -> Result<(), GraphDiskError> {
    if before == after {
        return Ok(());
    }
    if family.contains_key(&key) {
        return Err(GraphDiskError::IndexCorrupt);
    }
    charge_delta_budget(budget, &key, before.as_deref(), after.as_deref(), limits)?;
    family.insert(key, (before, after));
    Ok(())
}

fn charge_delta_budget(
    budget: &mut DeltaBudget,
    key: &[u8],
    before: Option<&[u8]>,
    after: Option<&[u8]>,
    limits: GraphStateDeltaLimits,
) -> Result<(), GraphDiskError> {
    let new_bytes = delta_logical_bytes(key, before, after)?.ok_or(GraphDiskError::IndexCorrupt)?;
    let deltas = budget
        .deltas
        .checked_add(1)
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    let logical_bytes = budget
        .logical_bytes
        .checked_add(new_bytes)
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    if deltas > limits.maximum_deltas || logical_bytes > limits.maximum_logical_bytes {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    budget.deltas = deltas;
    budget.logical_bytes = logical_bytes;
    Ok(())
}

fn delta_logical_bytes(
    key: &[u8],
    before: Option<&[u8]>,
    after: Option<&[u8]>,
) -> Result<Option<u64>, GraphDiskError> {
    if before == after {
        return Ok(None);
    }
    let before_bytes = before.map_or(Ok(0), |value| count(value.len()))?;
    let after_bytes = after.map_or(Ok(0), |value| count(value.len()))?;
    let bytes = count(key.len())?
        .checked_add(before_bytes)
        .and_then(|value| value.checked_add(after_bytes))
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    Ok(Some(bytes))
}

fn record_secondary_entries(record: &Record) -> Result<[FamilyEntryMap; 4], GraphDiskError> {
    let mut families = core::array::from_fn(|_| BTreeMap::new());
    match record {
        Record::Relationship(relationship) => {
            if relationship.status == crate::AssertionStatus::Accepted {
                families[0].insert(
                    pair_key(relationship.from, relationship.id),
                    relationship.to.record().as_bytes().to_vec(),
                );
                families[1].insert(
                    pair_key(relationship.to, relationship.id),
                    relationship.from.record().as_bytes().to_vec(),
                );
            }
            for evidence in &relationship.evidence {
                if families[2]
                    .insert(pair_key(*evidence, relationship.id), Vec::new())
                    .is_some()
                {
                    return Err(GraphDiskError::IndexCorrupt);
                }
            }
        }
        Record::Assertion(assertion) => {
            for evidence in &assertion.evidence {
                if families[2]
                    .insert(pair_key(*evidence, assertion.id), Vec::new())
                    .is_some()
                {
                    return Err(GraphDiskError::IndexCorrupt);
                }
            }
        }
        Record::Entity(_) | Record::Evidence(_) => {}
    }
    for (target, reference) in record_reverse_references(record) {
        if families[3]
            .insert(pair_key(target, record.id()), reverse_value(reference))
            .is_some()
        {
            return Err(GraphDiskError::IndexCorrupt);
        }
    }
    Ok(families)
}

fn apply_count_delta(count: &mut u64, before: bool, after: bool) -> Result<(), GraphDiskError> {
    *count = match (before, after) {
        (false, true) => count.checked_add(1),
        (true, false) => count.checked_sub(1),
        _ => Some(*count),
    }
    .ok_or(GraphDiskError::IndexCorrupt)?;
    Ok(())
}

fn add_merge_report(
    total: &mut GraphStateRootMergeReport,
    next: &IndexRunMergeReport,
) -> Result<(), GraphDiskError> {
    total.base_entries = checked_sum(total.base_entries, next.base.entries)?;
    total.base_logical_bytes = checked_sum(total.base_logical_bytes, next.base.logical_bytes)?;
    total.deltas = checked_sum(total.deltas, next.deltas)?;
    total.delta_logical_bytes = checked_sum(total.delta_logical_bytes, next.delta_logical_bytes)?;
    total.insertions = checked_sum(total.insertions, next.insertions)?;
    total.replacements = checked_sum(total.replacements, next.replacements)?;
    total.deletions = checked_sum(total.deletions, next.deletions)?;
    total.output_entries = checked_sum(total.output_entries, next.output_entries)?;
    total.output_logical_bytes =
        checked_sum(total.output_logical_bytes, next.output_logical_bytes)?;
    total.pages_read = checked_sum(total.pages_read, next.base.stats.pages_read)?;
    Ok(())
}

fn checked_sum(left: u64, right: u64) -> Result<u64, GraphDiskError> {
    left.checked_add(right).ok_or(GraphDiskError::IndexCorrupt)
}

pub fn publish_graph_state_root<F, W, E, I>(
    coordinator: &mut CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    snapshot: &GraphSnapshot,
) -> Result<DurableIndexRoot, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    validate_snapshot_is_current(coordinator, snapshot)?;
    let revision = snapshot
        .revision()
        .ok_or(GraphDiskError::SnapshotHasNoRevision)?;
    let (anchor_revision, certificate_digest) = coordinator
        .checkpoint_anchor()?
        .ok_or(GraphDiskError::SnapshotHasNoRevision)?;
    if anchor_revision != revision {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let logical_state_digest = GraphState::logical_state_digest(snapshot)
        .map_err(|_| GraphDiskError::RootStateMismatch)?;
    let expected = expected_runs(snapshot, revision)?;
    let mut runs = Vec::with_capacity(expected.len());
    for expected_run in &expected {
        let run = coordinator.publish_index_run_fallible(
            filesystem,
            revision,
            GRAPH_STATE_PROFILE_V1,
            expected_run.family,
            family_entries(snapshot, revision, expected_run.family)?,
        )?;
        if !expected_run.matches(&run) {
            return Err(GraphDiskError::IndexCorrupt);
        }
        runs.push(run);
    }
    Ok(coordinator.publish_index_root(
        filesystem,
        IndexRootInput {
            scope: snapshot.scope(),
            revision,
            certificate_digest,
            reducer_profile: GraphState::REDUCER_PROFILE,
            logical_state_digest,
            index_profile: GRAPH_STATE_PROFILE_V1,
        },
        &runs,
    )?)
}

pub fn load_graph_state_roots<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    snapshot: &GraphSnapshot,
) -> Result<Vec<DerivedGraphStateRoot>, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    validate_snapshot_is_current(coordinator, snapshot)?;
    let revision = snapshot
        .revision()
        .ok_or(GraphDiskError::SnapshotHasNoRevision)?;
    let digest = GraphState::logical_state_digest(snapshot)
        .map_err(|_| GraphDiskError::RootStateMismatch)?;
    let (anchor_revision, certificate_digest) = coordinator
        .checkpoint_anchor()?
        .ok_or(GraphDiskError::SnapshotHasNoRevision)?;
    if anchor_revision != revision {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let expected = expected_runs(snapshot, revision)?;
    let mut cache = PageCache::default();
    let mut admitted = Vec::new();
    for root in coordinator.load_index_roots(filesystem, GRAPH_STATE_PROFILE_V1)? {
        if root.revision() == revision
            && root.certificate_digest() == &certificate_digest
            && root.reducer_profile() == &GraphState::REDUCER_PROFILE
            && root.logical_state_digest() == &digest
            && runs_match(root.runs(), &expected)
        {
            match coordinator.scrub_index_root(filesystem, &root, &mut cache) {
                Ok(_) => admitted.push(DerivedGraphStateRoot { root }),
                Err(TransactionError::Storage(
                    StorageError::IntegrityFailure | StorageError::UnsupportedProfile,
                )) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(admitted)
}

/// Discover authenticated roots on the journal certificate chain without comparing them to the
/// already-live reducer. Returned handles remain candidates until full semantic reconstruction.
pub fn load_graph_state_root_candidates<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
) -> Result<Vec<GraphStateRootCandidate>, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    load_candidates(coordinator, filesystem)
}

pub fn load_graph_state_root_candidates_for_recovery<F, W, E, I>(
    recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
    filesystem: &mut F,
) -> Result<Vec<GraphStateRootCandidate>, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    load_candidates(recovery, filesystem)
}

fn load_candidates<F, R>(
    reader: &R,
    filesystem: &mut F,
) -> Result<Vec<GraphStateRootCandidate>, GraphDiskError>
where
    F: OwnershipFileSystem,
    R: GraphStateIndexReader<F>,
{
    Ok(reader
        .reader_load_roots(filesystem, GRAPH_STATE_PROFILE_V1)?
        .into_iter()
        .filter(|root| root.reducer_profile() == &GraphState::REDUCER_PROFILE)
        .map(|root| GraphStateRootCandidate { root })
        .collect())
}

/// Reconstruct a privately staged graph state from a candidate and return it only after every run,
/// persisted invariant, derived family and logical digest has been verified.
///
/// This removes a monolithic encoded-checkpoint buffer but the returned `GraphState` still owns its
/// complete histories and indexes in memory; it is not the larger-than-memory T-20 endpoint.
pub fn reconstruct_graph_state_candidate<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    candidate: &GraphStateRootCandidate,
    limits: GraphStateLoadLimits,
) -> Result<(GraphState, GraphStateLoadReport), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    reconstruct_with_reader(coordinator, filesystem, candidate, limits)
}

pub fn reconstruct_graph_state_candidate_for_recovery<F, W, E, I>(
    recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
    filesystem: &mut F,
    candidate: &GraphStateRootCandidate,
    limits: GraphStateLoadLimits,
) -> Result<(GraphState, GraphStateLoadReport), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    reconstruct_with_reader(recovery, filesystem, candidate, limits)
}

/// Reconstruct a graph reducer and its exact certificate-paired coordinator metadata while one
/// authenticated recovery owner is held. Drop the owner before passing the returned seed to
/// `CommitCoordinator::open_seeded`, which reopens and verifies the journal prefix independently.
#[allow(clippy::too_many_arguments)]
pub fn reconstruct_graph_recovery_seed<F, W, E, I>(
    recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
    filesystem: &mut F,
    graph_candidate: &GraphStateRootCandidate,
    graph_limits: GraphStateLoadLimits,
    metadata_candidate: &CoordinatorMetadataCandidate,
    metadata_limits: CoordinatorMetadataLoadLimits,
) -> Result<(CoordinatorRecoverySeed<GraphState>, GraphRecoverySeedReport), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if graph_candidate.anchor() != metadata_candidate.anchor() {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let (state, graph_state) =
        reconstruct_with_reader(recovery, filesystem, graph_candidate, graph_limits)?;
    let (seed, coordinator_metadata) = reconstruct_coordinator_metadata_seed_for_recovery(
        recovery,
        filesystem,
        metadata_candidate,
        state,
        metadata_limits,
    )?;
    Ok((
        seed,
        GraphRecoverySeedReport {
            graph_state,
            coordinator_metadata,
        },
    ))
}

fn reconstruct_with_reader<F, R>(
    reader: &R,
    filesystem: &mut F,
    candidate: &GraphStateRootCandidate,
    limits: GraphStateLoadLimits,
) -> Result<(GraphState, GraphStateLoadReport), GraphDiskError>
where
    F: OwnershipFileSystem,
    R: GraphStateIndexReader<F>,
{
    if candidate.root.reducer_profile() != &GraphState::REDUCER_PROFILE
        || candidate.root.index_profile() != &GRAPH_STATE_PROFILE_V1
        || candidate.root.scope() != reader.reader_scope()
    {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let metadata_run = candidate
        .root
        .runs()
        .find(|run| run.family() == FAMILY_METADATA)
        .ok_or(GraphDiskError::IndexCorrupt)?;
    if metadata_run.entry_count() != 1 {
        return Err(GraphDiskError::IndexCorrupt);
    }

    let mut budget = LoadBudget::new(limits);
    let mut metadata = None;
    visit_candidate_family(
        reader,
        filesystem,
        candidate,
        FAMILY_METADATA,
        1,
        &mut budget,
        &mut |key, value| {
            if metadata.is_some() || key != b"graph-state-v1" {
                return Err(StorageError::IntegrityFailure);
            }
            metadata = Some(parse_metadata(value, candidate.revision())?);
            Ok(())
        },
    )?;
    let metadata = metadata.ok_or(GraphDiskError::IndexCorrupt)?;
    let family_counts = metadata.family_counts()?;
    validate_candidate_shape(candidate, metadata, &family_counts, limits)?;

    let scope = candidate.root.scope();
    let revision = candidate.revision();
    let mut records = BTreeMap::new();
    if family_counts[usize::from(FAMILY_CURRENT_RECORD - 1)] != 0 {
        visit_candidate_family(
            reader,
            filesystem,
            candidate,
            FAMILY_CURRENT_RECORD,
            metadata.current,
            &mut budget,
            &mut |key, value| {
                let id = record_key(scope, key)?;
                let record = decode_stored_record(value).map_err(codec_storage_error)?;
                if record.id() != id
                    || record.modified_revision() > revision
                    || records.insert(id, record).is_some()
                {
                    return Err(StorageError::IntegrityFailure);
                }
                Ok(())
            },
        )?;
    }

    let mut history: BTreeMap<RecordRef, Vec<Record>> = BTreeMap::new();
    if family_counts[usize::from(FAMILY_RECORD_HISTORY - 1)] != 0 {
        visit_candidate_family(
            reader,
            filesystem,
            candidate,
            FAMILY_RECORD_HISTORY,
            metadata.history,
            &mut budget,
            &mut |key, value| {
                if key.len() != 24 {
                    return Err(StorageError::IntegrityFailure);
                }
                let id = record_key(scope, &key[..16])?;
                let key_revision = read_u64_be(&key[16..])?;
                let record = decode_stored_record(value).map_err(codec_storage_error)?;
                if record.id() != id || record.modified_revision().get() != key_revision {
                    return Err(StorageError::IntegrityFailure);
                }
                history.entry(id).or_default().push(record);
                Ok(())
            },
        )?;
    }

    let mut policy = None;
    let mut policy_history = BTreeMap::new();
    let policy_entries = family_counts[usize::from(FAMILY_POLICY - 1)];
    if policy_entries != 0 {
        visit_candidate_family(
            reader,
            filesystem,
            candidate,
            FAMILY_POLICY,
            policy_entries,
            &mut budget,
            &mut |key, value| {
                let decoded = decode_result_policy(value).map_err(codec_storage_error)?;
                match key {
                    [0] => {
                        let decoded = decoded.ok_or(StorageError::IntegrityFailure)?;
                        if metadata.current_policy != 1 || policy.replace(decoded).is_some() {
                            return Err(StorageError::IntegrityFailure);
                        }
                    }
                    [1, revision_bytes @ ..] if revision_bytes.len() == 8 => {
                        let policy_revision = CommitRevision::new(read_u64_be(revision_bytes)?)
                            .map_err(|_| StorageError::IntegrityFailure)?;
                        let decoded = decoded.ok_or(StorageError::IntegrityFailure)?;
                        if policy_history.insert(policy_revision, decoded).is_some() {
                            return Err(StorageError::IntegrityFailure);
                        }
                    }
                    _ => return Err(StorageError::IntegrityFailure),
                }
                Ok(())
            },
        )?;
    }

    let state = GraphState::from_persisted_parts_with_derived_counts(
        scope,
        revision,
        records,
        history,
        policy,
        policy_history,
        [
            metadata.outgoing,
            metadata.incoming,
            metadata.provenance,
            metadata.reverse,
        ],
    )
    .map_err(checkpoint_state_disk_error)?;
    let snapshot = state.current_snapshot();

    for family in [
        FAMILY_OUTGOING,
        FAMILY_INCOMING,
        FAMILY_PROVENANCE,
        FAMILY_REVERSE,
    ] {
        let expected_count = family_counts[usize::from(family - 1)];
        let mut expected = family_entries(snapshot, revision, family)?;
        if expected_count == 0 {
            if expected.next().is_some() {
                return Err(GraphDiskError::IndexCorrupt);
            }
            continue;
        }
        visit_candidate_family(
            reader,
            filesystem,
            candidate,
            family,
            expected_count,
            &mut budget,
            &mut |key, value| {
                let expected = expected.next().ok_or(StorageError::IntegrityFailure)??;
                if expected.key != key || expected.value != value {
                    return Err(StorageError::IntegrityFailure);
                }
                Ok(())
            },
        )?;
        if expected.next().is_some() {
            return Err(GraphDiskError::IndexCorrupt);
        }
    }

    let logical_digest =
        GraphState::logical_state_digest(snapshot).map_err(checkpoint_state_disk_error)?;
    let expected = expected_runs(snapshot, revision)?;
    if logical_digest != *candidate.root.logical_state_digest()
        || !runs_match(candidate.root.runs(), &expected)
    {
        return Err(GraphDiskError::RootStateMismatch);
    }
    Ok((state, budget.report))
}

pub fn scrub_graph_state_root<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    root: &DerivedGraphStateRoot,
    cache: &mut PageCache,
) -> Result<IndexScrubReport, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    validate_current_root(coordinator, root)?;
    Ok(coordinator.scrub_index_root(filesystem, &root.root, cache)?)
}

#[derive(Clone, Copy)]
struct GraphStateMetadata {
    current: u64,
    history: u64,
    outgoing: u64,
    incoming: u64,
    provenance: u64,
    reverse: u64,
    policy_history: u64,
    current_policy: u64,
}

impl GraphStateMetadata {
    fn family_counts(self) -> Result<[u64; 8], GraphDiskError> {
        let policy = self
            .policy_history
            .checked_add(self.current_policy)
            .ok_or(GraphDiskError::IndexCorrupt)?;
        Ok([
            1,
            self.current,
            self.history,
            self.outgoing,
            self.incoming,
            self.provenance,
            self.reverse,
            policy,
        ])
    }
}

struct LoadBudget {
    maximum_pages: u64,
    maximum_logical_bytes: u64,
    report: GraphStateLoadReport,
}

impl LoadBudget {
    const fn new(limits: GraphStateLoadLimits) -> Self {
        Self {
            maximum_pages: limits.maximum_total_pages,
            maximum_logical_bytes: limits.maximum_logical_bytes,
            report: GraphStateLoadReport {
                runs: 0,
                entries: 0,
                logical_bytes: 0,
                pages_read: 0,
            },
        }
    }

    fn remaining_logical_bytes(&self) -> Result<u64, GraphDiskError> {
        self.maximum_logical_bytes
            .checked_sub(self.report.logical_bytes)
            .filter(|remaining| *remaining != 0)
            .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))
    }

    fn remaining_pages(&self) -> Result<u64, GraphDiskError> {
        self.maximum_pages
            .checked_sub(self.report.pages_read)
            .filter(|remaining| *remaining != 0)
            .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))
    }

    fn add(&mut self, run: &IndexRunReadReport) -> Result<(), GraphDiskError> {
        self.report.runs = self
            .report
            .runs
            .checked_add(1)
            .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
        self.report.entries = self
            .report
            .entries
            .checked_add(run.entries)
            .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
        self.report.logical_bytes = self
            .report
            .logical_bytes
            .checked_add(run.logical_bytes)
            .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
        self.report.pages_read = self
            .report
            .pages_read
            .checked_add(run.stats.pages_read)
            .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
        if self.report.logical_bytes > self.maximum_logical_bytes {
            return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
        }
        if self.report.pages_read > self.maximum_pages {
            return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
        }
        Ok(())
    }
}

fn visit_candidate_family<F, R>(
    reader: &R,
    filesystem: &mut F,
    candidate: &GraphStateRootCandidate,
    family: u8,
    expected_entries: u64,
    budget: &mut LoadBudget,
    visitor: &mut IndexRunVisitor<'_>,
) -> Result<(), GraphDiskError>
where
    F: OwnershipFileSystem,
    R: GraphStateIndexReader<F>,
{
    let remaining = budget.remaining_logical_bytes()?;
    let remaining_pages = budget.remaining_pages()?;
    let limits = IndexRunReadLimits::new(
        remaining_pages.min(MAX_INDEX_PAGES_PER_RUN),
        expected_entries,
        remaining.min(MAX_INDEX_RUN_LOGICAL_BYTES),
    )?;
    let report = reader.reader_visit_run(filesystem, &candidate.root, family, limits, visitor)?;
    if report.entries != expected_entries {
        return Err(GraphDiskError::IndexCorrupt);
    }
    budget.add(&report)
}

fn parse_metadata(
    value: &[u8],
    expected_revision: CommitRevision,
) -> Result<GraphStateMetadata, StorageError> {
    if value.len() != 80
        || &value[..4] != b"UGSM"
        || value[4] != 1
        || value[5] != 0
        || value[6..8] != [0, 0]
        || read_u64_be(&value[8..16])? != expected_revision.get()
    {
        return Err(StorageError::IntegrityFailure);
    }
    let mut counts = [0_u64; 8];
    for (index, count) in counts.iter_mut().enumerate() {
        let start = 16 + index * 8;
        *count = read_u64_be(&value[start..start + 8])?;
    }
    Ok(GraphStateMetadata {
        current: counts[0],
        history: counts[1],
        outgoing: counts[2],
        incoming: counts[3],
        provenance: counts[4],
        reverse: counts[5],
        policy_history: counts[6],
        current_policy: counts[7],
    })
}

fn validate_candidate_shape(
    candidate: &GraphStateRootCandidate,
    metadata: GraphStateMetadata,
    family_counts: &[u64; 8],
    limits: GraphStateLoadLimits,
) -> Result<(), GraphDiskError> {
    if metadata.current_policy > 1
        || (metadata.policy_history != 0 && metadata.current_policy != 1)
        || family_counts
            .iter()
            .any(|count| *count > MAX_INDEX_ENTRIES_PER_RUN)
    {
        return Err(GraphDiskError::IndexCorrupt);
    }
    let total = family_counts.iter().try_fold(0_u64, |total, count| {
        total
            .checked_add(*count)
            .ok_or(GraphDiskError::IndexCorrupt)
    })?;
    let total_pages = candidate.root.runs().try_fold(0_u64, |total, run| {
        total
            .checked_add(run.page_count())
            .ok_or(GraphDiskError::IndexCorrupt)
    })?;
    let expected = family_counts
        .iter()
        .enumerate()
        .filter(|(_, count)| **count != 0)
        .map(|(index, count)| (u8::try_from(index + 1).unwrap(), *count));
    if !candidate
        .root
        .runs()
        .map(|run| (run.family(), run.entry_count()))
        .eq(expected)
    {
        return Err(GraphDiskError::IndexCorrupt);
    }
    if metadata.current > limits.maximum_records
        || metadata.history > limits.maximum_versions
        || metadata.policy_history > limits.maximum_policy_history
        || total > limits.maximum_total_entries
        || total_pages > limits.maximum_total_pages
    {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    usize::try_from(metadata.current)
        .and_then(|_| usize::try_from(metadata.history))
        .and_then(|_| usize::try_from(metadata.policy_history))
        .map_err(|_| GraphDiskError::Storage(StorageError::ResourceLimit))?;
    Ok(())
}

fn record_key(scope: uste_types::NamespaceRef, key: &[u8]) -> Result<RecordRef, StorageError> {
    let bytes: [u8; 16] = key.try_into().map_err(|_| StorageError::IntegrityFailure)?;
    Ok(RecordRef::new(
        scope.database(),
        scope.namespace(),
        RecordId::from_bytes(bytes),
    ))
}

fn read_u64_be(bytes: &[u8]) -> Result<u64, StorageError> {
    Ok(u64::from_be_bytes(
        bytes
            .try_into()
            .map_err(|_| StorageError::IntegrityFailure)?,
    ))
}

fn checkpoint_state_disk_error(error: CheckpointStateError) -> GraphDiskError {
    match error {
        CheckpointStateError::ResourceLimit => GraphDiskError::Storage(StorageError::ResourceLimit),
        CheckpointStateError::Invalid | CheckpointStateError::UnsupportedProfile => {
            GraphDiskError::IndexCorrupt
        }
    }
}

fn validate_current_root<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    root: &DerivedGraphStateRoot,
) -> Result<(), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let Some((revision, certificate_digest)) = coordinator.checkpoint_anchor()? else {
        return Err(GraphDiskError::RootStateMismatch);
    };
    if root.root.revision() != revision
        || root.root.certificate_digest() != &certificate_digest
        || root.root.reducer_profile() != &GraphState::REDUCER_PROFILE
        || root.root.index_profile() != &GRAPH_STATE_PROFILE_V1
    {
        return Err(GraphDiskError::RootStateMismatch);
    }
    Ok(())
}

fn validate_snapshot_is_current<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    supplied: &GraphSnapshot,
) -> Result<(), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let current = coordinator
        .reducer_state_for_checkpoint()?
        .current_snapshot();
    if supplied.scope() != current.scope()
        || supplied.revision() != current.revision()
        || GraphState::logical_state_digest(supplied)
            .map_err(|_| GraphDiskError::RootStateMismatch)?
            != GraphState::logical_state_digest(current)
                .map_err(|_| GraphDiskError::RootStateMismatch)?
    {
        return Err(GraphDiskError::RootStateMismatch);
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct ExpectedRun {
    family: u8,
    entry_count: u64,
    logical_digest: [u8; 32],
}

impl ExpectedRun {
    fn matches(self, actual: &IndexRunDescriptor) -> bool {
        actual.family() == self.family
            && actual.entry_count() == self.entry_count
            && actual.logical_digest() == &self.logical_digest
    }
}

fn expected_runs(
    snapshot: &GraphSnapshot,
    revision: CommitRevision,
) -> Result<Vec<ExpectedRun>, GraphDiskError> {
    let mut expected = Vec::with_capacity(usize::from(FAMILY_COUNT));
    for family in 1..=FAMILY_COUNT {
        let mut builder = ExpectedRunBuilder::new(snapshot, revision, family);
        for entry in family_entries(snapshot, revision, family)? {
            let entry = entry.map_err(GraphDiskError::Storage)?;
            builder.add(&entry.key, &entry.value)?;
        }
        let run = builder.finish();
        if run.entry_count != 0 {
            expected.push(run);
        }
    }
    Ok(expected)
}

fn runs_match<'a>(
    actual: impl ExactSizeIterator<Item = &'a IndexRunDescriptor>,
    expected: &[ExpectedRun],
) -> bool {
    actual.len() == expected.len()
        && actual
            .zip(expected)
            .all(|(actual, expected)| expected.matches(actual))
}

type EntryIterator<'a> = Box<dyn Iterator<Item = Result<IndexEntry, StorageError>> + 'a>;

fn family_entries<'a>(
    snapshot: &'a GraphSnapshot,
    revision: CommitRevision,
    family: u8,
) -> Result<EntryIterator<'a>, GraphDiskError> {
    let records: &'a BTreeMap<RecordRef, Record> = &snapshot.records;
    let entries: EntryIterator<'a> = match family {
        FAMILY_METADATA => Box::new(core::iter::once(Ok(IndexEntry {
            key: b"graph-state-v1".to_vec(),
            value: metadata_value(snapshot, revision)?,
        }))),
        FAMILY_CURRENT_RECORD => Box::new(snapshot.records.values().map(|record| {
            encode_stored_record(record)
                .map(|value| IndexEntry {
                    key: record.id().record().as_bytes().to_vec(),
                    value,
                })
                .map_err(codec_storage_error)
        })),
        FAMILY_RECORD_HISTORY => Box::new(snapshot.history.iter().flat_map(|(id, versions)| {
            versions.iter().map(move |record| {
                encode_stored_record(record)
                    .map(|value| IndexEntry {
                        key: history_key(*id, record.modified_revision()),
                        value,
                    })
                    .map_err(codec_storage_error)
            })
        })),
        FAMILY_OUTGOING => Box::new(snapshot.outgoing.iter().flat_map(
            move |(entity, relationships)| {
                relationships.iter().map(move |relationship| {
                    let neighbor = relationship_neighbor(records, *entity, *relationship)?;
                    Ok(IndexEntry {
                        key: pair_key(*entity, *relationship),
                        value: neighbor.record().as_bytes().to_vec(),
                    })
                })
            },
        )),
        FAMILY_INCOMING => Box::new(snapshot.incoming.iter().flat_map(
            move |(entity, relationships)| {
                relationships.iter().map(move |relationship| {
                    let neighbor = relationship_neighbor(records, *entity, *relationship)?;
                    Ok(IndexEntry {
                        key: pair_key(*entity, *relationship),
                        value: neighbor.record().as_bytes().to_vec(),
                    })
                })
            },
        )),
        FAMILY_PROVENANCE => Box::new(snapshot.provenance.iter().flat_map(|(evidence, claims)| {
            claims.iter().map(move |claim| {
                Ok(IndexEntry {
                    key: pair_key(*evidence, *claim),
                    value: Vec::new(),
                })
            })
        })),
        FAMILY_REVERSE => Box::new(snapshot.reverse.iter().flat_map(|(target, owners)| {
            owners.iter().map(move |(owner, reference)| {
                Ok(IndexEntry {
                    key: pair_key(*target, *owner),
                    value: reverse_value(*reference),
                })
            })
        })),
        FAMILY_POLICY => {
            let current = snapshot.policy.iter().map(|policy| {
                encode_result_policy(Some(policy))
                    .map(|value| IndexEntry {
                        key: vec![0],
                        value,
                    })
                    .map_err(codec_storage_error)
            });
            let history = snapshot.policy_history.iter().map(|(revision, policy)| {
                encode_result_policy(Some(policy))
                    .map(|value| {
                        let mut key = Vec::with_capacity(9);
                        key.push(1);
                        key.extend_from_slice(&revision.get().to_be_bytes());
                        IndexEntry { key, value }
                    })
                    .map_err(codec_storage_error)
            });
            Box::new(current.chain(history))
        }
        _ => return Err(GraphDiskError::IndexCorrupt),
    };
    Ok(entries)
}

fn metadata_value(
    snapshot: &GraphSnapshot,
    revision: CommitRevision,
) -> Result<Vec<u8>, GraphDiskError> {
    Ok(metadata_value_from_counts(
        revision,
        metadata_counts(snapshot)?,
    ))
}

fn metadata_counts(snapshot: &GraphSnapshot) -> Result<[u64; 8], GraphDiskError> {
    Ok([
        count(snapshot.records.len())?,
        total(snapshot.history.values().map(Vec::len))?,
        total(snapshot.outgoing.values().map(|values| values.len()))?,
        total(snapshot.incoming.values().map(|values| values.len()))?,
        total(snapshot.provenance.values().map(|values| values.len()))?,
        total(snapshot.reverse.values().map(|values| values.len()))?,
        count(snapshot.policy_history.len())?,
        u64::from(snapshot.policy.is_some()),
    ])
}

fn metadata_value_from_counts(revision: CommitRevision, counts: [u64; 8]) -> Vec<u8> {
    let mut value = Vec::with_capacity(80);
    value.extend_from_slice(b"UGSM");
    value.extend_from_slice(&[1, 0, 0, 0]);
    value.extend_from_slice(&revision.get().to_be_bytes());
    for count in counts {
        value.extend_from_slice(&count.to_be_bytes());
    }
    debug_assert_eq!(value.len(), 80);
    value
}

fn count(value: usize) -> Result<u64, GraphDiskError> {
    u64::try_from(value).map_err(|_| GraphDiskError::IndexCorrupt)
}

fn total(mut values: impl Iterator<Item = usize>) -> Result<u64, GraphDiskError> {
    values.try_fold(0_u64, |total, value| {
        total
            .checked_add(count(value)?)
            .ok_or(GraphDiskError::IndexCorrupt)
    })
}

fn history_key(id: RecordRef, revision: CommitRevision) -> Vec<u8> {
    let mut key = Vec::with_capacity(24);
    key.extend_from_slice(id.record().as_bytes());
    key.extend_from_slice(&revision.get().to_be_bytes());
    key
}

fn pair_key(left: RecordRef, right: RecordRef) -> Vec<u8> {
    let mut key = Vec::with_capacity(32);
    key.extend_from_slice(left.record().as_bytes());
    key.extend_from_slice(right.record().as_bytes());
    key
}

fn reverse_value(reference: ReverseReference) -> Vec<u8> {
    let mut value = Vec::with_capacity(24);
    value.push(reference.owner_kind);
    value.push(reference.owner_state);
    value.extend_from_slice(&reference.roles.to_be_bytes());
    value.extend_from_slice(&reference.owner_version.get().to_be_bytes());
    value.extend_from_slice(&reference.owner_revision.get().to_be_bytes());
    value.extend_from_slice(&[0; 4]);
    value
}

fn relationship_neighbor(
    records: &BTreeMap<RecordRef, Record>,
    entity: RecordRef,
    relationship: RecordRef,
) -> Result<RecordRef, StorageError> {
    let Some(Record::Relationship(record)) = records.get(&relationship) else {
        return Err(StorageError::IntegrityFailure);
    };
    if record.from == entity {
        Ok(record.to)
    } else if record.to == entity {
        Ok(record.from)
    } else {
        Err(StorageError::IntegrityFailure)
    }
}

struct ExpectedRunBuilder {
    family: u8,
    entry_count: u64,
    digest: Sha256,
}

impl ExpectedRunBuilder {
    fn new(snapshot: &GraphSnapshot, revision: CommitRevision, family: u8) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"USTE-INDEX-RUN-V1\0");
        digest.update(snapshot.scope().namespace().as_bytes());
        digest.update(revision.get().to_be_bytes());
        digest.update(GRAPH_STATE_PROFILE_V1);
        digest.update([family]);
        Self {
            family,
            entry_count: 0,
            digest,
        }
    }

    fn add(&mut self, key: &[u8], value: &[u8]) -> Result<(), GraphDiskError> {
        self.entry_count = self
            .entry_count
            .checked_add(1)
            .ok_or(GraphDiskError::IndexCorrupt)?;
        self.digest.update(
            u32::try_from(key.len())
                .map_err(|_| GraphDiskError::IndexCorrupt)?
                .to_be_bytes(),
        );
        self.digest.update(
            u64::try_from(value.len())
                .map_err(|_| GraphDiskError::IndexCorrupt)?
                .to_be_bytes(),
        );
        self.digest.update(key);
        self.digest.update(value);
        Ok(())
    }

    fn finish(self) -> ExpectedRun {
        ExpectedRun {
            family: self.family,
            entry_count: self.entry_count,
            logical_digest: self.digest.finalize().into(),
        }
    }
}

fn codec_storage_error(error: GraphCodecError) -> StorageError {
    if matches!(error, GraphCodecError::ResourceLimit) {
        StorageError::ResourceLimit
    } else {
        StorageError::IntegrityFailure
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AssertionAction, DurablePolicyMutation, Expected, GraphTransaction, NewEntity, NewEvidence,
        NewRecord, NewRelationship, Operation, ValidTime,
    };
    use uste_policy::{NamespacePolicy, PolicyVersion, QuotaLimits};
    use uste_txn::TransactionState;
    use uste_types::{BoundedString, DatabaseId, NamespaceId, NamespaceRef, RecordId, Value};

    fn scope() -> NamespaceRef {
        NamespaceRef::new(
            DatabaseId::from_bytes([0xa1; 16]),
            NamespaceId::from_bytes([0xa2; 16]),
        )
    }

    fn record(value: u8) -> RecordRef {
        RecordRef::new(
            scope().database(),
            scope().namespace(),
            RecordId::from_bytes([value; 16]),
        )
    }

    fn text(value: &str) -> BoundedString {
        BoundedString::new(value.to_owned()).unwrap()
    }

    fn canonical_fixture() -> (GraphSnapshot, [RecordRef; 4]) {
        let ids = [record(1), record(2), record(3), record(4)];
        let mut state = GraphState::new(scope());
        let prepared = state
            .prepare_transaction(
                &GraphTransaction::new(
                    scope(),
                    vec![
                        Operation::Create {
                            expected: Expected::Absent,
                            record: NewRecord::Entity(NewEntity {
                                id: ids[0],
                                entity_type: text("left"),
                                schema_version: 1,
                                properties: Value::Null,
                            }),
                        },
                        Operation::Create {
                            expected: Expected::Absent,
                            record: NewRecord::Entity(NewEntity {
                                id: ids[1],
                                entity_type: text("right"),
                                schema_version: 1,
                                properties: Value::Null,
                            }),
                        },
                        Operation::Create {
                            expected: Expected::Absent,
                            record: NewRecord::Evidence(NewEvidence {
                                id: ids[2],
                                digest: [0x33; 32],
                                locator: text("fixture://state-root"),
                            }),
                        },
                        Operation::Create {
                            expected: Expected::Absent,
                            record: NewRecord::Relationship(NewRelationship {
                                id: ids[3],
                                from: ids[0],
                                to: ids[1],
                                relationship_type: text("edge"),
                                properties: Value::Null,
                                evidence: vec![ids[2]],
                                valid_time: ValidTime::Unknown,
                            }),
                        },
                    ],
                ),
                CommitRevision::FIRST,
            )
            .unwrap();
        TransactionState::publish(&mut state, prepared);
        let prepared = state
            .prepare_transaction(
                &GraphTransaction::new(
                    scope(),
                    vec![Operation::ActOnRelationship {
                        target: ids[3],
                        expected: Expected::Version(crate::RecordVersion::FIRST),
                        action: AssertionAction::Accept,
                        correction: None,
                        correction_expected: None,
                    }],
                ),
                CommitRevision::new(2).unwrap(),
            )
            .unwrap();
        TransactionState::publish(&mut state, prepared);
        (state.snapshot(), ids)
    }

    fn entries(snapshot: &GraphSnapshot, family: u8) -> Vec<IndexEntry> {
        family_entries(snapshot, CommitRevision::new(2).unwrap(), family)
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn canonical_fixture_pins_every_nonempty_family_and_run_digest() {
        let (snapshot, [left, right, evidence, relationship]) = canonical_fixture();
        let revision = CommitRevision::new(2).unwrap();

        let metadata = entries(&snapshot, FAMILY_METADATA);
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].key, b"graph-state-v1");
        assert_eq!(
            &metadata[0].value[..16],
            [b"UGSM\x01\0\0\0".as_slice(), &2_u64.to_be_bytes()].concat()
        );
        assert_eq!(
            &metadata[0].value[16..],
            [4_u64, 5, 1, 1, 1, 3, 0, 0]
                .into_iter()
                .flat_map(u64::to_be_bytes)
                .collect::<Vec<_>>()
        );

        let current = entries(&snapshot, FAMILY_CURRENT_RECORD);
        assert_eq!(
            current
                .iter()
                .map(|entry| entry.key.clone())
                .collect::<Vec<_>>(),
            [&left, &right, &evidence, &relationship].map(|id| id.record().as_bytes().to_vec())
        );
        for entry in &current {
            let id = RecordRef::new(
                scope().database(),
                scope().namespace(),
                RecordId::from_bytes(entry.key.clone().try_into().unwrap()),
            );
            assert_eq!(
                entry.value,
                encode_stored_record(snapshot.record(id).unwrap()).unwrap()
            );
        }

        let history = entries(&snapshot, FAMILY_RECORD_HISTORY);
        assert_eq!(history.len(), 5);
        assert_eq!(
            history
                .iter()
                .map(|entry| entry.key.clone())
                .collect::<Vec<_>>(),
            vec![
                history_key(left, CommitRevision::FIRST),
                history_key(right, CommitRevision::FIRST),
                history_key(evidence, CommitRevision::FIRST),
                history_key(relationship, CommitRevision::FIRST),
                history_key(relationship, revision),
            ]
        );

        assert_eq!(
            entries(&snapshot, FAMILY_OUTGOING),
            vec![IndexEntry {
                key: pair_key(left, relationship),
                value: right.record().as_bytes().to_vec(),
            }]
        );
        assert_eq!(
            entries(&snapshot, FAMILY_INCOMING),
            vec![IndexEntry {
                key: pair_key(right, relationship),
                value: left.record().as_bytes().to_vec(),
            }]
        );
        assert_eq!(
            entries(&snapshot, FAMILY_PROVENANCE),
            vec![IndexEntry {
                key: pair_key(evidence, relationship),
                value: Vec::new(),
            }]
        );
        let reverse = entries(&snapshot, FAMILY_REVERSE);
        assert_eq!(
            reverse
                .iter()
                .map(|entry| entry.key.clone())
                .collect::<Vec<_>>(),
            vec![
                pair_key(left, relationship),
                pair_key(right, relationship),
                pair_key(evidence, relationship),
            ]
        );
        assert_eq!(
            reverse
                .iter()
                .map(|entry| entry.value.clone())
                .collect::<Vec<_>>(),
            [0x0008_u16, 0x0010, 0x0040].map(|roles| {
                reverse_value(ReverseReference {
                    owner_kind: 3,
                    owner_state: 2,
                    roles,
                    owner_version: crate::RecordVersion::new(2).unwrap(),
                    owner_revision: revision,
                })
            })
        );
        assert!(entries(&snapshot, FAMILY_POLICY).is_empty());

        let runs = expected_runs(&snapshot, revision).unwrap();
        assert_eq!(
            runs.iter()
                .map(|run| (run.family, run.entry_count))
                .collect::<Vec<_>>(),
            vec![(1, 1), (2, 4), (3, 5), (4, 1), (5, 1), (6, 1), (7, 3)]
        );
        assert_eq!(
            runs.iter()
                .map(|run| hex(&run.logical_digest))
                .collect::<Vec<_>>(),
            vec![
                "4dbe1e3810ea2d731493955c02f8da4ffc9bf85e7daa28b9b7f21118169788d9",
                "69cd84ba1c43b708efd74276a0ff5b0352273c9c927519860146e49b59e9308d",
                "f6818475c142205696213b8c18f1a48d9688eee627e13baea86938be73b08a35",
                "64d853f62bbcdad7f8dcc69690aeb7ec4d7c3144f1b399ea438290b0ff61f06b",
                "7855bec47713289802d8eb2d8df9be03bce4e36a1a9cf8cc0778130801a94332",
                "d7057241aaa6cfb400566303603cae6fa553053fad96ab516ced8c93637ac8ff",
                "27273b10d66f68ad64d24bf5baa321a444c76dca20c14ae07d6b6e62eac85d59",
            ]
        );
    }

    #[test]
    fn derived_counts_reject_understatement_before_global_index_rebuild() {
        let (snapshot, _) = canonical_fixture();
        assert_eq!(
            GraphState::from_persisted_parts_with_derived_counts(
                snapshot.scope(),
                snapshot.revision().unwrap(),
                snapshot.records.clone(),
                snapshot.history.clone(),
                snapshot.policy.clone(),
                snapshot.policy_history.clone(),
                [0, 1, 1, 3],
            ),
            Err(CheckpointStateError::Invalid)
        );
        let rebuilt = GraphState::from_persisted_parts_with_derived_counts(
            snapshot.scope(),
            snapshot.revision().unwrap(),
            snapshot.records.clone(),
            snapshot.history.clone(),
            snapshot.policy.clone(),
            snapshot.policy_history.clone(),
            [1, 1, 1, 3],
        )
        .unwrap();
        assert_eq!(rebuilt.current_snapshot(), &snapshot);
    }

    #[test]
    fn policy_only_state_has_canonical_metadata_and_policy_family() {
        let mut state = GraphState::new(scope());
        let policy = NamespacePolicy::new(
            scope(),
            PolicyVersion::new(1).unwrap(),
            QuotaLimits::new(1024, 2048, 4096, 2, 1024).unwrap(),
        );
        let prepared = state
            .prepare_transaction(
                &GraphTransaction::with_policy_mutation(
                    scope(),
                    Vec::new(),
                    DurablePolicyMutation::Install { policy },
                ),
                CommitRevision::FIRST,
            )
            .unwrap();
        TransactionState::publish(&mut state, prepared);
        let snapshot = state.snapshot();

        let runs = expected_runs(&snapshot, CommitRevision::FIRST).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].family, FAMILY_METADATA);
        assert_eq!(runs[0].entry_count, 1);
        assert_eq!(runs[1].family, FAMILY_POLICY);
        assert_eq!(runs[1].entry_count, 2);

        let metadata = family_entries(&snapshot, CommitRevision::FIRST, FAMILY_METADATA)
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(metadata.key, b"graph-state-v1");
        assert_eq!(metadata.value.len(), 80);
        assert_eq!(&metadata.value[..8], b"UGSM\x01\0\0\0");
        assert_eq!(&metadata.value[8..16], &1_u64.to_be_bytes());
        assert_eq!(&metadata.value[64..72], &1_u64.to_be_bytes());
        assert_eq!(&metadata.value[72..80], &1_u64.to_be_bytes());

        let policy_entries: Vec<_> =
            family_entries(&snapshot, CommitRevision::FIRST, FAMILY_POLICY)
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
        assert_eq!(policy_entries[0].key, vec![0]);
        assert_eq!(
            policy_entries[1].key,
            [vec![1], 1_u64.to_be_bytes().to_vec()].concat()
        );
        assert_eq!(policy_entries[0].value, policy_entries[1].value);
    }

    #[test]
    fn reverse_descriptor_has_fixed_versioned_field_layout() {
        let encoded = reverse_value(ReverseReference {
            owner_kind: 3,
            owner_state: 6,
            roles: 0x00a5,
            owner_version: crate::RecordVersion::new(9).unwrap(),
            owner_revision: CommitRevision::new(11).unwrap(),
        });
        assert_eq!(encoded.len(), 24);
        assert_eq!(&encoded[..4], &[3, 6, 0, 0xa5]);
        assert_eq!(&encoded[4..12], &9_u64.to_be_bytes());
        assert_eq!(&encoded[12..20], &11_u64.to_be_bytes());
        assert_eq!(&encoded[20..], &[0; 4]);
    }
}
