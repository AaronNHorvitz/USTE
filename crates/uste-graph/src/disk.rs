//! Certificate-anchored current-graph disk projection.
//!
//! These functions are trusted maintenance/raw projection surfaces. Consumer reads still require
//! the authorization facade; possessing a `JournalStore` is already a privileged capability.

use std::{collections::BTreeSet, sync::Mutex};

use sha2::{Digest, Sha256};
use uste_crypto::EntropySource;
use uste_policy::{Action, Target};
use uste_storage::{
    DurableIndexRoot, IndexEntry, IndexReadStats, IndexRootInput, IndexRunDescriptor,
    IndexScrubReport, OwnershipFileSystem, PageCache, RecoveredIndexRoot,
    journal::{DurableKeyEnvelope, StorageError},
};
use uste_txn::{AuthorizedIndexedReadState, CheckpointState, CommitCoordinator, TransactionError};
use uste_types::{RecordId, RecordRef};

use crate::{
    AdjacencyDirection, GraphCodecError, GraphError, GraphNeighbor, GraphReadOutput,
    GraphReadRequest, GraphSnapshot, GraphState, MAX_TRAVERSAL_RESULTS, MAX_TRAVERSAL_VISITS,
    Record, decode_stored_record, encode_stored_record,
    query::{record_references_are_authorized, visible_record},
};

pub const GRAPH_INDEX_PROFILE_V1: [u8; 32] = [
    0x62, 0x6a, 0x8c, 0x10, 0xa8, 0x5d, 0xd7, 0x9f, 0x03, 0x8f, 0x2e, 0xbc, 0xd6, 0x83, 0x33, 0x0b,
    0xd3, 0x1f, 0x48, 0x9d, 0xaa, 0x37, 0x2d, 0xbb, 0xf0, 0xa4, 0x1e, 0x72, 0x23, 0x24, 0x87, 0x12,
];

const FAMILY_METADATA: u8 = 1;
const FAMILY_CURRENT_RECORD: u8 = 2;
const FAMILY_OUTGOING: u8 = 3;
const FAMILY_INCOMING: u8 = 4;
const FAMILY_PROVENANCE: u8 = 5;

/// Opaque graph-specific admission of one storage root against an exact current snapshot.
/// Every read rechecks the live journal frontier, so this handle becomes stale after a commit.
pub struct CurrentGraphIndexRoot {
    root: RecoveredIndexRoot,
}

impl core::fmt::Debug for CurrentGraphIndexRoot {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("CurrentGraphIndexRoot")
            .field("revision", &self.root.revision())
            .field("generation", &self.root.generation())
            .finish_non_exhaustive()
    }
}

impl CurrentGraphIndexRoot {
    #[must_use]
    pub const fn revision(&self) -> uste_types::CommitRevision {
        self.root.revision()
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.root.generation()
    }
}

/// Policy-admitted graph index handle with an internal cache whose access pattern is not exposed
/// to callers. The raw root remains available only through the privileged storage APIs above.
pub struct AuthorizedGraphIndex {
    root: CurrentGraphIndexRoot,
    cache: Mutex<PageCache>,
}

impl core::fmt::Debug for AuthorizedGraphIndex {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AuthorizedGraphIndex")
            .field("revision", &self.root.revision())
            .field("generation", &self.root.generation())
            .finish_non_exhaustive()
    }
}

impl AuthorizedGraphIndex {
    #[must_use]
    pub const fn revision(&self) -> uste_types::CommitRevision {
        self.root.revision()
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.root.generation()
    }

    fn new(root: CurrentGraphIndexRoot) -> Self {
        Self {
            root,
            cache: Mutex::new(PageCache::default()),
        }
    }
}

/// Borrowed opaque resources for one authorization-preserving current-graph disk read.
pub struct GraphDiskReadContext<'a> {
    index: &'a AuthorizedGraphIndex,
}

impl core::fmt::Debug for GraphDiskReadContext<'_> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("GraphDiskReadContext([REDACTED])")
    }
}

impl<'a> GraphDiskReadContext<'a> {
    #[must_use]
    pub const fn new(index: &'a AuthorizedGraphIndex) -> Self {
        Self { index }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum GraphDiskError {
    Storage(StorageError),
    Transaction(TransactionError),
    Codec(GraphCodecError),
    SnapshotHasNoRevision,
    RootStateMismatch,
    IndexCorrupt,
    UnsupportedRequest,
    Graph(GraphError),
}

impl core::fmt::Display for GraphDiskError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for GraphDiskError {}

impl From<StorageError> for GraphDiskError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

impl From<TransactionError> for GraphDiskError {
    fn from(error: TransactionError) -> Self {
        Self::Transaction(error)
    }
}

impl From<GraphCodecError> for GraphDiskError {
    fn from(error: GraphCodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<GraphError> for GraphDiskError {
    fn from(error: GraphError) -> Self {
        Self::Graph(error)
    }
}

/// Build all current graph families and atomically publish a root at the unchanged journal anchor.
/// A failed build leaves only unreferenced rebuildable run files.
pub fn publish_current_graph_index<F, W, E, I>(
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
    let scope = snapshot.scope();
    let logical_state_digest = GraphState::logical_state_digest(snapshot)
        .map_err(|_| GraphDiskError::RootStateMismatch)?;
    let mut runs = Vec::new();
    runs.push(coordinator.publish_index_run(
        filesystem,
        revision,
        GRAPH_INDEX_PROFILE_V1,
        FAMILY_METADATA,
        [IndexEntry {
            key: b"current-graph-v1".to_vec(),
            value: Vec::new(),
        }],
    )?);
    if !snapshot.records.is_empty() {
        let entries = snapshot.records.values().map(|record| {
            encode_stored_record(record)
                .map(|value| IndexEntry {
                    key: record.id().record().as_bytes().to_vec(),
                    value,
                })
                .map_err(codec_storage_error)
        });
        runs.push(coordinator.publish_index_run_fallible(
            filesystem,
            revision,
            GRAPH_INDEX_PROFILE_V1,
            FAMILY_CURRENT_RECORD,
            entries,
        )?);
    }
    if !snapshot.outgoing.is_empty() {
        let entries = snapshot
            .outgoing
            .iter()
            .flat_map(|(entity, relationships)| {
                relationships.iter().map(move |relationship| {
                    let neighbor = relationship_neighbor(&snapshot.records, *entity, *relationship)
                        .map_err(|_| StorageError::IntegrityFailure)?;
                    Ok(IndexEntry {
                        key: pair_key(*entity, *relationship),
                        value: neighbor.record().as_bytes().to_vec(),
                    })
                })
            });
        runs.push(coordinator.publish_index_run_fallible(
            filesystem,
            revision,
            GRAPH_INDEX_PROFILE_V1,
            FAMILY_OUTGOING,
            entries,
        )?);
    }
    if !snapshot.incoming.is_empty() {
        let entries = snapshot
            .incoming
            .iter()
            .flat_map(|(entity, relationships)| {
                relationships.iter().map(move |relationship| {
                    let neighbor = relationship_neighbor(&snapshot.records, *entity, *relationship)
                        .map_err(|_| StorageError::IntegrityFailure)?;
                    Ok(IndexEntry {
                        key: pair_key(*entity, *relationship),
                        value: neighbor.record().as_bytes().to_vec(),
                    })
                })
            });
        runs.push(coordinator.publish_index_run_fallible(
            filesystem,
            revision,
            GRAPH_INDEX_PROFILE_V1,
            FAMILY_INCOMING,
            entries,
        )?);
    }
    if !snapshot.provenance.is_empty() {
        let entries = snapshot.provenance.iter().flat_map(|(evidence, claims)| {
            claims.iter().map(move |claim| IndexEntry {
                key: pair_key(*evidence, *claim),
                value: Vec::new(),
            })
        });
        runs.push(coordinator.publish_index_run(
            filesystem,
            revision,
            GRAPH_INDEX_PROFILE_V1,
            FAMILY_PROVENANCE,
            entries,
        )?);
    }
    runs.sort_by_key(IndexRunDescriptor::family);
    if !root_matches_snapshot(&runs, snapshot, revision)? {
        return Err(GraphDiskError::IndexCorrupt);
    }
    Ok(coordinator.publish_index_root(
        filesystem,
        IndexRootInput {
            scope,
            revision,
            certificate_digest,
            reducer_profile: GraphState::REDUCER_PROFILE,
            logical_state_digest,
            index_profile: GRAPH_INDEX_PROFILE_V1,
        },
        &runs,
    )?)
}

/// Admit only roots for this exact frozen graph state. Older valid roots remain usable only with
/// their matching historical state and are not silently served as current.
pub fn load_current_graph_index_roots<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    snapshot: &GraphSnapshot,
) -> Result<Vec<CurrentGraphIndexRoot>, GraphDiskError>
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
    let mut cache = PageCache::default();
    let mut admitted = Vec::new();
    for root in coordinator.load_index_roots(filesystem, GRAPH_INDEX_PROFILE_V1)? {
        if root.revision() == revision
            && root.certificate_digest() == &certificate_digest
            && root.reducer_profile() == &GraphState::REDUCER_PROFILE
            && root.logical_state_digest() == &digest
            && root_matches_snapshot(
                &root.runs().copied().collect::<Vec<_>>(),
                snapshot,
                revision,
            )?
        {
            match coordinator.scrub_index_root(filesystem, &root, &mut cache) {
                Ok(_) => admitted.push(CurrentGraphIndexRoot { root }),
                Err(TransactionError::Storage(
                    StorageError::IntegrityFailure | StorageError::UnsupportedProfile,
                )) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(admitted)
}

pub fn disk_record<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    root: &CurrentGraphIndexRoot,
    id: RecordRef,
    cache: &mut PageCache,
) -> Result<(Option<Record>, IndexReadStats), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    validate_current_root(coordinator, root)?;
    if !same_scope(id, root.root.scope()) {
        return Err(GraphDiskError::RootStateMismatch);
    }
    if root
        .root
        .runs()
        .all(|run| run.family() != FAMILY_CURRENT_RECORD)
    {
        return Ok((None, IndexReadStats::default()));
    }
    let (value, stats) = coordinator.index_get(
        filesystem,
        &root.root,
        FAMILY_CURRENT_RECORD,
        id.record().as_bytes(),
        cache,
    )?;
    let record = value
        .map(|encoded| decode_stored_record(&encoded))
        .transpose()?;
    if record.as_ref().is_some_and(|record| record.id() != id) {
        return Err(GraphDiskError::IndexCorrupt);
    }
    Ok((record, stats))
}

pub fn disk_adjacent_ids<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    root: &CurrentGraphIndexRoot,
    entity: RecordRef,
    direction: AdjacencyDirection,
    maximum: usize,
    cache: &mut PageCache,
) -> Result<(Vec<(RecordRef, RecordRef)>, IndexReadStats), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    validate_current_root(coordinator, root)?;
    if !same_scope(entity, root.root.scope()) {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let mut combined = BTreeSet::new();
    let mut total = IndexReadStats::default();
    let mut budget =
        AggregateScanBudget::new(MAX_TRAVERSAL_VISITS, uste_storage::MAX_INDEX_RESULT_BYTES);
    for family in match direction {
        AdjacencyDirection::Outgoing => &[FAMILY_OUTGOING][..],
        AdjacencyDirection::Incoming => &[FAMILY_INCOMING][..],
        AdjacencyDirection::Either => &[FAMILY_OUTGOING, FAMILY_INCOMING][..],
    } {
        if root.root.runs().all(|run| run.family() != *family) {
            continue;
        }
        let (remaining_visits, remaining_result_bytes) = budget.remaining();
        let scan = coordinator.index_scan_prefix(
            filesystem,
            &root.root,
            *family,
            entity.record().as_bytes(),
            remaining_visits,
            remaining_result_bytes,
            cache,
        )?;
        budget.account(scan.entries.len(), scan.stats.result_bytes)?;
        add_stats(&mut total, &scan.stats);
        for entry in scan.entries {
            if entry.key.len() != 32
                || &entry.key[..16] != entity.record().as_bytes()
                || entry.value.len() != 16
            {
                return Err(GraphDiskError::IndexCorrupt);
            }
            let relationship = record_ref(&root.root, &entry.key[16..])?;
            let neighbor = record_ref(&root.root, &entry.value)?;
            if !combined.contains(&(relationship, neighbor)) && combined.len() == maximum {
                return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
            }
            combined.insert((relationship, neighbor));
        }
    }
    Ok((combined.into_iter().collect(), total))
}

pub fn disk_supported_ids<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    root: &CurrentGraphIndexRoot,
    evidence: RecordRef,
    maximum: usize,
    cache: &mut PageCache,
) -> Result<(Vec<RecordRef>, IndexReadStats), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    validate_current_root(coordinator, root)?;
    if !same_scope(evidence, root.root.scope()) {
        return Err(GraphDiskError::RootStateMismatch);
    }
    if root
        .root
        .runs()
        .all(|run| run.family() != FAMILY_PROVENANCE)
    {
        return Ok((Vec::new(), IndexReadStats::default()));
    }
    let scan = coordinator.index_scan_prefix(
        filesystem,
        &root.root,
        FAMILY_PROVENANCE,
        evidence.record().as_bytes(),
        maximum,
        uste_storage::MAX_INDEX_RESULT_BYTES,
        cache,
    )?;
    let mut records = Vec::new();
    for entry in &scan.entries {
        if entry.key.len() != 32
            || &entry.key[..16] != evidence.record().as_bytes()
            || !entry.value.is_empty()
        {
            return Err(GraphDiskError::IndexCorrupt);
        }
        records.push(record_ref(&root.root, &entry.key[16..])?);
    }
    Ok((records, scan.stats))
}

pub fn scrub_current_graph_index<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    root: &CurrentGraphIndexRoot,
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

fn validate_current_root<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    root: &CurrentGraphIndexRoot,
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
        || root.root.index_profile() != &GRAPH_INDEX_PROFILE_V1
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

impl<F, W, E, I> AuthorizedIndexedReadState<F, W, E, I> for GraphState
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    type IndexRoot = AuthorizedGraphIndex;
    type IndexContext<'a> = GraphDiskReadContext<'a>;
    type IndexError = GraphDiskError;

    fn publish_current_index(
        coordinator: &mut CommitCoordinator<Self, F, W, E, I>,
        filesystem: &mut F,
    ) -> Result<Self::IndexRoot, Self::IndexError> {
        let snapshot = coordinator.reducer_state_for_checkpoint()?.snapshot();
        let published = publish_current_graph_index(coordinator, filesystem, &snapshot)?;
        load_current_graph_index_roots(coordinator, filesystem, &snapshot)?
            .into_iter()
            .find(|root| root.generation() == published.generation)
            .map(AuthorizedGraphIndex::new)
            .ok_or(GraphDiskError::IndexCorrupt)
    }

    fn load_current_index_roots(
        coordinator: &CommitCoordinator<Self, F, W, E, I>,
        filesystem: &mut F,
    ) -> Result<Vec<Self::IndexRoot>, Self::IndexError> {
        let snapshot = coordinator.reducer_state_for_checkpoint()?.snapshot();
        load_current_graph_index_roots(coordinator, filesystem, &snapshot)
            .map(|roots| roots.into_iter().map(AuthorizedGraphIndex::new).collect())
    }

    fn read_index_authorized(
        snapshot: &Self::Snapshot,
        coordinator: &CommitCoordinator<Self, F, W, E, I>,
        filesystem: &mut F,
        context: Self::IndexContext<'_>,
        request: &Self::ReadRequest,
        authorize_candidate: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Result<Self::ReadOutput, Self::IndexError> {
        validate_indexed_view(snapshot, context.index)?;
        let root = &context.index.root;
        let mut cache = context
            .index
            .cache
            .lock()
            .map_err(|_| GraphDiskError::IndexCorrupt)?;
        match request {
            GraphReadRequest::Record { id } => {
                let (record, _) = disk_record(coordinator, filesystem, root, *id, &mut cache)?;
                Ok(GraphReadOutput::Record(visible_record(
                    record.as_ref(),
                    authorize_candidate,
                )))
            }
            GraphReadRequest::RecordAt { .. } => Err(GraphDiskError::UnsupportedRequest),
            GraphReadRequest::Adjacent {
                entity,
                direction,
                maximum,
            } => {
                if *maximum > MAX_TRAVERSAL_RESULTS {
                    return Err(GraphError::ResourceLimit.into());
                }
                let (candidate_ids, _) = disk_adjacent_ids(
                    coordinator,
                    filesystem,
                    root,
                    *entity,
                    *direction,
                    MAX_TRAVERSAL_VISITS,
                    &mut cache,
                )?;
                let mut visible = Vec::new();
                for (relationship_id, neighbor_id) in candidate_ids {
                    let relationship_target = Target::Record(relationship_id);
                    if !authorize_candidate(Action::ReadRecord, relationship_target)
                        || !authorize_candidate(Action::ExpandGraph, relationship_target)
                    {
                        continue;
                    }
                    let (relationship, _) =
                        disk_record(coordinator, filesystem, root, relationship_id, &mut cache)?;
                    let Some(Record::Relationship(relationship)) = relationship else {
                        return Err(GraphDiskError::IndexCorrupt);
                    };
                    let expected_neighbor = if relationship.from == *entity {
                        relationship.to
                    } else if relationship.to == *entity {
                        relationship.from
                    } else {
                        return Err(GraphDiskError::IndexCorrupt);
                    };
                    if expected_neighbor != neighbor_id
                        || !record_references_are_authorized(
                            &Record::Relationship(relationship.clone()),
                            authorize_candidate,
                        )
                    {
                        if expected_neighbor != neighbor_id {
                            return Err(GraphDiskError::IndexCorrupt);
                        }
                        continue;
                    }
                    let neighbor_target = Target::Record(neighbor_id);
                    if !authorize_candidate(Action::ReadRecord, neighbor_target)
                        || !authorize_candidate(Action::ExpandGraph, neighbor_target)
                    {
                        continue;
                    }
                    let (neighbor, _) =
                        disk_record(coordinator, filesystem, root, neighbor_id, &mut cache)?;
                    let Some(Record::Entity(entity)) = neighbor else {
                        return Err(GraphDiskError::IndexCorrupt);
                    };
                    if !record_references_are_authorized(
                        &Record::Entity(entity.clone()),
                        authorize_candidate,
                    ) {
                        continue;
                    }
                    if visible.len() == *maximum {
                        return Err(GraphError::ResultLimit {
                            actual: visible.len().saturating_add(1),
                            maximum: *maximum,
                        }
                        .into());
                    }
                    visible.push(GraphNeighbor {
                        relationship,
                        entity,
                    });
                }
                Ok(GraphReadOutput::Adjacent(visible))
            }
            GraphReadRequest::SupportedBy { evidence, maximum } => {
                if *maximum > MAX_TRAVERSAL_RESULTS {
                    return Err(GraphError::ResourceLimit.into());
                }
                let (candidate_ids, _) = disk_supported_ids(
                    coordinator,
                    filesystem,
                    root,
                    *evidence,
                    MAX_TRAVERSAL_VISITS,
                    &mut cache,
                )?;
                let mut visible = Vec::new();
                for id in candidate_ids {
                    if !authorize_candidate(Action::ReadRecord, Target::Record(id)) {
                        continue;
                    }
                    let (record, _) = disk_record(coordinator, filesystem, root, id, &mut cache)?;
                    let Some(record) = record else {
                        return Err(GraphDiskError::IndexCorrupt);
                    };
                    if !record_references_are_authorized(&record, authorize_candidate) {
                        continue;
                    }
                    if visible.len() == *maximum {
                        return Err(GraphError::ResultLimit {
                            actual: visible.len().saturating_add(1),
                            maximum: *maximum,
                        }
                        .into());
                    }
                    visible.push(record);
                }
                Ok(GraphReadOutput::Supported(visible))
            }
        }
    }
}

fn validate_indexed_view(
    snapshot: &GraphSnapshot,
    index: &AuthorizedGraphIndex,
) -> Result<(), GraphDiskError> {
    // `AuthorizedGraphIndex` construction is private and only follows admission of the root
    // against the current snapshot. The authorization facade already binds the view to this
    // coordinator instance, while every raw disk operation rechecks the live journal frontier.
    if snapshot.revision() != Some(index.revision()) {
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

fn root_matches_snapshot(
    runs: &[IndexRunDescriptor],
    snapshot: &GraphSnapshot,
    revision: uste_types::CommitRevision,
) -> Result<bool, GraphDiskError> {
    let expected = expected_runs(snapshot, revision)?;
    Ok(runs.len() == expected.len()
        && runs.iter().zip(expected).all(|(actual, expected)| {
            actual.family() == expected.family
                && actual.entry_count() == expected.entry_count
                && actual.logical_digest() == &expected.logical_digest
        }))
}

fn expected_runs(
    snapshot: &GraphSnapshot,
    revision: uste_types::CommitRevision,
) -> Result<Vec<ExpectedRun>, GraphDiskError> {
    let mut expected = Vec::with_capacity(5);

    let mut metadata = ExpectedRunBuilder::new(snapshot, revision, FAMILY_METADATA);
    metadata.add(b"current-graph-v1", b"")?;
    expected.push(metadata.finish());

    if !snapshot.records.is_empty() {
        let mut records = ExpectedRunBuilder::new(snapshot, revision, FAMILY_CURRENT_RECORD);
        for record in snapshot.records.values() {
            let value = encode_stored_record(record)?;
            records.add(record.id().record().as_bytes(), &value)?;
        }
        expected.push(records.finish());
    }
    if !snapshot.outgoing.is_empty() {
        let mut outgoing = ExpectedRunBuilder::new(snapshot, revision, FAMILY_OUTGOING);
        for (entity, relationships) in &snapshot.outgoing {
            for relationship in relationships {
                let neighbor = relationship_neighbor(&snapshot.records, *entity, *relationship)?;
                outgoing.add(
                    &pair_key(*entity, *relationship),
                    neighbor.record().as_bytes(),
                )?;
            }
        }
        expected.push(outgoing.finish());
    }
    if !snapshot.incoming.is_empty() {
        let mut incoming = ExpectedRunBuilder::new(snapshot, revision, FAMILY_INCOMING);
        for (entity, relationships) in &snapshot.incoming {
            for relationship in relationships {
                let neighbor = relationship_neighbor(&snapshot.records, *entity, *relationship)?;
                incoming.add(
                    &pair_key(*entity, *relationship),
                    neighbor.record().as_bytes(),
                )?;
            }
        }
        expected.push(incoming.finish());
    }
    if !snapshot.provenance.is_empty() {
        let mut provenance = ExpectedRunBuilder::new(snapshot, revision, FAMILY_PROVENANCE);
        for (evidence, claims) in &snapshot.provenance {
            for claim in claims {
                provenance.add(&pair_key(*evidence, *claim), b"")?;
            }
        }
        expected.push(provenance.finish());
    }
    Ok(expected)
}

struct ExpectedRunBuilder {
    family: u8,
    entry_count: u64,
    digest: Sha256,
}

impl ExpectedRunBuilder {
    fn new(snapshot: &GraphSnapshot, revision: uste_types::CommitRevision, family: u8) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"USTE-INDEX-RUN-V1\0");
        digest.update(snapshot.scope().namespace().as_bytes());
        digest.update(revision.get().to_be_bytes());
        digest.update(GRAPH_INDEX_PROFILE_V1);
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

fn relationship_neighbor(
    records: &std::collections::BTreeMap<RecordRef, Record>,
    entity: RecordRef,
    relationship: RecordRef,
) -> Result<RecordRef, GraphDiskError> {
    let Some(Record::Relationship(record)) = records.get(&relationship) else {
        return Err(GraphDiskError::IndexCorrupt);
    };
    if record.from == entity {
        Ok(record.to)
    } else if record.to == entity {
        Ok(record.from)
    } else {
        Err(GraphDiskError::IndexCorrupt)
    }
}

fn pair_key(left: RecordRef, right: RecordRef) -> Vec<u8> {
    let mut key = Vec::with_capacity(32);
    key.extend_from_slice(left.record().as_bytes());
    key.extend_from_slice(right.record().as_bytes());
    key
}

fn record_ref(root: &RecoveredIndexRoot, encoded: &[u8]) -> Result<RecordRef, GraphDiskError> {
    let id: [u8; 16] = encoded
        .try_into()
        .map_err(|_| GraphDiskError::IndexCorrupt)?;
    Ok(RecordRef::new(
        root.scope().database(),
        root.scope().namespace(),
        RecordId::from_bytes(id),
    ))
}

fn same_scope(record: RecordRef, scope: uste_types::NamespaceRef) -> bool {
    record.database() == scope.database() && record.namespace() == scope.namespace()
}

fn codec_storage_error(error: GraphCodecError) -> StorageError {
    if matches!(error, GraphCodecError::ResourceLimit) {
        StorageError::ResourceLimit
    } else {
        StorageError::IntegrityFailure
    }
}

struct AggregateScanBudget {
    remaining_entries: usize,
    remaining_bytes: usize,
}

impl AggregateScanBudget {
    const fn new(maximum_entries: usize, maximum_bytes: usize) -> Self {
        Self {
            remaining_entries: maximum_entries,
            remaining_bytes: maximum_bytes,
        }
    }

    const fn remaining(&self) -> (usize, usize) {
        (self.remaining_entries, self.remaining_bytes)
    }

    fn account(&mut self, entries: usize, bytes: u64) -> Result<(), GraphDiskError> {
        let bytes = usize::try_from(bytes)
            .map_err(|_| GraphDiskError::Storage(StorageError::ResourceLimit))?;
        self.remaining_entries = self
            .remaining_entries
            .checked_sub(entries)
            .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
        self.remaining_bytes = self
            .remaining_bytes
            .checked_sub(bytes)
            .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
        Ok(())
    }
}

fn add_stats(total: &mut IndexReadStats, next: &IndexReadStats) {
    total.pages_read = total.pages_read.saturating_add(next.pages_read);
    total.cache_hits = total.cache_hits.saturating_add(next.cache_hits);
    total.fragments_visited = total
        .fragments_visited
        .saturating_add(next.fragments_visited);
    total.result_bytes = total.result_bytes.saturating_add(next.result_bytes);
}

#[cfg(test)]
mod tests {
    use super::{AggregateScanBudget, GraphDiskError};
    use uste_storage::journal::StorageError;

    #[test]
    fn mixed_direction_scans_share_entry_and_byte_budgets() {
        let mut budget = AggregateScanBudget::new(3, 12);
        budget.account(2, 7).unwrap();
        assert_eq!(budget.remaining(), (1, 5));
        budget.account(1, 5).unwrap();
        assert_eq!(budget.remaining(), (0, 0));
        assert_eq!(
            budget.account(1, 0),
            Err(GraphDiskError::Storage(StorageError::ResourceLimit))
        );

        let mut byte_budget = AggregateScanBudget::new(3, 12);
        assert_eq!(
            byte_budget.account(1, 13),
            Err(GraphDiskError::Storage(StorageError::ResourceLimit))
        );
    }
}
