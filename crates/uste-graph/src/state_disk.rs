//! Certificate-anchored complete graph-state derived cache.
//!
//! This profile remains optional and read-only. The authenticated journal is the only commit
//! authority; reconstructed state becomes usable only through exact-paired seeded recovery.

use std::collections::{BTreeMap, BTreeSet};

mod authorized_read;
mod authorized_write;
pub mod packed;
mod streaming_recovery;
pub use authorized_read::{GraphDiskExpansionLimits, GraphDiskReadLimits};
pub use authorized_write::GraphDiskWritePreparationLimits;
pub use streaming_recovery::{
    GraphDiskSuffixRecoveryLimits, GraphDiskSuffixRecoveryReport, recover_graph_disk_suffix,
    recover_graph_disk_suffix_with_streamed_metadata, stage_graph_genesis_root,
};

use sha2::{Digest, Sha256};
use uste_crypto::EntropySource;
use uste_policy::NamespacePolicy;
use uste_storage::{
    BlobInventory, Clock, DurableIndexRoot, IndexDelta, IndexEntry, IndexGetLimits,
    IndexPredecessor, IndexPredecessorLimits, IndexReadStats, IndexRootAnchor, IndexRootInput,
    IndexRunCursor, IndexRunDescriptor, IndexRunMergeLimits, IndexRunMergeReport,
    IndexRunReadLimits, IndexRunReadReport, IndexRunVisitor, IndexScrubReport,
    MAX_INDEX_DELTA_LOGICAL_BYTES, MAX_INDEX_ENTRIES_PER_RUN, MAX_INDEX_GET_PAGE_VISITS,
    MAX_INDEX_PAGES_PER_RUN, MAX_INDEX_RESULT_BYTES, MAX_INDEX_RUN_LOGICAL_BYTES,
    MAX_INDEX_SCAN_RESULTS, MAX_INDEX_VALUE_BYTES, OwnershipFileSystem, PageCache,
    RecoveredIndexRoot,
    journal::{DurableKeyEnvelope, StorageError},
};
use uste_txn::{
    ApplyError, AuthenticatedIndexRecovery, Cancellation, CheckpointState, CheckpointStateError,
    CommitCoordinator, CoordinatorMetadataCandidate, CoordinatorMetadataLoadLimits,
    CoordinatorMetadataLoadReport, CoordinatorRecoverySeed, DerivedIndexMaintenance,
    EquivalentTransactionState, ExternallyPreparedTransactionState,
    JournalAnchoredTransactionState, PostCommitStateMaintenance, RecoveredFrontierTransaction,
    TransactionError, TransactionOutcome, TransactionRequest, TransactionState,
    reconstruct_coordinator_metadata_seed_for_recovery,
};
use uste_types::{CommitRevision, NamespaceRef, RecordId, RecordRef, Value};

use crate::codec::{decode_result_policy, encode_result_policy};
use crate::{
    AssertionAction, Expected, GraphCodecError, GraphDiskError, GraphSnapshot, GraphState,
    GraphTransaction, NewAssertion, NewRecord, NewRelationship, Operation, Predicate, Record,
    decode_stored_record, encode_stored_record,
    state::{
        MAX_GRAPH_CHECKPOINT_BYTES, PreparedGraph, REVERSE_KIND_ASSERTION, REVERSE_KIND_ENTITY,
        REVERSE_KIND_RELATIONSHIP, REVERSE_ROLE_ASSERTION_OBJECT, REVERSE_ROLE_ASSERTION_SUBJECT,
        REVERSE_ROLE_CORRECTION_OF, REVERSE_ROLE_ENTITY_PROPERTY, REVERSE_ROLE_EVIDENCE,
        REVERSE_ROLE_RELATIONSHIP_FROM, REVERSE_ROLE_RELATIONSHIP_PROPERTY,
        REVERSE_ROLE_RELATIONSHIP_TO, REVERSE_STATE_ACCEPTED, REVERSE_STATE_ACTIVE,
        REVERSE_STATE_DELETED, REVERSE_STATE_DISPUTED, REVERSE_STATE_EXPIRED,
        REVERSE_STATE_PROPOSED, REVERSE_STATE_REJECTED, REVERSE_STATE_RETRACTED,
        REVERSE_STATE_SUPERSEDED, ReferenceRequirement, ReferenceRequirementVisitError,
        ReverseReference, prepare_from_complete_disk_proofs, record_reverse_references,
        reference_requirement_matches, reverse_reference, try_visit_record_references,
        validate_history_first, validate_history_successor, validate_request_limits,
        visit_current_reference_requirements, visit_history_first_reference_requirements,
        visit_history_successor_reference_requirements, visit_record_references,
    },
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
/// This is intentionally distinct from `DerivedGraphStateRoot`, which has already been admitted
/// either against a live reducer snapshot or by proof-derived terminal output validation.
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

/// Caller-selected bounds for cold semantic admission without materializing complete graph maps.
/// Scan capacities bound distinct run contents; aggregate lookup work includes repeated reads and
/// cache hits, and is independently bounded by operation count and per-lookup storage ceilings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphDiskBaseAdmissionLimits {
    scan: GraphStateLoadLimits,
    maximum_history_group_versions: u64,
    maximum_history_group_logical_bytes: u64,
    maximum_semantic_reference_visits: u64,
    maximum_lookup_operations: u64,
    maximum_lookup_page_visits: u64,
    maximum_lookup_result_bytes: u64,
    predecessor: IndexPredecessorLimits,
}

impl GraphDiskBaseAdmissionLimits {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        scan: GraphStateLoadLimits,
        maximum_history_group_versions: u64,
        maximum_history_group_logical_bytes: u64,
        maximum_semantic_reference_visits: u64,
        maximum_lookup_operations: u64,
        maximum_lookup_page_visits: u64,
        maximum_lookup_result_bytes: u64,
        predecessor: IndexPredecessorLimits,
    ) -> Result<Self, GraphDiskError> {
        let pages_per_lookup = if predecessor.maximum_page_visits() > MAX_INDEX_GET_PAGE_VISITS {
            predecessor.maximum_page_visits()
        } else {
            MAX_INDEX_GET_PAGE_VISITS
        };
        let bytes_per_lookup = if predecessor.maximum_result_bytes() > MAX_INDEX_VALUE_BYTES {
            predecessor.maximum_result_bytes()
        } else {
            MAX_INDEX_VALUE_BYTES
        };
        // These are representable configuration ceilings, not allocations or work counters.
        // Saturation clamps a theoretical product to u64; actual charging remains checked and
        // exact and every lookup is additionally clamped to the remaining aggregate budget.
        let possible_pages = maximum_lookup_operations.saturating_mul(pages_per_lookup);
        let possible_bytes = maximum_lookup_operations.saturating_mul(bytes_per_lookup as u64);
        if maximum_history_group_versions == 0
            || maximum_history_group_versions > MAX_INDEX_ENTRIES_PER_RUN
            || maximum_history_group_logical_bytes == 0
            || maximum_history_group_logical_bytes > MAX_INDEX_RUN_LOGICAL_BYTES
            || maximum_history_group_logical_bytes > usize::MAX as u64
            || maximum_semantic_reference_visits == 0
            || maximum_semantic_reference_visits
                > MAX_INDEX_ENTRIES_PER_RUN * crate::MAX_TRANSACTION_REFERENCES as u64
            || maximum_lookup_operations == 0
            || maximum_lookup_operations
                > MAX_INDEX_ENTRIES_PER_RUN * crate::MAX_TRANSACTION_REFERENCES as u64
            || maximum_lookup_page_visits == 0
            || maximum_lookup_page_visits > possible_pages
            || maximum_lookup_result_bytes == 0
            || maximum_lookup_result_bytes > possible_bytes
        {
            return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
        }
        Ok(Self {
            scan,
            maximum_history_group_versions,
            maximum_history_group_logical_bytes,
            maximum_semantic_reference_visits,
            maximum_lookup_operations,
            maximum_lookup_page_visits,
            maximum_lookup_result_bytes,
            predecessor,
        })
    }
}

/// Authenticated work observed while semantically admitting one cold graph-state root.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphDiskBaseAdmissionReport {
    pub scan: GraphStateLoadReport,
    pub exact_lookups: u64,
    pub predecessor_lookups: u64,
    pub semantic_reference_visits: u64,
    pub lookup_page_visits: u64,
    pub lookup_result_bytes: u64,
    pub peak_history_group_logical_bytes: u64,
}

/// Semantically admitted cold graph-state base. It owns no filesystem or journal capability.
pub struct GraphDiskBase {
    root: DerivedGraphStateRoot,
    counts: [u64; 8],
    current_policy: Option<NamespacePolicy>,
}

/// Warm live graph reducer backed by one admitted terminal disk root.
///
/// At most one durably committed, request-bounded root plan may be pending. A pending state is
/// repair-only until its terminal root is published and installed.
pub struct GraphDiskLiveState {
    base: GraphDiskBase,
    pending: Option<GraphDiskPending>,
}

struct GraphDiskPending {
    plan: GraphStateRootDelta,
    target_policy: Option<NamespacePolicy>,
}

/// Metadata-only snapshot for representation equivalence and coordinator health checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphDiskLiveSnapshot {
    scope: NamespaceRef,
    revision: CommitRevision,
    logical_state_digest: Option<[u8; 32]>,
}

/// Opaque, already-published terminal base installed through coordinator maintenance.
pub struct GraphDiskLivePublication {
    base: GraphDiskBase,
}

impl core::fmt::Debug for GraphDiskBase {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("GraphDiskBase")
            .field("scope", &"[REDACTED]")
            .field("revision", &self.revision())
            .field("generation", &self.generation())
            .field("counts", &"[REDACTED]")
            .field("has_policy", &self.current_policy.is_some())
            .finish()
    }
}

impl GraphDiskBase {
    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.root.root.scope()
    }

    #[must_use]
    pub const fn anchor(&self) -> IndexRootAnchor {
        self.root.root.anchor()
    }

    #[must_use]
    pub const fn revision(&self) -> CommitRevision {
        self.root.revision()
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.root.generation()
    }

    #[must_use]
    pub const fn namespace_policy(&self) -> Option<&NamespacePolicy> {
        self.current_policy.as_ref()
    }

    /// Privileged admitted counts in current, history, outgoing, incoming, provenance, reverse,
    /// policy-history and current-policy order. Debug output deliberately redacts these values.
    #[must_use]
    pub const fn state_counts(&self) -> [u64; 8] {
        self.counts
    }

    #[must_use]
    pub const fn admitted_root(&self) -> &DerivedGraphStateRoot {
        &self.root
    }
}

impl core::fmt::Debug for GraphDiskLiveState {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("GraphDiskLiveState")
            .field("scope", &"[REDACTED]")
            .field("revision", &self.revision())
            .field("pending", &self.pending.is_some())
            .finish()
    }
}

impl GraphDiskLiveState {
    #[must_use]
    pub const fn new(base: GraphDiskBase) -> Self {
        Self {
            base,
            pending: None,
        }
    }

    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.base.scope()
    }

    #[must_use]
    pub fn revision(&self) -> CommitRevision {
        self.pending
            .as_ref()
            .map_or_else(|| self.base.revision(), |pending| pending.plan.revision)
    }

    #[must_use]
    pub const fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// The current usable base. A pending journal revision deliberately hides the stale root.
    #[must_use]
    pub const fn current_base(&self) -> Option<&GraphDiskBase> {
        if self.pending.is_none() {
            Some(&self.base)
        } else {
            None
        }
    }

    fn pending_publication(
        &self,
        outcome: TransactionOutcome,
    ) -> Result<(&GraphDiskBase, &GraphDiskPending), GraphDiskError> {
        let pending = self
            .pending
            .as_ref()
            .ok_or(GraphDiskError::RootStateMismatch)?;
        if outcome.revision != pending.plan.revision
            || outcome.result_digest != pending.plan.result_digest
        {
            return Err(GraphDiskError::RootStateMismatch);
        }
        Ok((&self.base, pending))
    }
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
#[derive(Clone)]
pub struct GraphStateRootDelta {
    scope: NamespaceRef,
    base_anchor: IndexRootAnchor,
    base_revision: CommitRevision,
    revision: CommitRevision,
    result_digest: [u8; 32],
    target_counts: [u64; 8],
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

/// Per-family storage merge budgets in graph-state family order 1 through 8, plus the explicit
/// bound for one record's canonical history framing buffer during output validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphStateRootMergeLimits {
    families: [IndexRunMergeLimits; FAMILY_COUNT as usize],
    maximum_history_group_logical_bytes: u64,
}

impl GraphStateRootMergeLimits {
    /// Both fallback slots use the largest declared per-family base budget, never format maxima.
    fn fallback_read_limits(&self) -> Result<IndexRunReadLimits, GraphDiskError> {
        IndexRunReadLimits::new(
            self.families
                .iter()
                .map(|limit| limit.base().maximum_pages())
                .max()
                .unwrap_or(0),
            self.families
                .iter()
                .map(|limit| limit.base().maximum_entries())
                .max()
                .unwrap_or(0),
            self.families
                .iter()
                .map(|limit| limit.base().maximum_logical_bytes())
                .max()
                .unwrap_or(0),
        )
        .map_err(GraphDiskError::Storage)
    }

    pub const fn new(
        families: [IndexRunMergeLimits; FAMILY_COUNT as usize],
        maximum_history_group_logical_bytes: u64,
    ) -> Result<Self, GraphDiskError> {
        if maximum_history_group_logical_bytes == 0
            || maximum_history_group_logical_bytes > MAX_INDEX_RUN_LOGICAL_BYTES
            || maximum_history_group_logical_bytes > usize::MAX as u64
        {
            return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
        }
        Ok(Self {
            families,
            maximum_history_group_logical_bytes,
        })
    }

    pub const fn uniform(
        limit: IndexRunMergeLimits,
        maximum_history_group_logical_bytes: u64,
    ) -> Result<Self, GraphDiskError> {
        Self::new(
            [limit; FAMILY_COUNT as usize],
            maximum_history_group_logical_bytes,
        )
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
    fn reader_scope(&self) -> NamespaceRef;

    fn reader_load_root_manifests(
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

    fn reader_get_bounded(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        key: &[u8],
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<(Option<Vec<u8>>, IndexReadStats), TransactionError>;

    #[allow(clippy::too_many_arguments)]
    fn reader_get_predecessor(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        prefix: &[u8],
        upper_bound: &[u8],
        limits: IndexPredecessorLimits,
        cache: &mut PageCache,
    ) -> Result<IndexPredecessor, TransactionError>;

    #[allow(clippy::too_many_arguments)]
    fn reader_scan_prefix(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        prefix: &[u8],
        maximum_results: usize,
        maximum_result_bytes: usize,
        cache: &mut PageCache,
    ) -> Result<uste_storage::IndexScan, TransactionError>;

    fn reader_open_cursor(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        limits: IndexRunReadLimits,
    ) -> Result<IndexRunCursor<F>, TransactionError>;

    fn reader_next_cursor(
        &self,
        filesystem: &mut F,
        cursor: &mut IndexRunCursor<F>,
    ) -> Result<Option<IndexEntry>, TransactionError>;

    fn reader_finish_cursor(
        &self,
        cursor: IndexRunCursor<F>,
    ) -> Result<IndexRunReadReport, TransactionError>;
}

impl<S, F, W, E, I> GraphStateIndexReader<F> for CommitCoordinator<S, F, W, E, I>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn reader_scope(&self) -> NamespaceRef {
        self.scope()
    }

    fn reader_load_root_manifests(
        &self,
        filesystem: &mut F,
        profile: [u8; 32],
    ) -> Result<Vec<RecoveredIndexRoot>, TransactionError> {
        self.load_index_root_manifests(filesystem, profile)
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

    fn reader_get_bounded(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        key: &[u8],
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<(Option<Vec<u8>>, IndexReadStats), TransactionError> {
        self.index_get_bounded(filesystem, root, family, key, limits, cache)
    }

    fn reader_get_predecessor(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        prefix: &[u8],
        upper_bound: &[u8],
        limits: IndexPredecessorLimits,
        cache: &mut PageCache,
    ) -> Result<IndexPredecessor, TransactionError> {
        self.index_get_predecessor(filesystem, root, family, prefix, upper_bound, limits, cache)
    }

    fn reader_scan_prefix(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        prefix: &[u8],
        maximum_results: usize,
        maximum_result_bytes: usize,
        cache: &mut PageCache,
    ) -> Result<uste_storage::IndexScan, TransactionError> {
        self.index_scan_prefix(
            filesystem,
            root,
            family,
            prefix,
            maximum_results,
            maximum_result_bytes,
            cache,
        )
    }

    fn reader_open_cursor(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        limits: IndexRunReadLimits,
    ) -> Result<IndexRunCursor<F>, TransactionError> {
        self.open_index_run_cursor(filesystem, root, family, limits)
    }

    fn reader_next_cursor(
        &self,
        filesystem: &mut F,
        cursor: &mut IndexRunCursor<F>,
    ) -> Result<Option<IndexEntry>, TransactionError> {
        self.next_index_run_entry(filesystem, cursor)
    }

    fn reader_finish_cursor(
        &self,
        cursor: IndexRunCursor<F>,
    ) -> Result<IndexRunReadReport, TransactionError> {
        self.finish_index_run_cursor(cursor)
    }
}

impl<S, F, W, E, I> GraphStateIndexReader<F> for uste_txn::DiskCommitCoordinator<S, F, W, E, I>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn reader_scope(&self) -> NamespaceRef {
        self.scope()
    }
    fn reader_load_root_manifests(
        &self,
        filesystem: &mut F,
        profile: [u8; 32],
    ) -> Result<Vec<RecoveredIndexRoot>, TransactionError> {
        self.load_index_root_manifests(filesystem, profile)
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
    fn reader_get_bounded(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        key: &[u8],
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<(Option<Vec<u8>>, IndexReadStats), TransactionError> {
        self.index_get_bounded(filesystem, root, family, key, limits, cache)
    }
    fn reader_get_predecessor(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        prefix: &[u8],
        upper_bound: &[u8],
        limits: IndexPredecessorLimits,
        cache: &mut PageCache,
    ) -> Result<IndexPredecessor, TransactionError> {
        self.index_get_predecessor(filesystem, root, family, prefix, upper_bound, limits, cache)
    }
    fn reader_scan_prefix(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        prefix: &[u8],
        maximum_results: usize,
        maximum_result_bytes: usize,
        cache: &mut PageCache,
    ) -> Result<uste_storage::IndexScan, TransactionError> {
        self.index_scan_prefix(
            filesystem,
            root,
            family,
            prefix,
            maximum_results,
            maximum_result_bytes,
            cache,
        )
    }
    fn reader_open_cursor(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        limits: IndexRunReadLimits,
    ) -> Result<IndexRunCursor<F>, TransactionError> {
        self.open_index_run_cursor(filesystem, root, family, limits)
    }
    fn reader_next_cursor(
        &self,
        filesystem: &mut F,
        cursor: &mut IndexRunCursor<F>,
    ) -> Result<Option<IndexEntry>, TransactionError> {
        self.next_index_run_entry(filesystem, cursor)
    }
    fn reader_finish_cursor(
        &self,
        cursor: IndexRunCursor<F>,
    ) -> Result<IndexRunReadReport, TransactionError> {
        self.finish_index_run_cursor(cursor)
    }
}

impl<F, W, E, I> GraphStateIndexReader<F> for AuthenticatedIndexRecovery<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn reader_scope(&self) -> NamespaceRef {
        self.scope()
    }

    fn reader_load_root_manifests(
        &self,
        filesystem: &mut F,
        profile: [u8; 32],
    ) -> Result<Vec<RecoveredIndexRoot>, TransactionError> {
        self.load_index_root_manifests(filesystem, profile)
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

    fn reader_get_bounded(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        key: &[u8],
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<(Option<Vec<u8>>, IndexReadStats), TransactionError> {
        self.index_get_bounded(filesystem, root, family, key, limits, cache)
    }

    fn reader_get_predecessor(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        prefix: &[u8],
        upper_bound: &[u8],
        limits: IndexPredecessorLimits,
        cache: &mut PageCache,
    ) -> Result<IndexPredecessor, TransactionError> {
        self.index_get_predecessor(filesystem, root, family, prefix, upper_bound, limits, cache)
    }

    fn reader_scan_prefix(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        prefix: &[u8],
        maximum_results: usize,
        maximum_result_bytes: usize,
        cache: &mut PageCache,
    ) -> Result<uste_storage::IndexScan, TransactionError> {
        self.index_scan_prefix(
            filesystem,
            root,
            family,
            prefix,
            maximum_results,
            maximum_result_bytes,
            cache,
        )
    }

    fn reader_open_cursor(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        limits: IndexRunReadLimits,
    ) -> Result<IndexRunCursor<F>, TransactionError> {
        self.open_index_run_cursor(filesystem, root, family, limits)
    }

    fn reader_next_cursor(
        &self,
        filesystem: &mut F,
        cursor: &mut IndexRunCursor<F>,
    ) -> Result<Option<IndexEntry>, TransactionError> {
        self.next_index_run_entry(filesystem, cursor)
    }

    fn reader_finish_cursor(
        &self,
        cursor: IndexRunCursor<F>,
    ) -> Result<IndexRunReadReport, TransactionError> {
        self.finish_index_run_cursor(cursor)
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

/// Caller-selected aggregate bounds for one disk preparation proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphDiskPreparationLimits {
    maximum_record_proofs: u64,
    maximum_reference_visits: u64,
    maximum_history_versions: u64,
    maximum_reverse_references: u64,
    maximum_proof_logical_bytes: u64,
}

impl GraphDiskPreparationLimits {
    pub const fn new(
        maximum_record_proofs: u64,
        maximum_reference_visits: u64,
        maximum_history_versions: u64,
        maximum_reverse_references: u64,
        maximum_proof_logical_bytes: u64,
    ) -> Result<Self, GraphDiskError> {
        if maximum_record_proofs == 0
            || maximum_record_proofs
                > (crate::MAX_TRANSACTION_OPERATIONS + crate::MAX_TRANSACTION_REFERENCES) as u64
            || maximum_reference_visits == 0
            || maximum_reference_visits > crate::MAX_TRAVERSAL_VISITS as u64
            || maximum_history_versions > MAX_INDEX_SCAN_RESULTS as u64
            || maximum_reverse_references > MAX_INDEX_SCAN_RESULTS as u64
            || maximum_proof_logical_bytes == 0
            || maximum_proof_logical_bytes > MAX_GRAPH_CHECKPOINT_BYTES as u64
        {
            return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
        }
        Ok(Self {
            maximum_record_proofs,
            maximum_reference_visits,
            maximum_history_versions,
            maximum_reverse_references,
            maximum_proof_logical_bytes,
        })
    }
}

/// Observed logical proof and authenticated-index work for disk-backed preparation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphDiskPreparationReport {
    pub record_proofs: u64,
    pub present_records: u64,
    pub absent_records: u64,
    pub reference_visits: u64,
    pub history_versions: u64,
    pub reverse_references: u64,
    pub proof_logical_bytes: u64,
    pub index_lookups: u64,
    pub pages_read: u64,
    pub cache_hits: u64,
    pub fragments_visited: u64,
}

enum CurrentRecordProof {
    Absent,
    Present(Box<Record>),
}

/// Complete bounded current, history and reverse proof for one graph transaction.
///
/// This value owns no filesystem, coordinator, key-vault, or cache capability. Consequently its
/// pure `prepare` phase cannot perform hidden I/O.
pub struct GraphDiskPreparationView {
    scope: NamespaceRef,
    base_anchor: IndexRootAnchor,
    base_revision: CommitRevision,
    base_counts: [u64; 8],
    transaction: GraphTransaction,
    current: BTreeMap<RecordRef, CurrentRecordProof>,
    history: BTreeMap<RecordRef, Vec<Record>>,
    reverse: BTreeMap<RecordRef, BTreeMap<RecordRef, ReverseReference>>,
    current_policy: Option<NamespacePolicy>,
    report: GraphDiskPreparationReport,
}

impl core::fmt::Debug for GraphDiskPreparationView {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("GraphDiskPreparationView")
            .field("scope", &"[REDACTED]")
            .field("base_revision", &self.base_revision)
            .field("record_proofs", &self.current.len())
            .field("report", &self.report)
            .finish()
    }
}

impl GraphDiskPreparationView {
    #[must_use]
    pub const fn base_revision(&self) -> CommitRevision {
        self.base_revision
    }

    #[must_use]
    pub const fn report(&self) -> &GraphDiskPreparationReport {
        &self.report
    }

    /// Run the existing graph reducer using only the already authenticated proof closure.
    pub fn prepare(self) -> Result<DiskPreparedGraph, GraphDiskError> {
        let required = required_current_ids(&self.transaction, &self.current);
        if required.iter().any(|id| !self.current.contains_key(id)) {
            return Err(GraphDiskError::IndexCorrupt);
        }
        if history_proof_targets(&self.transaction)
            .iter()
            .any(|id| !self.history.contains_key(id))
            || reverse_proof_targets(&self.transaction)
                .iter()
                .any(|id| !self.reverse.contains_key(id))
        {
            return Err(GraphDiskError::IndexCorrupt);
        }
        let records = self
            .current
            .into_iter()
            .filter_map(|(id, proof)| match proof {
                CurrentRecordProof::Absent => None,
                CurrentRecordProof::Present(record) => Some((id, *record)),
            })
            .collect();
        let (prepared, base_policy) = prepare_from_complete_disk_proofs(
            self.scope,
            self.base_revision,
            records,
            self.history,
            self.reverse,
            self.current_policy,
            &self.transaction,
        )?;
        Ok(DiskPreparedGraph {
            prepared,
            base_anchor: self.base_anchor,
            base_counts: self.base_counts,
            base_policy,
        })
    }
}

/// Opaque reducer result produced from a disk preparation proof.
#[derive(Clone)]
pub struct DiskPreparedGraph {
    prepared: PreparedGraph,
    base_anchor: IndexRootAnchor,
    base_counts: [u64; 8],
    base_policy: Option<NamespacePolicy>,
}

impl core::fmt::Debug for DiskPreparedGraph {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("DiskPreparedGraph")
            .field("base_revision", &self.prepared.base_revision)
            .field("revision", &self.prepared.revision)
            .field("change_count", &self.prepared.change_count())
            .finish()
    }
}

impl DiskPreparedGraph {
    #[must_use]
    pub const fn base_revision(&self) -> CommitRevision {
        self.prepared
            .base_revision
            .expect("disk preparation always has a current base revision")
    }

    #[must_use]
    pub const fn revision(&self) -> CommitRevision {
        self.prepared.revision
    }

    #[must_use]
    pub const fn result_digest(&self) -> [u8; 32] {
        self.prepared.result_digest
    }

    #[must_use]
    pub fn change_count(&self) -> usize {
        self.prepared.change_count()
    }
}

/// Opaque proof-prepared commit bundle owned by the warm disk reducer after journal publication.
#[derive(Clone)]
pub struct GraphDiskCommit {
    prepared: DiskPreparedGraph,
    plan: GraphStateRootDelta,
}

impl core::fmt::Debug for GraphDiskCommit {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("GraphDiskCommit")
            .field("base_revision", &self.plan.base_revision)
            .field("revision", &self.plan.revision)
            .field("deltas", &self.plan.deltas)
            .finish()
    }
}

/// Load a bounded authenticated proof for a current-state graph transaction.
///
/// Current exact lookups plus complete bounded history/reverse prefix scans occur only in this
/// explicit phase. The returned view has no capability to perform further storage I/O.
pub fn load_graph_disk_preparation_view<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    base: &DerivedGraphStateRoot,
    transaction: GraphTransaction,
    limits: GraphDiskPreparationLimits,
    cache: &mut PageCache,
) -> Result<GraphDiskPreparationView, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    validate_current_root(coordinator, base)?;
    load_graph_disk_preparation_view_with_reader(
        coordinator,
        filesystem,
        base,
        transaction,
        limits,
        cache,
    )
}

/// Load a proof against the exact ready base owned by a warm disk-backed coordinator.
pub fn load_graph_disk_live_preparation_view<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphDiskLiveState, F, W, E, I>,
    filesystem: &mut F,
    transaction: GraphTransaction,
    limits: GraphDiskPreparationLimits,
    cache: &mut PageCache,
) -> Result<GraphDiskPreparationView, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let state = coordinator.reducer_state_for_checkpoint()?;
    let base = state
        .current_base()
        .ok_or(GraphDiskError::RootStateMismatch)?;
    validate_current_root(coordinator, base.admitted_root())?;
    load_graph_disk_preparation_view_with_reader(
        coordinator,
        filesystem,
        base.admitted_root(),
        transaction,
        limits,
        cache,
    )
}

/// Trusted warm preparation against the disk coordinator's exact ready graph base.
/// Consumer authorization must precede this explicit-I/O proof phase.
pub fn load_graph_disk_coordinator_preparation_view<F, W, E, I>(
    coordinator: &uste_txn::DiskCommitCoordinator<GraphDiskLiveState, F, W, E, I>,
    filesystem: &mut F,
    transaction: GraphTransaction,
    limits: GraphDiskPreparationLimits,
    cache: &mut PageCache,
) -> Result<GraphDiskPreparationView, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let base = coordinator
        .state()?
        .current_base()
        .ok_or(GraphDiskError::RootStateMismatch)?;
    validate_root_anchor(coordinator.checkpoint_anchor()?, base.admitted_root())?;
    load_graph_disk_preparation_view_with_reader(
        coordinator,
        filesystem,
        base.admitted_root(),
        transaction,
        limits,
        cache,
    )
}

/// Load a bounded proof against an admitted predecessor root during authenticated recovery.
pub fn load_graph_disk_recovery_preparation_view<F, W, E, I>(
    recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
    filesystem: &mut F,
    base: &GraphDiskBase,
    frontier: &RecoveredFrontierTransaction,
    limits: GraphDiskPreparationLimits,
    cache: &mut PageCache,
) -> Result<GraphDiskPreparationView, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if frontier.blob_inventory().is_some()
        || base.scope() != recovery.scope()
        || base.revision().checked_next().ok() != Some(frontier.revision())
    {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let transaction = crate::decode_transaction(frontier.canonical_request())?;
    load_graph_disk_preparation_view_with_reader(
        recovery,
        filesystem,
        base.admitted_root(),
        transaction,
        limits,
        cache,
    )
}

fn load_graph_disk_preparation_view_with_reader<R, F>(
    reader: &R,
    filesystem: &mut F,
    base: &DerivedGraphStateRoot,
    transaction: GraphTransaction,
    limits: GraphDiskPreparationLimits,
    cache: &mut PageCache,
) -> Result<GraphDiskPreparationView, GraphDiskError>
where
    F: OwnershipFileSystem,
    R: GraphStateIndexReader<F>,
{
    validate_disk_preparation_subset(&transaction)?;
    validate_request_limits(&transaction)?;
    if transaction.scope() != reader.reader_scope() {
        return Err(GraphDiskError::Graph(
            crate::GraphError::TransactionScopeMismatch,
        ));
    }
    let mut report = GraphDiskPreparationReport::default();
    let mut pending = transaction_required_ids(&transaction, &mut report, &limits)?;
    charge_logical_bytes(&mut report, &limits, b"graph-state-v1".len())?;
    let (metadata, stats) = reader.reader_get_bounded(
        filesystem,
        &base.root,
        FAMILY_METADATA,
        b"graph-state-v1",
        IndexGetLimits::new(MAX_INDEX_GET_PAGE_VISITS, MAX_INDEX_VALUE_BYTES)
            .map_err(GraphDiskError::Storage)?,
        cache,
    )?;
    add_read_stats(&mut report, stats)?;
    let metadata = metadata.ok_or(GraphDiskError::IndexCorrupt)?;
    charge_logical_bytes(&mut report, &limits, metadata.len())?;
    let metadata =
        parse_metadata(&metadata, base.revision()).map_err(|_| GraphDiskError::IndexCorrupt)?;
    validate_metadata_against_root(&base.root, metadata)?;
    let base_counts = metadata.state_counts();

    charge_logical_bytes(&mut report, &limits, 1)?;
    let current_policy = if has_family(&base.root, FAMILY_POLICY) {
        let (encoded, stats) = reader.reader_get_bounded(
            filesystem,
            &base.root,
            FAMILY_POLICY,
            &[0],
            IndexGetLimits::new(MAX_INDEX_GET_PAGE_VISITS, MAX_INDEX_VALUE_BYTES)
                .map_err(GraphDiskError::Storage)?,
            cache,
        )?;
        add_read_stats(&mut report, stats)?;
        let encoded = encoded.ok_or(GraphDiskError::IndexCorrupt)?;
        charge_logical_bytes(&mut report, &limits, encoded.len())?;
        let policy = decode_result_policy(&encoded)?.ok_or(GraphDiskError::IndexCorrupt)?;
        if policy.scope() != transaction.scope() {
            return Err(GraphDiskError::IndexCorrupt);
        }
        Some(policy)
    } else {
        None
    };

    let mut current = BTreeMap::new();
    while let Some(id) = pending.pop_first() {
        if current.contains_key(&id) {
            continue;
        }
        report.record_proofs += 1;
        let proof = if has_family(&base.root, FAMILY_CURRENT_RECORD) {
            let (encoded, stats) = reader.reader_get_bounded(
                filesystem,
                &base.root,
                FAMILY_CURRENT_RECORD,
                id.record().as_bytes(),
                IndexGetLimits::new(MAX_INDEX_GET_PAGE_VISITS, MAX_INDEX_VALUE_BYTES)
                    .map_err(GraphDiskError::Storage)?,
                cache,
            )?;
            add_read_stats(&mut report, stats)?;
            match encoded {
                Some(encoded) => {
                    charge_logical_bytes(&mut report, &limits, encoded.len())?;
                    let record = decode_stored_record(&encoded)?;
                    if record.id() != id || record.modified_revision() > base.revision() {
                        return Err(GraphDiskError::IndexCorrupt);
                    }
                    report.present_records += 1;
                    CurrentRecordProof::Present(Box::new(record))
                }
                None => {
                    report.absent_records += 1;
                    CurrentRecordProof::Absent
                }
            }
        } else {
            report.absent_records += 1;
            CurrentRecordProof::Absent
        };
        current.insert(id, proof);
        if needs_retained_references(&transaction, id)
            && let Some(CurrentRecordProof::Present(record)) = current.get(&id)
        {
            try_visit_record_references(record, &mut |reference, _| {
                charge_reference(&mut report, &limits)?;
                add_pending(reference, &current, &mut pending, &mut report, &limits)
            })?;
        }
    }

    let history_targets = history_proof_targets(&transaction);
    let history = load_history_proofs(
        reader,
        filesystem,
        &base.root,
        transaction.scope(),
        &history_targets,
        &limits,
        &mut report,
        cache,
    )?;
    let reverse_targets = reverse_proof_targets(&transaction);
    let reverse = load_reverse_proofs(
        reader,
        filesystem,
        &base.root,
        transaction.scope(),
        &reverse_targets,
        &limits,
        &mut report,
        cache,
    )?;

    Ok(GraphDiskPreparationView {
        scope: transaction.scope(),
        base_anchor: base.root.anchor(),
        base_revision: base.revision(),
        base_counts,
        transaction,
        current,
        history,
        reverse,
        current_policy,
        report,
    })
}

fn validate_disk_preparation_subset(transaction: &GraphTransaction) -> Result<(), GraphDiskError> {
    fn validate_expected(expected: &Expected) -> Result<(), GraphDiskError> {
        match expected {
            Expected::Absent | Expected::Version(_) | Expected::ReadView { .. } => Ok(()),
        }
    }

    for operation in transaction.operations() {
        match operation {
            Operation::Create { expected, .. } | Operation::ReplaceEntity { expected, .. } => {
                validate_expected(expected)?;
            }
            Operation::ActOnAssertion {
                expected,
                correction_expected,
                ..
            }
            | Operation::ActOnRelationship {
                expected,
                correction_expected,
                ..
            } => {
                validate_expected(expected)?;
                if let Some(expected) = correction_expected {
                    validate_expected(expected)?;
                }
            }
            Operation::DeleteEntity { expected, .. } => validate_expected(expected)?,
        }
    }
    Ok(())
}

fn transaction_required_ids(
    transaction: &GraphTransaction,
    report: &mut GraphDiskPreparationReport,
    limits: &GraphDiskPreparationLimits,
) -> Result<BTreeSet<RecordRef>, GraphDiskError> {
    let mut required = BTreeSet::new();
    for operation in transaction.operations() {
        add_required(
            transaction.scope(),
            operation.target(),
            &mut required,
            report,
            limits,
        )?;
        add_expected_required(
            transaction.scope(),
            operation.expected(),
            &mut required,
            report,
            limits,
        )?;
        match operation {
            Operation::Create { record, .. } => {
                visit_new_record_references(record, &mut |id| {
                    add_required(transaction.scope(), id, &mut required, report, limits)
                })?;
            }
            Operation::ReplaceEntity { properties, .. } => {
                visit_value_record_references(properties, &mut |id| {
                    add_required(transaction.scope(), id, &mut required, report, limits)
                })?;
            }
            Operation::ActOnAssertion { correction, .. } => {
                if let Some(correction) = correction {
                    add_required(
                        transaction.scope(),
                        correction.id,
                        &mut required,
                        report,
                        limits,
                    )?;
                    visit_new_assertion_references(correction, &mut |id| {
                        add_required(transaction.scope(), id, &mut required, report, limits)
                    })?;
                }
                if let Operation::ActOnAssertion {
                    correction_expected: Some(expected),
                    ..
                } = operation
                {
                    add_expected_required(
                        transaction.scope(),
                        expected,
                        &mut required,
                        report,
                        limits,
                    )?;
                }
            }
            Operation::ActOnRelationship { correction, .. } => {
                if let Some(correction) = correction {
                    add_required(
                        transaction.scope(),
                        correction.id,
                        &mut required,
                        report,
                        limits,
                    )?;
                    visit_new_relationship_references(correction, &mut |id| {
                        add_required(transaction.scope(), id, &mut required, report, limits)
                    })?;
                }
                if let Operation::ActOnRelationship {
                    correction_expected: Some(expected),
                    ..
                } = operation
                {
                    add_expected_required(
                        transaction.scope(),
                        expected,
                        &mut required,
                        report,
                        limits,
                    )?;
                }
            }
            Operation::DeleteEntity { affected, .. } => {
                for id in affected {
                    add_required(transaction.scope(), *id, &mut required, report, limits)?;
                }
            }
        }
    }
    Ok(required)
}

fn required_current_ids(
    transaction: &GraphTransaction,
    current: &BTreeMap<RecordRef, CurrentRecordProof>,
) -> BTreeSet<RecordRef> {
    let mut required = BTreeSet::new();
    let mut add = |id| {
        required.insert(id);
        Ok::<(), GraphDiskError>(())
    };
    for operation in transaction.operations() {
        add(operation.target()).expect("infallible proof-set insertion");
        if let Expected::ReadView { predicate, .. } = operation.expected() {
            add(predicate_record(predicate)).expect("infallible proof-set insertion");
        }
        match operation {
            Operation::Create { record, .. } => {
                visit_new_record_references(record, &mut add)
                    .expect("infallible proof-set insertion");
            }
            Operation::ReplaceEntity { properties, .. } => {
                visit_value_record_references(properties, &mut add)
                    .expect("infallible proof-set insertion");
            }
            Operation::ActOnAssertion { correction, .. } => {
                if let Some(correction) = correction {
                    add(correction.id).expect("infallible proof-set insertion");
                    visit_new_assertion_references(correction, &mut add)
                        .expect("infallible proof-set insertion");
                }
                if let Operation::ActOnAssertion {
                    correction_expected: Some(Expected::ReadView { predicate, .. }),
                    ..
                } = operation
                {
                    add(predicate_record(predicate)).expect("infallible proof-set insertion");
                }
            }
            Operation::ActOnRelationship { correction, .. } => {
                if let Some(correction) = correction {
                    add(correction.id).expect("infallible proof-set insertion");
                    visit_new_relationship_references(correction, &mut add)
                        .expect("infallible proof-set insertion");
                }
                if let Operation::ActOnRelationship {
                    correction_expected: Some(Expected::ReadView { predicate, .. }),
                    ..
                } = operation
                {
                    add(predicate_record(predicate)).expect("infallible proof-set insertion");
                }
            }
            Operation::DeleteEntity { affected, .. } => {
                for id in affected {
                    add(*id).expect("infallible proof-set insertion");
                }
            }
        }
    }
    for (id, proof) in current {
        if needs_retained_references(transaction, *id)
            && let CurrentRecordProof::Present(record) = proof
        {
            visit_record_references(record, &mut |reference, _| {
                required.insert(reference);
            });
        }
    }
    required
}

fn needs_retained_references(transaction: &GraphTransaction, id: RecordRef) -> bool {
    transaction
        .operations()
        .iter()
        .any(|operation| match operation {
            Operation::ActOnAssertion { target, action, .. }
            | Operation::ActOnRelationship { target, action, .. } => {
                *target == id && *action != AssertionAction::Correct
            }
            Operation::Create { .. } | Operation::ReplaceEntity { .. } => false,
            Operation::DeleteEntity { affected, .. } => affected.binary_search(&id).is_ok(),
        })
}

fn add_expected_required(
    scope: NamespaceRef,
    expected: &Expected,
    required: &mut BTreeSet<RecordRef>,
    report: &mut GraphDiskPreparationReport,
    limits: &GraphDiskPreparationLimits,
) -> Result<(), GraphDiskError> {
    match expected {
        Expected::Absent | Expected::Version(_) => Ok(()),
        Expected::ReadView { predicate, .. } => {
            add_required(scope, predicate_record(predicate), required, report, limits)
        }
    }
}

const fn predicate_record(predicate: &Predicate) -> RecordRef {
    match predicate {
        Predicate::RecordAbsent(id)
        | Predicate::RecordVisible(id)
        | Predicate::RecordVersion { record: id, .. } => *id,
    }
}

fn history_proof_targets(transaction: &GraphTransaction) -> BTreeSet<RecordRef> {
    let mut targets = BTreeSet::new();
    for operation in transaction.operations() {
        collect_history_target(operation.expected(), &mut targets);
        match operation {
            Operation::ActOnAssertion {
                correction_expected,
                ..
            }
            | Operation::ActOnRelationship {
                correction_expected,
                ..
            } => {
                if let Some(expected) = correction_expected {
                    collect_history_target(expected, &mut targets);
                }
            }
            Operation::Create { .. }
            | Operation::ReplaceEntity { .. }
            | Operation::DeleteEntity { .. } => {}
        }
    }
    targets
}

fn collect_history_target(expected: &Expected, targets: &mut BTreeSet<RecordRef>) {
    if let Expected::ReadView { predicate, .. } = expected {
        targets.insert(predicate_record(predicate));
    }
}

fn reverse_proof_targets(transaction: &GraphTransaction) -> BTreeSet<RecordRef> {
    transaction
        .operations()
        .iter()
        .filter_map(|operation| match operation {
            Operation::DeleteEntity { target, .. } => Some(*target),
            Operation::Create { .. }
            | Operation::ReplaceEntity { .. }
            | Operation::ActOnAssertion { .. }
            | Operation::ActOnRelationship { .. } => None,
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn load_history_proofs<R, F>(
    reader: &R,
    filesystem: &mut F,
    root: &RecoveredIndexRoot,
    scope: NamespaceRef,
    targets: &BTreeSet<RecordRef>,
    limits: &GraphDiskPreparationLimits,
    report: &mut GraphDiskPreparationReport,
    cache: &mut PageCache,
) -> Result<BTreeMap<RecordRef, Vec<Record>>, GraphDiskError>
where
    F: OwnershipFileSystem,
    R: GraphStateIndexReader<F>,
{
    let mut output = BTreeMap::new();
    for target in targets {
        let mut versions = Vec::new();
        if has_family(root, FAMILY_RECORD_HISTORY) {
            let scan = reader
                .reader_scan_prefix(
                    filesystem,
                    root,
                    FAMILY_RECORD_HISTORY,
                    target.record().as_bytes(),
                    remaining_scan_count(report.history_versions, limits.maximum_history_versions)?,
                    remaining_scan_bytes(report, limits)?,
                    cache,
                )
                .map_err(index_reader_error)?;
            add_read_stats(report, scan.stats.clone())?;
            for entry in scan.entries {
                if entry.key.len() != 24 || &entry.key[..16] != target.record().as_bytes() {
                    return Err(GraphDiskError::IndexCorrupt);
                }
                let revision = CommitRevision::new(read_u64_be(&entry.key[16..])?)
                    .map_err(|_| GraphDiskError::IndexCorrupt)?;
                let record = decode_stored_record(&entry.value)?;
                if record.id() != *target
                    || record.modified_revision() != revision
                    || revision > root.revision()
                    || versions
                        .last()
                        .is_some_and(|prior: &Record| prior.modified_revision() >= revision)
                {
                    return Err(GraphDiskError::IndexCorrupt);
                }
                versions.push(record);
            }
            let count = u64::try_from(versions.len())
                .map_err(|_| GraphDiskError::Storage(StorageError::ResourceLimit))?;
            report.history_versions = report
                .history_versions
                .checked_add(count)
                .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
            charge_logical_u64(report, limits, scan.stats.result_bytes)?;
        }
        output.insert(*target, versions);
    }
    if output
        .keys()
        .any(|id| id.database() != scope.database() || id.namespace() != scope.namespace())
    {
        return Err(GraphDiskError::IndexCorrupt);
    }
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
fn load_reverse_proofs<R, F>(
    reader: &R,
    filesystem: &mut F,
    root: &RecoveredIndexRoot,
    scope: NamespaceRef,
    targets: &BTreeSet<RecordRef>,
    limits: &GraphDiskPreparationLimits,
    report: &mut GraphDiskPreparationReport,
    cache: &mut PageCache,
) -> Result<BTreeMap<RecordRef, BTreeMap<RecordRef, ReverseReference>>, GraphDiskError>
where
    F: OwnershipFileSystem,
    R: GraphStateIndexReader<F>,
{
    let mut output = BTreeMap::new();
    for target in targets {
        let mut owners = BTreeMap::new();
        if has_family(root, FAMILY_REVERSE) {
            let scan = reader
                .reader_scan_prefix(
                    filesystem,
                    root,
                    FAMILY_REVERSE,
                    target.record().as_bytes(),
                    remaining_scan_count(
                        report.reverse_references,
                        limits.maximum_reverse_references,
                    )?,
                    remaining_scan_bytes(report, limits)?,
                    cache,
                )
                .map_err(index_reader_error)?;
            add_read_stats(report, scan.stats.clone())?;
            for entry in scan.entries {
                if entry.key.len() != 32
                    || &entry.key[..16] != target.record().as_bytes()
                    || entry.value.len() != 24
                    || entry.value[20..] != [0; 4]
                {
                    return Err(GraphDiskError::IndexCorrupt);
                }
                let owner = record_key(scope, &entry.key[16..])?;
                let owner_version = crate::RecordVersion::new(read_u64_be(&entry.value[4..12])?)
                    .map_err(|_| GraphDiskError::IndexCorrupt)?;
                let owner_revision = CommitRevision::new(read_u64_be(&entry.value[12..20])?)
                    .map_err(|_| GraphDiskError::IndexCorrupt)?;
                if owner_revision > root.revision() {
                    return Err(GraphDiskError::IndexCorrupt);
                }
                let roles = u16::from_be_bytes(
                    entry.value[2..4]
                        .try_into()
                        .map_err(|_| GraphDiskError::IndexCorrupt)?,
                );
                if owners
                    .insert(
                        owner,
                        ReverseReference {
                            owner_kind: entry.value[0],
                            owner_state: entry.value[1],
                            roles,
                            owner_version,
                            owner_revision,
                        },
                    )
                    .is_some()
                {
                    return Err(GraphDiskError::IndexCorrupt);
                }
            }
            let count = u64::try_from(owners.len())
                .map_err(|_| GraphDiskError::Storage(StorageError::ResourceLimit))?;
            report.reverse_references = report
                .reverse_references
                .checked_add(count)
                .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
            charge_logical_u64(report, limits, scan.stats.result_bytes)?;
        }
        output.insert(*target, owners);
    }
    Ok(output)
}

fn remaining_scan_count(current: u64, maximum: u64) -> Result<usize, GraphDiskError> {
    usize::try_from(
        maximum
            .checked_sub(current)
            .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?,
    )
    .map_err(|_| GraphDiskError::Storage(StorageError::ResourceLimit))
}

fn index_reader_error(error: TransactionError) -> GraphDiskError {
    match error {
        TransactionError::Storage(StorageError::ResourceLimit) => {
            GraphDiskError::Storage(StorageError::ResourceLimit)
        }
        error => GraphDiskError::Transaction(error),
    }
}

fn remaining_scan_bytes(
    report: &GraphDiskPreparationReport,
    limits: &GraphDiskPreparationLimits,
) -> Result<usize, GraphDiskError> {
    let remaining = limits
        .maximum_proof_logical_bytes
        .checked_sub(report.proof_logical_bytes)
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?
        .min(MAX_INDEX_RESULT_BYTES as u64);
    usize::try_from(remaining).map_err(|_| GraphDiskError::Storage(StorageError::ResourceLimit))
}

fn add_required(
    scope: NamespaceRef,
    id: RecordRef,
    required: &mut BTreeSet<RecordRef>,
    report: &mut GraphDiskPreparationReport,
    limits: &GraphDiskPreparationLimits,
) -> Result<(), GraphDiskError> {
    if id.database() != scope.database() || id.namespace() != scope.namespace() {
        return Err(GraphDiskError::Graph(crate::GraphError::ScopeMismatch(id)));
    }
    charge_reference(report, limits)?;
    if !required.contains(&id) {
        if u64::try_from(required.len())
            .map_err(|_| GraphDiskError::Storage(StorageError::ResourceLimit))?
            >= limits.maximum_record_proofs
        {
            return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
        }
        charge_logical_bytes(report, limits, 16)?;
        required.insert(id);
    }
    Ok(())
}

fn add_pending(
    id: RecordRef,
    current: &BTreeMap<RecordRef, CurrentRecordProof>,
    pending: &mut BTreeSet<RecordRef>,
    report: &mut GraphDiskPreparationReport,
    limits: &GraphDiskPreparationLimits,
) -> Result<(), GraphDiskError> {
    if current.contains_key(&id) || pending.contains(&id) {
        return Ok(());
    }
    let retained = current
        .len()
        .checked_add(pending.len())
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    if u64::try_from(retained).map_err(|_| GraphDiskError::Storage(StorageError::ResourceLimit))?
        >= limits.maximum_record_proofs
    {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    charge_logical_bytes(report, limits, 16)?;
    pending.insert(id);
    Ok(())
}

fn charge_reference(
    report: &mut GraphDiskPreparationReport,
    limits: &GraphDiskPreparationLimits,
) -> Result<(), GraphDiskError> {
    if report.reference_visits == limits.maximum_reference_visits {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    report.reference_visits += 1;
    Ok(())
}

fn visit_new_record_references(
    record: &NewRecord,
    visitor: &mut impl FnMut(RecordRef) -> Result<(), GraphDiskError>,
) -> Result<(), GraphDiskError> {
    match record {
        NewRecord::Entity(entity) => visit_value_record_references(&entity.properties, visitor),
        NewRecord::Evidence(_) => Ok(()),
        NewRecord::Assertion(assertion) => visit_new_assertion_references(assertion, visitor),
        NewRecord::Relationship(relationship) => {
            visit_new_relationship_references(relationship, visitor)
        }
    }
}

fn visit_new_assertion_references(
    assertion: &NewAssertion,
    visitor: &mut impl FnMut(RecordRef) -> Result<(), GraphDiskError>,
) -> Result<(), GraphDiskError> {
    visitor(assertion.subject)?;
    visit_value_record_references(&assertion.object, visitor)?;
    for evidence in &assertion.evidence {
        visitor(*evidence)?;
    }
    Ok(())
}

fn visit_new_relationship_references(
    relationship: &NewRelationship,
    visitor: &mut impl FnMut(RecordRef) -> Result<(), GraphDiskError>,
) -> Result<(), GraphDiskError> {
    visitor(relationship.from)?;
    visitor(relationship.to)?;
    visit_value_record_references(&relationship.properties, visitor)?;
    for evidence in &relationship.evidence {
        visitor(*evidence)?;
    }
    Ok(())
}

fn visit_value_record_references(
    value: &Value,
    visitor: &mut impl FnMut(RecordRef) -> Result<(), GraphDiskError>,
) -> Result<(), GraphDiskError> {
    match value {
        Value::RecordRef(id) => visitor(*id),
        Value::List(values) => {
            for value in values.as_slice() {
                visit_value_record_references(value, visitor)?;
            }
            Ok(())
        }
        Value::Map(values) => {
            for (_, value) in values.as_slice() {
                visit_value_record_references(value, visitor)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn has_family(root: &RecoveredIndexRoot, family: u8) -> bool {
    root.runs().any(|run| run.family() == family)
}

fn charge_logical_bytes(
    report: &mut GraphDiskPreparationReport,
    limits: &GraphDiskPreparationLimits,
    bytes: usize,
) -> Result<(), GraphDiskError> {
    let bytes =
        u64::try_from(bytes).map_err(|_| GraphDiskError::Storage(StorageError::ResourceLimit))?;
    let next = report
        .proof_logical_bytes
        .checked_add(bytes)
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    if next > limits.maximum_proof_logical_bytes {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    report.proof_logical_bytes = next;
    Ok(())
}

fn charge_logical_u64(
    report: &mut GraphDiskPreparationReport,
    limits: &GraphDiskPreparationLimits,
    bytes: u64,
) -> Result<(), GraphDiskError> {
    let next = report
        .proof_logical_bytes
        .checked_add(bytes)
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    if next > limits.maximum_proof_logical_bytes {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    report.proof_logical_bytes = next;
    Ok(())
}

fn add_read_stats(
    report: &mut GraphDiskPreparationReport,
    stats: IndexReadStats,
) -> Result<(), GraphDiskError> {
    report.index_lookups = report
        .index_lookups
        .checked_add(1)
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    report.pages_read = report
        .pages_read
        .checked_add(stats.pages_read)
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    report.cache_hits = report
        .cache_hits
        .checked_add(stats.cache_hits)
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    report.fragments_visited = report
        .fragments_visited
        .checked_add(stats.fragments_visited)
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    Ok(())
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
    build_graph_state_root_delta(snapshot, base.root.anchor(), &prepared, limits)
}

/// Derive exact bounded terminal-family changes from an authenticated disk preparation proof.
///
/// This borrows the proof-backed reducer result and performs no filesystem, coordinator, cache, or
/// key-vault access. The resulting plan carries exact target counts used by bounded postcommit
/// output-stream validation; publication does not reacquire a complete graph snapshot.
pub fn prepare_graph_state_root_delta_from_disk(
    prepared: &DiskPreparedGraph,
    limits: GraphStateDeltaLimits,
) -> Result<GraphStateRootDelta, GraphDiskError> {
    build_graph_state_root_delta_from_base(
        prepared.base_anchor,
        prepared.base_counts,
        prepared.base_policy.as_ref(),
        &prepared.prepared,
        limits,
    )
}

/// Bind one proof-prepared graph change to its exact bounded terminal-root plan.
pub fn prepare_graph_disk_commit(
    prepared: DiskPreparedGraph,
    limits: GraphStateDeltaLimits,
) -> Result<GraphDiskCommit, GraphDiskError> {
    let plan = prepare_graph_state_root_delta_from_disk(&prepared, limits)?;
    Ok(GraphDiskCommit { prepared, plan })
}

impl GraphDiskLiveState {
    fn can_publish(&self, prepared: &GraphDiskCommit) -> bool {
        self.pending.is_none()
            && prepared.prepared.base_anchor == self.base.anchor()
            && prepared.prepared.base_counts == self.base.state_counts()
            && prepared.prepared.base_policy.as_ref() == self.base.namespace_policy()
            && prepared.prepared.prepared.scope == self.base.scope()
            && prepared.prepared.prepared.base_revision == Some(self.base.revision())
            && prepared.prepared.prepared.base_policy_version
                == self.base.namespace_policy().map(NamespacePolicy::version)
            && prepared.plan.scope == self.base.scope()
            && prepared.plan.base_anchor == self.base.anchor()
            && prepared.plan.base_revision == self.base.revision()
            && prepared.plan.revision == prepared.prepared.prepared.revision
            && prepared.plan.result_digest == prepared.prepared.prepared.result_digest
    }
}

impl TransactionState for GraphDiskLiveState {
    type Prepared = GraphDiskCommit;
    type Snapshot = GraphDiskLiveSnapshot;

    fn prepare(
        &self,
        _canonical_request: &[u8],
        _blob_inventory: Option<&BlobInventory>,
        _revision: CommitRevision,
    ) -> Result<Self::Prepared, ApplyError> {
        Err(ApplyError::InvalidRequest)
    }

    fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
        prepared.plan.result_digest
    }

    fn publish(&mut self, prepared: Self::Prepared) {
        assert!(
            self.can_publish(&prepared),
            "disk graph commit must publish on its exact admitted base"
        );
        let target_policy = prepared
            .prepared
            .prepared
            .policy_change
            .clone()
            .or(prepared.prepared.base_policy);
        self.pending = Some(GraphDiskPending {
            plan: prepared.plan,
            target_policy,
        });
    }

    fn snapshot(&self) -> Self::Snapshot {
        GraphDiskLiveSnapshot {
            scope: self.scope(),
            revision: self.revision(),
            logical_state_digest: self
                .pending
                .is_none()
                .then(|| *self.base.root.root.logical_state_digest()),
        }
    }
}

impl ExternallyPreparedTransactionState for GraphDiskLiveState {
    fn validate_external_prepared(
        &self,
        canonical_request: &[u8],
        blob_inventory: Option<&BlobInventory>,
        revision: CommitRevision,
        prepared: &Self::Prepared,
    ) -> Result<(), ApplyError> {
        if self.pending.is_some() {
            return Err(ApplyError::Conflict);
        }
        prepared.prepared.prepared.validate_external_request(
            canonical_request,
            blob_inventory,
            revision,
        )?;
        if !self.can_publish(prepared) {
            return Err(ApplyError::Conflict);
        }
        Ok(())
    }
}

impl JournalAnchoredTransactionState for GraphDiskLiveState {
    fn journal_base_anchor(&self) -> Result<(NamespaceRef, CommitRevision, [u8; 32]), ApplyError> {
        if self.pending.is_some() {
            return Err(ApplyError::Conflict);
        }
        Ok((
            self.scope(),
            self.base.revision(),
            *self.base.root.root.certificate_digest(),
        ))
    }
}

impl uste_txn::DiskCoordinatorState for GraphDiskLiveState {
    fn metadata_publication_input(
        &self,
        anchor: (CommitRevision, [u8; 32]),
    ) -> Result<IndexRootInput, ApplyError> {
        let root = &self.base.root.root;
        if self.pending.is_some()
            || root.generation() == 0
            || anchor != (root.revision(), *root.certificate_digest())
        {
            return Err(ApplyError::Conflict);
        }
        Ok(IndexRootInput {
            scope: root.scope(),
            revision: anchor.0,
            certificate_digest: anchor.1,
            reducer_profile: *root.reducer_profile(),
            logical_state_digest: *root.logical_state_digest(),
            index_profile: uste_txn::COORDINATOR_METADATA_PROFILE_V1,
        })
    }

    fn validate_metadata_base(&self, root: &RecoveredIndexRoot) -> Result<(), ApplyError> {
        if self.pending.is_some()
            || self.base.generation() == 0
            || self.base.anchor() != root.anchor()
        {
            return Err(ApplyError::Conflict);
        }
        Ok(())
    }
}

impl uste_txn::AuthorizedDiskPolicyState for GraphDiskLiveState {
    fn current_durable_policy(&self) -> Result<&NamespacePolicy, ApplyError> {
        self.current_base()
            .ok_or(ApplyError::Conflict)?
            .namespace_policy()
            .ok_or(ApplyError::InvalidRequest)
    }
}

impl EquivalentTransactionState for GraphState {
    type Equivalent = GraphDiskLiveState;

    fn validate_equivalent(
        &self,
        next: &GraphDiskLiveState,
        scope: NamespaceRef,
        anchor: Option<(CommitRevision, [u8; 32])>,
    ) -> Result<(), ApplyError> {
        let snapshot = self.current_snapshot();
        let digest = GraphState::logical_state_digest(snapshot).map_err(|error| match error {
            CheckpointStateError::ResourceLimit => ApplyError::ResourceLimit,
            CheckpointStateError::Invalid | CheckpointStateError::UnsupportedProfile => {
                ApplyError::Conflict
            }
        })?;
        if next.pending.is_some()
            || next.scope() != scope
            || snapshot.scope() != scope
            || snapshot.revision() != Some(next.base.revision())
            || snapshot.namespace_policy() != next.base.namespace_policy()
            || digest != *next.base.root.root.logical_state_digest()
            || anchor
                != Some((
                    next.base.revision(),
                    *next.base.root.root.certificate_digest(),
                ))
        {
            return Err(ApplyError::Conflict);
        }
        Ok(())
    }
}

impl PostCommitStateMaintenance for GraphDiskLiveState {
    type Publication = GraphDiskLivePublication;

    fn install_publication(
        &mut self,
        scope: NamespaceRef,
        anchor: Option<(CommitRevision, [u8; 32])>,
        publication: Self::Publication,
    ) -> Result<(), ApplyError> {
        let pending = self.pending.as_ref().ok_or(ApplyError::Conflict)?;
        if publication.base.scope() != scope
            || publication.base.revision() != pending.plan.revision
            || publication.base.state_counts() != pending.plan.target_counts
            || publication.base.namespace_policy() != pending.target_policy.as_ref()
            || anchor
                != Some((
                    publication.base.revision(),
                    *publication.base.root.root.certificate_digest(),
                ))
        {
            return Err(ApplyError::Conflict);
        }
        self.base = publication.base;
        self.pending = None;
        Ok(())
    }
}

/// Durably commit the exact graph request bound to a disk-prepared reducer change.
///
/// The coordinator preserves its normal retry, cancellation, journal, and publication ordering.
/// Before durable I/O, `GraphState` verifies the request bytes, target revision, policy version,
/// and every changed record's before-value against its current live base.
pub fn commit_graph_disk_prepared<F, W, E, I>(
    coordinator: &mut CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    request: TransactionRequest<'_>,
    prepared: DiskPreparedGraph,
    clock: &mut impl Clock,
    cancellation: &impl Cancellation,
) -> Result<TransactionOutcome, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    coordinator
        .commit_prepared(filesystem, request, prepared.prepared, clock, cancellation)
        .map_err(GraphDiskError::Transaction)
}

/// Durably commit one exact proof-prepared request through the warm disk-backed reducer.
pub fn commit_graph_disk_live_prepared<F, W, E, I>(
    coordinator: &mut CommitCoordinator<GraphDiskLiveState, F, W, E, I>,
    filesystem: &mut F,
    request: TransactionRequest<'_>,
    prepared: GraphDiskCommit,
    clock: &mut impl Clock,
    cancellation: &impl Cancellation,
) -> Result<TransactionOutcome, GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    coordinator
        .commit_prepared(filesystem, request, prepared, clock, cancellation)
        .map_err(GraphDiskError::Transaction)
}

trait GraphStateIndexPublisher<F>
where
    F: OwnershipFileSystem,
{
    fn graph_checkpoint_anchor(
        &self,
    ) -> Result<Option<(CommitRevision, [u8; 32])>, TransactionError>;

    #[allow(clippy::too_many_arguments)]
    fn graph_merge_index_run_visit<T>(
        &mut self,
        filesystem: &mut F,
        revision: CommitRevision,
        family: u8,
        base_root: Option<&RecoveredIndexRoot>,
        limits: IndexRunMergeLimits,
        deltas: T,
        visitor: &mut IndexRunVisitor<'_>,
    ) -> Result<uste_storage::MergedIndexRun, TransactionError>
    where
        T: IntoIterator<Item = Result<IndexDelta, StorageError>>;

    fn graph_publish_index_root_recovered(
        &mut self,
        filesystem: &mut F,
        input: IndexRootInput,
        runs: &[IndexRunDescriptor],
        fallback_limits: IndexRunReadLimits,
    ) -> Result<RecoveredIndexRoot, TransactionError>;
}

impl<S, F, W, E, I> GraphStateIndexPublisher<F> for CommitCoordinator<S, F, W, E, I>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn graph_checkpoint_anchor(
        &self,
    ) -> Result<Option<(CommitRevision, [u8; 32])>, TransactionError> {
        self.checkpoint_anchor()
    }

    fn graph_merge_index_run_visit<T>(
        &mut self,
        filesystem: &mut F,
        revision: CommitRevision,
        family: u8,
        base_root: Option<&RecoveredIndexRoot>,
        limits: IndexRunMergeLimits,
        deltas: T,
        visitor: &mut IndexRunVisitor<'_>,
    ) -> Result<uste_storage::MergedIndexRun, TransactionError>
    where
        T: IntoIterator<Item = Result<IndexDelta, StorageError>>,
    {
        self.merge_index_run_visit(
            filesystem,
            revision,
            GRAPH_STATE_PROFILE_V1,
            family,
            base_root,
            limits,
            deltas,
            visitor,
        )
    }

    fn graph_publish_index_root_recovered(
        &mut self,
        filesystem: &mut F,
        input: IndexRootInput,
        runs: &[IndexRunDescriptor],
        fallback_limits: IndexRunReadLimits,
    ) -> Result<RecoveredIndexRoot, TransactionError> {
        self.publish_index_root_recovered_bounded(filesystem, input, runs, fallback_limits)
    }
}

impl<F, W, E, I> GraphStateIndexPublisher<F> for DerivedIndexMaintenance<'_, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn graph_checkpoint_anchor(
        &self,
    ) -> Result<Option<(CommitRevision, [u8; 32])>, TransactionError> {
        Ok(self.checkpoint_anchor())
    }

    fn graph_merge_index_run_visit<T>(
        &mut self,
        filesystem: &mut F,
        revision: CommitRevision,
        family: u8,
        base_root: Option<&RecoveredIndexRoot>,
        limits: IndexRunMergeLimits,
        deltas: T,
        visitor: &mut IndexRunVisitor<'_>,
    ) -> Result<uste_storage::MergedIndexRun, TransactionError>
    where
        T: IntoIterator<Item = Result<IndexDelta, StorageError>>,
    {
        self.merge_index_run_visit(
            filesystem,
            revision,
            GRAPH_STATE_PROFILE_V1,
            family,
            base_root,
            limits,
            deltas,
            visitor,
        )
    }

    fn graph_publish_index_root_recovered(
        &mut self,
        filesystem: &mut F,
        input: IndexRootInput,
        runs: &[IndexRunDescriptor],
        fallback_limits: IndexRunReadLimits,
    ) -> Result<RecoveredIndexRoot, TransactionError> {
        self.publish_index_root_recovered_bounded(filesystem, input, runs, fallback_limits)
    }
}

/// Merge a previously prepared graph-state plan after its transaction has durably committed.
///
/// All merged runs remain invisible until their exact entries have reproduced the canonical graph
/// digest and their descriptors match the proof-derived target counts, after which one
/// certificate-bound root is atomically published. An error never rolls back or weakens the
/// already-authoritative journal commit. An in-process caller retaining the borrowed plan may
/// retry. Success returns the authenticated, semantically admitted root handle directly, without
/// a live-snapshot comparison; cold discovery after process loss remains a separate admission
/// operation.
pub fn publish_graph_state_root_delta<F, W, E, I>(
    coordinator: &mut CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    base: &DerivedGraphStateRoot,
    plan: &GraphStateRootDelta,
    outcome: TransactionOutcome,
    limits: GraphStateRootMergeLimits,
) -> Result<(DerivedGraphStateRoot, GraphStateRootMergeReport), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    publish_graph_state_root_delta_with(coordinator, filesystem, base, plan, outcome, limits)
}

/// Publish and install the pending terminal root for one warm disk-backed commit.
///
/// Any merge or publication error leaves the reducer pending and repair-only so the same outcome
/// can be retried without accepting another commit against a stale base.
pub fn publish_graph_disk_live_base<F, W, E, I>(
    coordinator: &mut CommitCoordinator<GraphDiskLiveState, F, W, E, I>,
    filesystem: &mut F,
    outcome: TransactionOutcome,
    limits: GraphStateRootMergeLimits,
) -> Result<(DerivedGraphStateRoot, GraphStateRootMergeReport), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let (published, report, publication) = publish_pending_graph_base(
        coordinator.reducer_and_index_maintenance()?,
        filesystem,
        outcome,
        limits,
    )?;
    coordinator.install_postcommit_publication(publication)?;
    Ok((published, report))
}

/// Repair/install a pending graph root while coordinator metadata remains disk-backed.
pub fn publish_graph_disk_coordinator_base<F, W, E, I>(
    coordinator: &mut uste_txn::DiskCommitCoordinator<GraphDiskLiveState, F, W, E, I>,
    filesystem: &mut F,
    outcome: TransactionOutcome,
    limits: GraphStateRootMergeLimits,
) -> Result<(DerivedGraphStateRoot, GraphStateRootMergeReport), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let (published, report, publication) = publish_pending_graph_base(
        coordinator.reducer_and_index_maintenance()?,
        filesystem,
        outcome,
        limits,
    )?;
    coordinator.install_postcommit_publication(publication)?;
    Ok((published, report))
}

fn publish_pending_graph_base<F, W, E, I>(
    mut maintenance: uste_txn::ReducerIndexMaintenance<'_, GraphDiskLiveState, F, W, E, I>,
    filesystem: &mut F,
    outcome: TransactionOutcome,
    limits: GraphStateRootMergeLimits,
) -> Result<
    (
        DerivedGraphStateRoot,
        GraphStateRootMergeReport,
        GraphDiskLivePublication,
    ),
    GraphDiskError,
>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let (published, report, counts, policy) = {
        let (base, pending) = maintenance.reducer.pending_publication(outcome)?;
        let counts = pending.plan.target_counts;
        let policy = pending.target_policy.clone();
        let (published, report) = publish_graph_state_root_delta_with(
            &mut maintenance.indexes,
            filesystem,
            base.admitted_root(),
            &pending.plan,
            outcome,
            limits,
        )?;
        (published, report, counts, policy)
    };
    let installed = GraphDiskBase {
        root: DerivedGraphStateRoot {
            root: published.root.clone(),
        },
        counts,
        current_policy: policy,
    };
    Ok((
        published,
        report,
        GraphDiskLivePublication { base: installed },
    ))
}

fn publish_graph_state_root_delta_with<P, F>(
    publisher: &mut P,
    filesystem: &mut F,
    base: &DerivedGraphStateRoot,
    plan: &GraphStateRootDelta,
    outcome: TransactionOutcome,
    limits: GraphStateRootMergeLimits,
) -> Result<(DerivedGraphStateRoot, GraphStateRootMergeReport), GraphDiskError>
where
    P: GraphStateIndexPublisher<F>,
    F: OwnershipFileSystem,
{
    if base.root.anchor() != plan.base_anchor
        || base.root.revision() != plan.base_revision
        || outcome.revision != plan.revision
        || outcome.result_digest != plan.result_digest
    {
        return Err(GraphDiskError::RootStateMismatch);
    }

    let (anchor_revision, certificate_digest) = publisher
        .graph_checkpoint_anchor()?
        .ok_or(GraphDiskError::RootStateMismatch)?;
    if anchor_revision != plan.revision {
        return Err(GraphDiskError::RootStateMismatch);
    }

    let mut report = GraphStateRootMergeReport::default();
    let mut runs = Vec::with_capacity(FAMILY_COUNT as usize);
    let mut validator = MergedGraphStateValidator::new(
        plan.scope,
        plan.revision,
        plan.target_counts,
        limits.maximum_history_group_logical_bytes,
    )?;
    for (index, deltas) in plan.families.iter().enumerate() {
        let family = u8::try_from(index + 1).map_err(|_| GraphDiskError::IndexCorrupt)?;
        let base_has_family = base.root.runs().any(|run| run.family() == family);
        let merged = publisher.graph_merge_index_run_visit(
            filesystem,
            plan.revision,
            family,
            base_has_family.then_some(&base.root),
            limits.families[index],
            deltas.iter().cloned().map(Ok),
            &mut |key, value| validator.observe(family, key, value),
        )?;
        validator.finish_family(family)?;
        let expected_entries = target_family_entries(plan.target_counts, family)?;
        match (merged.run, expected_entries) {
            (Some(run), expected) if expected != 0 && run.entry_count() == expected => {
                runs.push(run);
                report.runs = checked_sum(report.runs, 1)?;
            }
            (None, 0) => {}
            _ => return Err(GraphDiskError::IndexCorrupt),
        }
        add_merge_report(&mut report, &merged.report)?;
    }
    if report.deltas != plan.deltas || report.delta_logical_bytes != plan.logical_bytes {
        return Err(GraphDiskError::IndexCorrupt);
    }
    let logical_state_digest = validator.finish()?;

    let root = publisher.graph_publish_index_root_recovered(
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
        limits.fallback_read_limits()?,
    )?;
    Ok((DerivedGraphStateRoot { root }, report))
}

fn target_family_entries(counts: [u64; 8], family: u8) -> Result<u64, GraphDiskError> {
    match family {
        FAMILY_METADATA => Ok(1),
        FAMILY_CURRENT_RECORD..=FAMILY_REVERSE => {
            Ok(counts[usize::from(family - FAMILY_CURRENT_RECORD)])
        }
        FAMILY_POLICY => counts[6]
            .checked_add(counts[7])
            .ok_or(GraphDiskError::IndexCorrupt),
        _ => Err(GraphDiskError::IndexCorrupt),
    }
}

/// Provisional semantic observer for the exact entries emitted by all eight family merges.
///
/// The history framing requires its version count before its values. One record-local group is
/// therefore retained under an explicit bound; memory never scales with the complete base.
struct MergedGraphStateValidator {
    scope: NamespaceRef,
    revision: CommitRevision,
    counts: [u64; 8],
    next_family: u8,
    digest: Sha256,
    metadata_seen: u64,
    current_seen: u64,
    history_seen: u64,
    history_groups: u64,
    history_group_id: Option<[u8; 16]>,
    history_group_versions: u64,
    history_group_bytes: Vec<u8>,
    maximum_history_group_logical_bytes: u64,
    secondary_seen: [u64; 4],
    policy_current_seen: bool,
    policy_history_seen: u64,
}

impl MergedGraphStateValidator {
    fn new(
        scope: NamespaceRef,
        revision: CommitRevision,
        counts: [u64; 8],
        maximum_history_group_logical_bytes: u64,
    ) -> Result<Self, GraphDiskError> {
        if counts[7] > 1
            || (counts[6] != 0 && counts[7] != 1)
            || maximum_history_group_logical_bytes == 0
            || maximum_history_group_logical_bytes > MAX_INDEX_RUN_LOGICAL_BYTES
            || maximum_history_group_logical_bytes > usize::MAX as u64
        {
            return Err(GraphDiskError::IndexCorrupt);
        }
        let mut digest = Sha256::new();
        digest.update(b"USTE-GRAPH-LOGICAL-STATE-V1\0");
        digest.update(scope.database().as_bytes());
        digest.update(scope.namespace().as_bytes());
        digest.update(revision.get().to_be_bytes());
        digest.update(counts[0].to_be_bytes());
        Ok(Self {
            scope,
            revision,
            counts,
            next_family: FAMILY_METADATA,
            digest,
            metadata_seen: 0,
            current_seen: 0,
            history_seen: 0,
            history_groups: 0,
            history_group_id: None,
            history_group_versions: 0,
            history_group_bytes: Vec::new(),
            maximum_history_group_logical_bytes,
            secondary_seen: [0; 4],
            policy_current_seen: false,
            policy_history_seen: 0,
        })
    }

    fn observe(&mut self, family: u8, key: &[u8], value: &[u8]) -> Result<(), StorageError> {
        if family != self.next_family {
            return Err(StorageError::IntegrityFailure);
        }
        match family {
            FAMILY_METADATA => self.observe_metadata(key, value),
            FAMILY_CURRENT_RECORD => self.observe_current(key, value),
            FAMILY_RECORD_HISTORY => self.observe_history(key, value),
            FAMILY_OUTGOING..=FAMILY_REVERSE => self.observe_secondary(family, key, value),
            FAMILY_POLICY => self.observe_policy(key, value),
            _ => Err(StorageError::IntegrityFailure),
        }
    }

    fn finish_family(&mut self, family: u8) -> Result<(), GraphDiskError> {
        if family != self.next_family {
            return Err(GraphDiskError::IndexCorrupt);
        }
        match family {
            FAMILY_METADATA if self.metadata_seen == 1 => {}
            FAMILY_CURRENT_RECORD if self.current_seen == self.counts[0] => {
                // Every admitted graph record owns one nonempty history bucket.
                self.digest.update(self.counts[0].to_be_bytes());
            }
            FAMILY_RECORD_HISTORY => {
                self.flush_history_group()
                    .map_err(GraphDiskError::Storage)?;
                if self.history_seen != self.counts[1] || self.history_groups != self.counts[0] {
                    return Err(GraphDiskError::IndexCorrupt);
                }
            }
            FAMILY_OUTGOING..=FAMILY_REVERSE => {
                let index = usize::from(family - FAMILY_OUTGOING);
                if self.secondary_seen[index] != self.counts[index + 2] {
                    return Err(GraphDiskError::IndexCorrupt);
                }
            }
            FAMILY_POLICY => {
                if self.counts[7] == 0 {
                    if self.policy_current_seen || self.policy_history_seen != 0 {
                        return Err(GraphDiskError::IndexCorrupt);
                    }
                    let absent = encode_result_policy(None)
                        .map_err(|error| GraphDiskError::Storage(codec_storage_error(error)))?;
                    update_graph_digest_frame(&mut self.digest, &absent)
                        .map_err(GraphDiskError::Storage)?;
                    self.digest.update(0_u64.to_be_bytes());
                } else if !self.policy_current_seen || self.policy_history_seen != self.counts[6] {
                    return Err(GraphDiskError::IndexCorrupt);
                }
            }
            _ => return Err(GraphDiskError::IndexCorrupt),
        }
        self.next_family = self
            .next_family
            .checked_add(1)
            .ok_or(GraphDiskError::IndexCorrupt)?;
        Ok(())
    }

    fn finish(self) -> Result<[u8; 32], GraphDiskError> {
        if self.next_family != FAMILY_COUNT + 1 {
            return Err(GraphDiskError::IndexCorrupt);
        }
        Ok(self.digest.finalize().into())
    }

    fn observe_metadata(&mut self, key: &[u8], value: &[u8]) -> Result<(), StorageError> {
        if self.metadata_seen != 0
            || key != b"graph-state-v1"
            || value != metadata_value_from_counts(self.revision, self.counts)
        {
            return Err(StorageError::IntegrityFailure);
        }
        self.metadata_seen = 1;
        Ok(())
    }

    fn observe_current(&mut self, key: &[u8], value: &[u8]) -> Result<(), StorageError> {
        let id = record_key(self.scope, key)?;
        let record = decode_stored_record(value).map_err(codec_storage_error)?;
        if record.id() != id || record.modified_revision() > self.revision {
            return Err(StorageError::IntegrityFailure);
        }
        self.digest.update(key);
        update_graph_digest_frame(&mut self.digest, value)?;
        self.current_seen = self
            .current_seen
            .checked_add(1)
            .ok_or(StorageError::ResourceLimit)?;
        Ok(())
    }

    fn observe_history(&mut self, key: &[u8], value: &[u8]) -> Result<(), StorageError> {
        if key.len() != 24 {
            return Err(StorageError::IntegrityFailure);
        }
        let id = record_key(self.scope, &key[..16])?;
        let recorded = CommitRevision::new(read_u64_be(&key[16..])?)
            .map_err(|_| StorageError::IntegrityFailure)?;
        let record = decode_stored_record(value).map_err(codec_storage_error)?;
        if record.id() != id || record.modified_revision() != recorded || recorded > self.revision {
            return Err(StorageError::IntegrityFailure);
        }
        let raw_id: [u8; 16] = key[..16]
            .try_into()
            .map_err(|_| StorageError::IntegrityFailure)?;
        if self
            .history_group_id
            .is_some_and(|current| current != raw_id)
        {
            self.flush_history_group()?;
        }
        if self.history_group_id.is_none() {
            self.history_group_id = Some(raw_id);
        }
        let frame_bytes = 8_u64
            .checked_add(u64::try_from(value.len()).map_err(|_| StorageError::ResourceLimit)?)
            .ok_or(StorageError::ResourceLimit)?;
        let next = u64::try_from(self.history_group_bytes.len())
            .map_err(|_| StorageError::ResourceLimit)?
            .checked_add(frame_bytes)
            .ok_or(StorageError::ResourceLimit)?;
        if next > self.maximum_history_group_logical_bytes {
            return Err(StorageError::ResourceLimit);
        }
        self.history_group_bytes
            .try_reserve(usize::try_from(frame_bytes).map_err(|_| StorageError::ResourceLimit)?)
            .map_err(|_| StorageError::ResourceLimit)?;
        self.history_group_bytes.extend_from_slice(
            &u64::try_from(value.len())
                .map_err(|_| StorageError::ResourceLimit)?
                .to_be_bytes(),
        );
        self.history_group_bytes.extend_from_slice(value);
        self.history_group_versions = self
            .history_group_versions
            .checked_add(1)
            .ok_or(StorageError::ResourceLimit)?;
        self.history_seen = self
            .history_seen
            .checked_add(1)
            .ok_or(StorageError::ResourceLimit)?;
        Ok(())
    }

    fn observe_secondary(
        &mut self,
        family: u8,
        key: &[u8],
        value: &[u8],
    ) -> Result<(), StorageError> {
        if key.len() != 32 {
            return Err(StorageError::IntegrityFailure);
        }
        record_key(self.scope, &key[..16])?;
        record_key(self.scope, &key[16..])?;
        match family {
            FAMILY_OUTGOING | FAMILY_INCOMING => {
                record_key(self.scope, value)?;
            }
            FAMILY_PROVENANCE if value.is_empty() => {}
            FAMILY_REVERSE => self.validate_reverse_value(value)?,
            _ => return Err(StorageError::IntegrityFailure),
        }
        let index = usize::from(family - FAMILY_OUTGOING);
        self.secondary_seen[index] = self.secondary_seen[index]
            .checked_add(1)
            .ok_or(StorageError::ResourceLimit)?;
        Ok(())
    }

    fn validate_reverse_value(&self, value: &[u8]) -> Result<(), StorageError> {
        if value.len() != 24 || value[20..] != [0; 4] {
            return Err(StorageError::IntegrityFailure);
        }
        let kind = value[0];
        let state = value[1];
        let roles = u16::from_be_bytes(
            value[2..4]
                .try_into()
                .map_err(|_| StorageError::IntegrityFailure)?,
        );
        let allowed_roles = match kind {
            REVERSE_KIND_ENTITY
                if matches!(state, REVERSE_STATE_ACTIVE | REVERSE_STATE_DELETED) =>
            {
                REVERSE_ROLE_ENTITY_PROPERTY
            }
            REVERSE_KIND_ASSERTION if valid_reverse_claim_state(state) => {
                REVERSE_ROLE_ASSERTION_SUBJECT
                    | REVERSE_ROLE_ASSERTION_OBJECT
                    | REVERSE_ROLE_EVIDENCE
                    | REVERSE_ROLE_CORRECTION_OF
            }
            REVERSE_KIND_RELATIONSHIP if valid_reverse_claim_state(state) => {
                REVERSE_ROLE_RELATIONSHIP_FROM
                    | REVERSE_ROLE_RELATIONSHIP_TO
                    | REVERSE_ROLE_RELATIONSHIP_PROPERTY
                    | REVERSE_ROLE_EVIDENCE
                    | REVERSE_ROLE_CORRECTION_OF
            }
            _ => return Err(StorageError::IntegrityFailure),
        };
        if roles == 0 || roles & !allowed_roles != 0 {
            return Err(StorageError::IntegrityFailure);
        }
        crate::RecordVersion::new(read_u64_be(&value[4..12])?)
            .map_err(|_| StorageError::IntegrityFailure)?;
        let owner_revision = CommitRevision::new(read_u64_be(&value[12..20])?)
            .map_err(|_| StorageError::IntegrityFailure)?;
        if owner_revision > self.revision {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(())
    }

    fn flush_history_group(&mut self) -> Result<(), StorageError> {
        let Some(id) = self.history_group_id.take() else {
            return Ok(());
        };
        if self.history_group_versions == 0 {
            return Err(StorageError::IntegrityFailure);
        }
        self.digest.update(id);
        self.digest
            .update(self.history_group_versions.to_be_bytes());
        self.digest.update(&self.history_group_bytes);
        self.history_group_versions = 0;
        self.history_group_bytes.clear();
        self.history_groups = self
            .history_groups
            .checked_add(1)
            .ok_or(StorageError::ResourceLimit)?;
        Ok(())
    }

    fn observe_policy(&mut self, key: &[u8], value: &[u8]) -> Result<(), StorageError> {
        let policy = decode_result_policy(value)
            .map_err(codec_storage_error)?
            .ok_or(StorageError::IntegrityFailure)?;
        if policy.scope() != self.scope {
            return Err(StorageError::IntegrityFailure);
        }
        match key {
            [0] if self.counts[7] == 1 && !self.policy_current_seen => {
                update_graph_digest_frame(&mut self.digest, value)?;
                self.digest.update(self.counts[6].to_be_bytes());
                self.policy_current_seen = true;
            }
            [1, revision @ ..] if revision.len() == 8 && self.policy_current_seen => {
                let revision = CommitRevision::new(read_u64_be(revision)?)
                    .map_err(|_| StorageError::IntegrityFailure)?;
                if revision > self.revision {
                    return Err(StorageError::IntegrityFailure);
                }
                self.digest.update(revision.get().to_be_bytes());
                update_graph_digest_frame(&mut self.digest, value)?;
                self.policy_history_seen = self
                    .policy_history_seen
                    .checked_add(1)
                    .ok_or(StorageError::ResourceLimit)?;
            }
            _ => return Err(StorageError::IntegrityFailure),
        }
        Ok(())
    }
}

const fn valid_reverse_claim_state(state: u8) -> bool {
    matches!(
        state,
        REVERSE_STATE_PROPOSED
            | REVERSE_STATE_ACCEPTED
            | REVERSE_STATE_REJECTED
            | REVERSE_STATE_DISPUTED
            | REVERSE_STATE_SUPERSEDED
            | REVERSE_STATE_RETRACTED
            | REVERSE_STATE_EXPIRED
    )
}

fn update_graph_digest_frame(digest: &mut Sha256, value: &[u8]) -> Result<(), StorageError> {
    digest.update(
        u64::try_from(value.len())
            .map_err(|_| StorageError::ResourceLimit)?
            .to_be_bytes(),
    );
    digest.update(value);
    Ok(())
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
    prepared: &PreparedGraph,
    limits: GraphStateDeltaLimits,
) -> Result<GraphStateRootDelta, GraphDiskError> {
    if prepared.scope != snapshot.scope() || prepared.base_revision != snapshot.revision() {
        return Err(GraphDiskError::RootStateMismatch);
    }
    if prepared
        .changes
        .iter()
        .any(|change| snapshot.records.get(&change.id) != change.before.as_ref())
    {
        return Err(GraphDiskError::IndexCorrupt);
    }
    build_graph_state_root_delta_from_base(
        base_anchor,
        metadata_counts(snapshot)?,
        snapshot.policy.as_ref(),
        prepared,
        limits,
    )
}

fn build_graph_state_root_delta_from_base(
    base_anchor: IndexRootAnchor,
    base_counts: [u64; 8],
    base_policy: Option<&NamespacePolicy>,
    prepared: &PreparedGraph,
    limits: GraphStateDeltaLimits,
) -> Result<GraphStateRootDelta, GraphDiskError> {
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
        if change.after.id() != change.id || change.after.modified_revision() != prepared.revision {
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
        let before = base_policy
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
        target_counts,
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

/// Discover authenticated root manifests on the journal certificate chain without comparing them
/// to the already-live reducer or implicitly scrubbing run pages. Returned handles remain
/// provisional until full caller-bounded semantic reconstruction or admission exhausts every run.
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

/// Semantically admit one authenticated graph root without reconstructing complete graph maps.
pub fn admit_graph_disk_base_candidate<F, W, E, I>(
    coordinator: &CommitCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    candidate: &GraphStateRootCandidate,
    limits: GraphDiskBaseAdmissionLimits,
    cache: &mut PageCache,
) -> Result<(GraphDiskBase, GraphDiskBaseAdmissionReport), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    admit_graph_disk_base_with_reader(coordinator, filesystem, candidate, limits, cache)
}

/// Recovery-owner form of [`admit_graph_disk_base_candidate`].
pub fn admit_graph_disk_base_candidate_for_recovery<F, W, E, I>(
    recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
    filesystem: &mut F,
    candidate: &GraphStateRootCandidate,
    limits: GraphDiskBaseAdmissionLimits,
    cache: &mut PageCache,
) -> Result<(GraphDiskBase, GraphDiskBaseAdmissionReport), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    admit_graph_disk_base_with_reader(recovery, filesystem, candidate, limits, cache)
}

fn admit_graph_disk_base_with_reader<F, R>(
    reader: &R,
    filesystem: &mut F,
    candidate: &GraphStateRootCandidate,
    limits: GraphDiskBaseAdmissionLimits,
    cache: &mut PageCache,
) -> Result<(GraphDiskBase, GraphDiskBaseAdmissionReport), GraphDiskError>
where
    F: OwnershipFileSystem,
    R: GraphStateIndexReader<F>,
{
    validate_candidate_identity(reader, candidate)?;
    let metadata_run = candidate
        .root
        .runs()
        .find(|run| run.family() == FAMILY_METADATA)
        .ok_or(GraphDiskError::IndexCorrupt)?;
    if metadata_run.entry_count() != 1 {
        return Err(GraphDiskError::IndexCorrupt);
    }

    let mut scan = LoadBudget::new(limits.scan);
    let mut metadata_cursor =
        open_admission_cursor(reader, filesystem, candidate, FAMILY_METADATA, 1, &scan)?;
    let metadata_entry = reader
        .reader_next_cursor(filesystem, &mut metadata_cursor)
        .map_err(index_reader_error)?
        .ok_or(GraphDiskError::IndexCorrupt)?;
    if reader
        .reader_next_cursor(filesystem, &mut metadata_cursor)
        .map_err(index_reader_error)?
        .is_some()
    {
        return Err(GraphDiskError::IndexCorrupt);
    }
    finish_admission_cursor(reader, metadata_cursor, 1, &mut scan)?;
    if metadata_entry.key != b"graph-state-v1" {
        return Err(GraphDiskError::IndexCorrupt);
    }
    let metadata = parse_metadata(&metadata_entry.value, candidate.revision())
        .map_err(|_| GraphDiskError::IndexCorrupt)?;
    let family_counts = metadata.family_counts()?;
    validate_candidate_shape(candidate, metadata, &family_counts, limits.scan)?;

    let scope = candidate.root.scope();
    let revision = candidate.revision();
    let counts = metadata.state_counts();
    let mut digest_validator = MergedGraphStateValidator::new(
        scope,
        revision,
        counts,
        limits.maximum_history_group_logical_bytes,
    )?;
    digest_validator
        .observe(FAMILY_METADATA, &metadata_entry.key, &metadata_entry.value)
        .map_err(GraphDiskError::Storage)?;
    digest_validator.finish_family(FAMILY_METADATA)?;

    let mut report = GraphDiskBaseAdmissionReport::default();
    let mut derived_counts = [0_u64; 4];
    if metadata.current != 0 {
        let mut cursor = open_admission_cursor(
            reader,
            filesystem,
            candidate,
            FAMILY_CURRENT_RECORD,
            metadata.current,
            &scan,
        )?;
        while let Some(entry) = reader
            .reader_next_cursor(filesystem, &mut cursor)
            .map_err(index_reader_error)?
        {
            digest_validator
                .observe(FAMILY_CURRENT_RECORD, &entry.key, &entry.value)
                .map_err(GraphDiskError::Storage)?;
            let id = record_key(scope, &entry.key).map_err(GraphDiskError::Storage)?;
            let record = decode_stored_record(&entry.value)?;
            if record.id() != id || record.modified_revision() > revision {
                return Err(GraphDiskError::IndexCorrupt);
            }
            for (index, entries) in record_secondary_counts(&record, limits, &mut report)?
                .into_iter()
                .enumerate()
            {
                derived_counts[index] = derived_counts[index]
                    .checked_add(entries)
                    .ok_or(GraphDiskError::IndexCorrupt)?;
            }
        }
        finish_admission_cursor(reader, cursor, metadata.current, &mut scan)?;
    }
    digest_validator.finish_family(FAMILY_CURRENT_RECORD)?;
    if derived_counts
        != [
            metadata.outgoing,
            metadata.incoming,
            metadata.provenance,
            metadata.reverse,
        ]
    {
        return Err(GraphDiskError::IndexCorrupt);
    }

    let mut history_group: Option<HistoryAdmissionGroup> = None;
    if metadata.history != 0 {
        let mut cursor = open_admission_cursor(
            reader,
            filesystem,
            candidate,
            FAMILY_RECORD_HISTORY,
            metadata.history,
            &scan,
        )?;
        while let Some(entry) = reader
            .reader_next_cursor(filesystem, &mut cursor)
            .map_err(index_reader_error)?
        {
            digest_validator
                .observe(FAMILY_RECORD_HISTORY, &entry.key, &entry.value)
                .map_err(GraphDiskError::Storage)?;
            if entry.key.len() != 24 {
                return Err(GraphDiskError::IndexCorrupt);
            }
            let id = record_key(scope, &entry.key[..16]).map_err(GraphDiskError::Storage)?;
            let key_revision = CommitRevision::new(
                read_u64_be(&entry.key[16..]).map_err(GraphDiskError::Storage)?,
            )
            .map_err(|_| GraphDiskError::IndexCorrupt)?;
            let record = decode_stored_record(&entry.value)?;
            if record.id() != id
                || record.modified_revision() != key_revision
                || key_revision > revision
            {
                return Err(GraphDiskError::IndexCorrupt);
            }
            if history_group.as_ref().is_some_and(|group| group.id != id) {
                finish_history_admission_group(
                    reader,
                    filesystem,
                    candidate,
                    history_group.take().ok_or(GraphDiskError::IndexCorrupt)?,
                    limits,
                    &mut report,
                    cache,
                )?;
            }
            if history_group.is_none() {
                history_group = Some(HistoryAdmissionGroup::new(id));
            }
            let group = history_group.as_mut().ok_or(GraphDiskError::IndexCorrupt)?;
            group.observe(
                scope,
                record,
                u64::try_from(entry.value.len())
                    .map_err(|_| GraphDiskError::Storage(StorageError::ResourceLimit))?,
                limits,
            )?;
            let record = group
                .previous
                .as_ref()
                .ok_or(GraphDiskError::IndexCorrupt)?;
            if group.versions == 1 {
                visit_history_first_reference_requirements(record, &mut |requirement| {
                    validate_historical_requirement(
                        reader,
                        filesystem,
                        candidate,
                        requirement,
                        limits,
                        &mut report,
                        cache,
                    )
                })
            } else {
                visit_history_successor_reference_requirements(record, &mut |requirement| {
                    validate_historical_requirement(
                        reader,
                        filesystem,
                        candidate,
                        requirement,
                        limits,
                        &mut report,
                        cache,
                    )
                })
            }
            .map_err(graph_requirement_error)?;
            report.peak_history_group_logical_bytes = report
                .peak_history_group_logical_bytes
                .max(group.logical_bytes);
        }
        finish_admission_cursor(reader, cursor, metadata.history, &mut scan)?;
    }
    if let Some(group) = history_group.take() {
        finish_history_admission_group(
            reader,
            filesystem,
            candidate,
            group,
            limits,
            &mut report,
            cache,
        )?;
    }
    digest_validator.finish_family(FAMILY_RECORD_HISTORY)?;

    for family in FAMILY_OUTGOING..=FAMILY_REVERSE {
        let expected = family_counts[usize::from(family - 1)];
        if expected != 0 {
            let mut cursor =
                open_admission_cursor(reader, filesystem, candidate, family, expected, &scan)?;
            while let Some(entry) = reader
                .reader_next_cursor(filesystem, &mut cursor)
                .map_err(index_reader_error)?
            {
                digest_validator
                    .observe(family, &entry.key, &entry.value)
                    .map_err(GraphDiskError::Storage)?;
                validate_secondary_entry(
                    reader,
                    filesystem,
                    candidate,
                    family,
                    &entry,
                    limits,
                    &mut report,
                    cache,
                )?;
            }
            finish_admission_cursor(reader, cursor, expected, &mut scan)?;
        }
        digest_validator.finish_family(family)?;
    }

    let mut current_policy = None;
    let mut terminal_policy = None;
    let mut previous_policy_revision = None;
    let mut previous_policy_version = None;
    let policy_entries = family_counts[usize::from(FAMILY_POLICY - 1)];
    if policy_entries != 0 {
        let mut cursor = open_admission_cursor(
            reader,
            filesystem,
            candidate,
            FAMILY_POLICY,
            policy_entries,
            &scan,
        )?;
        while let Some(entry) = reader
            .reader_next_cursor(filesystem, &mut cursor)
            .map_err(index_reader_error)?
        {
            digest_validator
                .observe(FAMILY_POLICY, &entry.key, &entry.value)
                .map_err(GraphDiskError::Storage)?;
            let policy = decode_result_policy(&entry.value)?.ok_or(GraphDiskError::IndexCorrupt)?;
            if policy.scope() != scope {
                return Err(GraphDiskError::IndexCorrupt);
            }
            match entry.key.as_slice() {
                [0] if current_policy.is_none() => current_policy = Some(policy),
                [1, bytes @ ..] if bytes.len() == 8 && current_policy.is_some() => {
                    let policy_revision =
                        CommitRevision::new(read_u64_be(bytes).map_err(GraphDiskError::Storage)?)
                            .map_err(|_| GraphDiskError::IndexCorrupt)?;
                    if policy_revision > revision
                        || previous_policy_revision.is_some_and(|prior| prior >= policy_revision)
                        || previous_policy_version.is_some_and(|prior| prior >= policy.version())
                    {
                        return Err(GraphDiskError::IndexCorrupt);
                    }
                    previous_policy_revision = Some(policy_revision);
                    previous_policy_version = Some(policy.version());
                    terminal_policy = Some(policy);
                }
                _ => return Err(GraphDiskError::IndexCorrupt),
            }
        }
        finish_admission_cursor(reader, cursor, policy_entries, &mut scan)?;
    }
    digest_validator.finish_family(FAMILY_POLICY)?;
    let logical_digest = digest_validator.finish()?;
    if logical_digest != *candidate.root.logical_state_digest() {
        return Err(GraphDiskError::RootStateMismatch);
    }
    if metadata.policy_history == 0 {
        if current_policy.is_some() {
            return Err(GraphDiskError::IndexCorrupt);
        }
    } else if terminal_policy.as_ref() != current_policy.as_ref() {
        return Err(GraphDiskError::IndexCorrupt);
    }

    report.scan = scan.report;
    Ok((
        GraphDiskBase {
            root: DerivedGraphStateRoot {
                root: candidate.root.clone(),
            },
            counts,
            current_policy,
        },
        report,
    ))
}

fn validate_candidate_identity<F, R>(
    reader: &R,
    candidate: &GraphStateRootCandidate,
) -> Result<(), GraphDiskError>
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
    Ok(())
}

fn open_admission_cursor<F, R>(
    reader: &R,
    filesystem: &mut F,
    candidate: &GraphStateRootCandidate,
    family: u8,
    expected_entries: u64,
    budget: &LoadBudget,
) -> Result<IndexRunCursor<F>, GraphDiskError>
where
    F: OwnershipFileSystem,
    R: GraphStateIndexReader<F>,
{
    let limits = IndexRunReadLimits::new(
        budget.remaining_pages()?.min(MAX_INDEX_PAGES_PER_RUN),
        expected_entries,
        budget
            .remaining_logical_bytes()?
            .min(MAX_INDEX_RUN_LOGICAL_BYTES),
    )?;
    reader
        .reader_open_cursor(filesystem, &candidate.root, family, limits)
        .map_err(index_reader_error)
}

fn finish_admission_cursor<F, R>(
    reader: &R,
    cursor: IndexRunCursor<F>,
    expected_entries: u64,
    budget: &mut LoadBudget,
) -> Result<(), GraphDiskError>
where
    F: OwnershipFileSystem,
    R: GraphStateIndexReader<F>,
{
    let report = reader
        .reader_finish_cursor(cursor)
        .map_err(index_reader_error)?;
    if report.entries != expected_entries {
        return Err(GraphDiskError::IndexCorrupt);
    }
    budget.add(&report)
}

struct HistoryAdmissionGroup {
    id: RecordRef,
    versions: u64,
    logical_bytes: u64,
    previous: Option<Record>,
}

impl HistoryAdmissionGroup {
    const fn new(id: RecordRef) -> Self {
        Self {
            id,
            versions: 0,
            logical_bytes: 0,
            previous: None,
        }
    }

    fn observe(
        &mut self,
        scope: NamespaceRef,
        record: Record,
        encoded_bytes: u64,
        limits: GraphDiskBaseAdmissionLimits,
    ) -> Result<(), GraphDiskError> {
        let expected_version = self
            .versions
            .checked_add(1)
            .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
        if record.id() != self.id || record.version().get() != expected_version {
            return Err(GraphDiskError::IndexCorrupt);
        }
        if let Some(previous) = &self.previous {
            validate_history_successor(scope, previous, &record)
                .map_err(checkpoint_state_disk_error)?;
        } else {
            validate_history_first(scope, &record).map_err(checkpoint_state_disk_error)?;
        }
        let frame_bytes = 8_u64
            .checked_add(encoded_bytes)
            .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
        self.logical_bytes = self
            .logical_bytes
            .checked_add(frame_bytes)
            .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
        self.versions = expected_version;
        if self.versions > limits.maximum_history_group_versions
            || self.logical_bytes > limits.maximum_history_group_logical_bytes
        {
            return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
        }
        self.previous = Some(record);
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_history_admission_group<F, R>(
    reader: &R,
    filesystem: &mut F,
    candidate: &GraphStateRootCandidate,
    group: HistoryAdmissionGroup,
    limits: GraphDiskBaseAdmissionLimits,
    report: &mut GraphDiskBaseAdmissionReport,
    cache: &mut PageCache,
) -> Result<(), GraphDiskError>
where
    F: OwnershipFileSystem,
    R: GraphStateIndexReader<F>,
{
    let current = load_exact_current_record(
        reader, filesystem, candidate, group.id, limits, report, cache,
    )?
    .ok_or(GraphDiskError::IndexCorrupt)?;
    let terminal = group.previous.ok_or(GraphDiskError::IndexCorrupt)?;
    if current != terminal {
        return Err(GraphDiskError::IndexCorrupt);
    }
    visit_current_reference_requirements(&current, candidate.revision(), &mut |requirement| {
        validate_current_requirement(
            reader,
            filesystem,
            candidate,
            requirement,
            limits,
            report,
            cache,
        )
    })
    .map_err(graph_requirement_error)
}

#[allow(clippy::too_many_arguments)]
fn validate_historical_requirement<F, R>(
    reader: &R,
    filesystem: &mut F,
    candidate: &GraphStateRootCandidate,
    requirement: ReferenceRequirement,
    limits: GraphDiskBaseAdmissionLimits,
    report: &mut GraphDiskBaseAdmissionReport,
    cache: &mut PageCache,
) -> Result<(), GraphDiskError>
where
    F: OwnershipFileSystem,
    R: GraphStateIndexReader<F>,
{
    charge_semantic_reference_visits(report, limits, 1)?;
    charge_lookup_operation(report, limits, false)?;
    let target_record = requirement.target.record();
    let prefix = target_record.as_bytes();
    let upper = history_key(requirement.target, requirement.revision);
    let predecessor_limits = remaining_predecessor_limits(report, limits)?;
    let predecessor = reader
        .reader_get_predecessor(
            filesystem,
            &candidate.root,
            FAMILY_RECORD_HISTORY,
            prefix,
            &upper,
            predecessor_limits,
            cache,
        )
        .map_err(index_reader_error)?;
    charge_lookup_stats(report, limits, &predecessor.stats)?;
    let record = predecessor
        .entry
        .map(|entry| decode_history_lookup(&candidate.root, requirement.target, entry))
        .transpose()?;
    if !reference_requirement_matches(requirement, record.as_ref()) {
        return Err(GraphDiskError::IndexCorrupt);
    }
    Ok(())
}

fn remaining_predecessor_limits(
    report: &GraphDiskBaseAdmissionReport,
    limits: GraphDiskBaseAdmissionLimits,
) -> Result<IndexPredecessorLimits, GraphDiskError> {
    let remaining_pages = limits
        .maximum_lookup_page_visits
        .checked_sub(report.lookup_page_visits)
        .filter(|remaining| *remaining != 0)
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    let remaining_bytes = limits
        .maximum_lookup_result_bytes
        .checked_sub(report.lookup_result_bytes)
        .filter(|remaining| *remaining != 0)
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    let maximum_result_bytes = usize::try_from(remaining_bytes)
        .unwrap_or(usize::MAX)
        .min(limits.predecessor.maximum_result_bytes());
    Ok(IndexPredecessorLimits::new(
        remaining_pages.min(limits.predecessor.maximum_page_visits()),
        maximum_result_bytes,
    )?)
}

fn remaining_exact_get_limits(
    report: &GraphDiskBaseAdmissionReport,
    limits: GraphDiskBaseAdmissionLimits,
) -> Result<IndexGetLimits, GraphDiskError> {
    let remaining_pages = limits
        .maximum_lookup_page_visits
        .checked_sub(report.lookup_page_visits)
        .filter(|remaining| *remaining != 0)
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    let remaining_bytes = limits
        .maximum_lookup_result_bytes
        .checked_sub(report.lookup_result_bytes)
        .filter(|remaining| *remaining != 0)
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    let maximum_result_bytes = usize::try_from(remaining_bytes)
        .unwrap_or(usize::MAX)
        .min(MAX_INDEX_VALUE_BYTES);
    Ok(IndexGetLimits::new(
        remaining_pages.min(MAX_INDEX_GET_PAGE_VISITS),
        maximum_result_bytes,
    )?)
}

#[allow(clippy::too_many_arguments)]
fn validate_current_requirement<F, R>(
    reader: &R,
    filesystem: &mut F,
    candidate: &GraphStateRootCandidate,
    requirement: ReferenceRequirement,
    limits: GraphDiskBaseAdmissionLimits,
    report: &mut GraphDiskBaseAdmissionReport,
    cache: &mut PageCache,
) -> Result<(), GraphDiskError>
where
    F: OwnershipFileSystem,
    R: GraphStateIndexReader<F>,
{
    charge_semantic_reference_visits(report, limits, 1)?;
    let record = load_exact_current_record(
        reader,
        filesystem,
        candidate,
        requirement.target,
        limits,
        report,
        cache,
    )?;
    if !reference_requirement_matches(requirement, record.as_ref()) {
        return Err(GraphDiskError::IndexCorrupt);
    }
    Ok(())
}

fn graph_requirement_error(
    error: ReferenceRequirementVisitError<GraphDiskError>,
) -> GraphDiskError {
    match error {
        ReferenceRequirementVisitError::State(error) => checkpoint_state_disk_error(error),
        ReferenceRequirementVisitError::Visitor(error) => error,
    }
}

fn decode_history_lookup(
    candidate: &RecoveredIndexRoot,
    id: RecordRef,
    entry: uste_storage::IndexScanEntry,
) -> Result<Record, GraphDiskError> {
    if entry.key.len() != 24 || entry.key[..16] != *id.record().as_bytes() {
        return Err(GraphDiskError::IndexCorrupt);
    }
    let revision =
        CommitRevision::new(read_u64_be(&entry.key[16..]).map_err(GraphDiskError::Storage)?)
            .map_err(|_| GraphDiskError::IndexCorrupt)?;
    let record = decode_stored_record(&entry.value)?;
    if record.id() != id
        || record.modified_revision() != revision
        || revision > candidate.revision()
    {
        return Err(GraphDiskError::IndexCorrupt);
    }
    Ok(record)
}

#[allow(clippy::too_many_arguments)]
fn load_exact_current_record<F, R>(
    reader: &R,
    filesystem: &mut F,
    candidate: &GraphStateRootCandidate,
    id: RecordRef,
    limits: GraphDiskBaseAdmissionLimits,
    report: &mut GraphDiskBaseAdmissionReport,
    cache: &mut PageCache,
) -> Result<Option<Record>, GraphDiskError>
where
    F: OwnershipFileSystem,
    R: GraphStateIndexReader<F>,
{
    charge_lookup_operation(report, limits, true)?;
    let get_limits = remaining_exact_get_limits(report, limits)?;
    let (encoded, stats) = reader
        .reader_get_bounded(
            filesystem,
            &candidate.root,
            FAMILY_CURRENT_RECORD,
            id.record().as_bytes(),
            get_limits,
            cache,
        )
        .map_err(index_reader_error)?;
    charge_lookup_stats(report, limits, &stats)?;
    encoded
        .map(|encoded| {
            let record = decode_stored_record(&encoded)?;
            if record.id() != id || record.modified_revision() > candidate.revision() {
                return Err(GraphDiskError::IndexCorrupt);
            }
            Ok(record)
        })
        .transpose()
}

#[allow(clippy::too_many_arguments)]
fn validate_secondary_entry<F, R>(
    reader: &R,
    filesystem: &mut F,
    candidate: &GraphStateRootCandidate,
    family: u8,
    entry: &IndexEntry,
    limits: GraphDiskBaseAdmissionLimits,
    report: &mut GraphDiskBaseAdmissionReport,
    cache: &mut PageCache,
) -> Result<(), GraphDiskError>
where
    F: OwnershipFileSystem,
    R: GraphStateIndexReader<F>,
{
    if entry.key.len() != 32 {
        return Err(GraphDiskError::IndexCorrupt);
    }
    let owner =
        record_key(candidate.root.scope(), &entry.key[16..]).map_err(GraphDiskError::Storage)?;
    let record =
        load_exact_current_record(reader, filesystem, candidate, owner, limits, report, cache)?
            .ok_or(GraphDiskError::IndexCorrupt)?;
    if !record_contributes_secondary_entry(
        candidate.root.scope(),
        &record,
        family,
        entry,
        limits,
        report,
    )? {
        return Err(GraphDiskError::IndexCorrupt);
    }
    Ok(())
}

fn record_secondary_counts(
    record: &Record,
    limits: GraphDiskBaseAdmissionLimits,
    report: &mut GraphDiskBaseAdmissionReport,
) -> Result<[u64; 4], GraphDiskError> {
    let mut counts = [0_u64; 4];
    match record {
        Record::Relationship(relationship) => {
            if relationship.status == crate::AssertionStatus::Accepted {
                counts[0] = 1;
                counts[1] = 1;
            }
            counts[2] = u64::try_from(relationship.evidence.len())
                .map_err(|_| GraphDiskError::Storage(StorageError::ResourceLimit))?;
        }
        Record::Assertion(assertion) => {
            counts[2] = u64::try_from(assertion.evidence.len())
                .map_err(|_| GraphDiskError::Storage(StorageError::ResourceLimit))?;
        }
        Record::Entity(_) | Record::Evidence(_) => {}
    }
    let mut targets = BTreeSet::new();
    try_visit_record_references(record, &mut |target, _| {
        charge_semantic_reference_visits(report, limits, 1)?;
        targets.insert(target);
        Ok::<(), GraphDiskError>(())
    })?;
    counts[3] = u64::try_from(targets.len())
        .map_err(|_| GraphDiskError::Storage(StorageError::ResourceLimit))?;
    Ok(counts)
}

fn record_contributes_secondary_entry(
    scope: NamespaceRef,
    record: &Record,
    family: u8,
    entry: &IndexEntry,
    limits: GraphDiskBaseAdmissionLimits,
    report: &mut GraphDiskBaseAdmissionReport,
) -> Result<bool, GraphDiskError> {
    let target = record_key(scope, &entry.key[..16]).map_err(GraphDiskError::Storage)?;
    match (family, record) {
        (FAMILY_OUTGOING, Record::Relationship(relationship)) => Ok(relationship.status
            == crate::AssertionStatus::Accepted
            && relationship.from == target
            && entry.value == relationship.to.record().as_bytes()),
        (FAMILY_INCOMING, Record::Relationship(relationship)) => Ok(relationship.status
            == crate::AssertionStatus::Accepted
            && relationship.to == target
            && entry.value == relationship.from.record().as_bytes()),
        (FAMILY_PROVENANCE, Record::Assertion(assertion)) => {
            evidence_contains(&assertion.evidence, target, limits, report)
        }
        (FAMILY_PROVENANCE, Record::Relationship(relationship)) => {
            evidence_contains(&relationship.evidence, target, limits, report)
        }
        (FAMILY_REVERSE, _) => {
            let mut roles = 0_u16;
            try_visit_record_references(record, &mut |candidate, role| {
                charge_semantic_reference_visits(report, limits, 1)?;
                if candidate == target {
                    roles |= role;
                }
                Ok::<(), GraphDiskError>(())
            })?;
            Ok(roles != 0 && entry.value == reverse_value(reverse_reference(record, roles)))
        }
        _ => Ok(false),
    }
}

fn evidence_contains(
    evidence: &[RecordRef],
    target: RecordRef,
    limits: GraphDiskBaseAdmissionLimits,
    report: &mut GraphDiskBaseAdmissionReport,
) -> Result<bool, GraphDiskError> {
    for candidate in evidence {
        charge_semantic_reference_visits(report, limits, 1)?;
        if *candidate == target {
            return Ok(true);
        }
    }
    Ok(false)
}

fn charge_lookup_operation(
    report: &mut GraphDiskBaseAdmissionReport,
    limits: GraphDiskBaseAdmissionLimits,
    exact: bool,
) -> Result<(), GraphDiskError> {
    let total = report
        .exact_lookups
        .checked_add(report.predecessor_lookups)
        .and_then(|value| value.checked_add(1))
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    if total > limits.maximum_lookup_operations {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    if exact {
        report.exact_lookups = report
            .exact_lookups
            .checked_add(1)
            .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    } else {
        report.predecessor_lookups = report
            .predecessor_lookups
            .checked_add(1)
            .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    }
    Ok(())
}

fn charge_semantic_reference_visits(
    report: &mut GraphDiskBaseAdmissionReport,
    limits: GraphDiskBaseAdmissionLimits,
    visits: u64,
) -> Result<(), GraphDiskError> {
    report.semantic_reference_visits = report
        .semantic_reference_visits
        .checked_add(visits)
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    if report.semantic_reference_visits > limits.maximum_semantic_reference_visits {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    Ok(())
}

fn charge_lookup_stats(
    report: &mut GraphDiskBaseAdmissionReport,
    limits: GraphDiskBaseAdmissionLimits,
    stats: &IndexReadStats,
) -> Result<(), GraphDiskError> {
    report.lookup_page_visits = report
        .lookup_page_visits
        .checked_add(stats.pages_read)
        .and_then(|value| value.checked_add(stats.cache_hits))
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    report.lookup_result_bytes = report
        .lookup_result_bytes
        .checked_add(stats.result_bytes)
        .ok_or(GraphDiskError::Storage(StorageError::ResourceLimit))?;
    if report.lookup_page_visits > limits.maximum_lookup_page_visits
        || report.lookup_result_bytes > limits.maximum_lookup_result_bytes
    {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    Ok(())
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
        .reader_load_root_manifests(filesystem, GRAPH_STATE_PROFILE_V1)?
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
    const fn state_counts(self) -> [u64; 8] {
        [
            self.current,
            self.history,
            self.outgoing,
            self.incoming,
            self.provenance,
            self.reverse,
            self.policy_history,
            self.current_policy,
        ]
    }

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

fn validate_metadata_against_root(
    root: &RecoveredIndexRoot,
    metadata: GraphStateMetadata,
) -> Result<(), GraphDiskError> {
    if metadata.current_policy > 1 || (metadata.policy_history != 0 && metadata.current_policy != 1)
    {
        return Err(GraphDiskError::IndexCorrupt);
    }
    let expected = metadata
        .family_counts()?
        .into_iter()
        .enumerate()
        .filter(|(_, count)| *count != 0)
        .map(|(index, count)| (u8::try_from(index + 1).unwrap(), count));
    if !root
        .runs()
        .map(|run| (run.family(), run.entry_count()))
        .eq(expected)
    {
        return Err(GraphDiskError::IndexCorrupt);
    }
    Ok(())
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
        || metadata.history < metadata.current
        || (metadata.current == 0
            && [
                metadata.history,
                metadata.outgoing,
                metadata.incoming,
                metadata.provenance,
                metadata.reverse,
            ]
            .into_iter()
            .any(|count| count != 0))
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

fn record_key(scope: NamespaceRef, key: &[u8]) -> Result<RecordRef, StorageError> {
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

fn validate_current_root<S, F, W, E, I>(
    coordinator: &CommitCoordinator<S, F, W, E, I>,
    root: &DerivedGraphStateRoot,
) -> Result<(), GraphDiskError>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    validate_root_anchor(coordinator.checkpoint_anchor()?, root)
}

fn validate_root_anchor(
    anchor: Option<(CommitRevision, [u8; 32])>,
    root: &DerivedGraphStateRoot,
) -> Result<(), GraphDiskError> {
    let Some((revision, certificate_digest)) = anchor else {
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

    #[test]
    fn admission_lookup_work_is_independent_of_single_scan_capacity() {
        let scan = GraphStateLoadLimits::new(1, 1, 1, 8, 8, 4096).unwrap();
        let predecessor = IndexPredecessorLimits::new(64, 16 * 1024).unwrap();
        let limits = |operations, pages, bytes| {
            GraphDiskBaseAdmissionLimits::new(
                scan,
                1,
                4096,
                1,
                operations,
                pages,
                bytes,
                predecessor,
            )
        };
        // Repeated proofs can exceed one traversal of the format's eight possible runs.
        assert!(
            limits(
                200_000,
                MAX_INDEX_PAGES_PER_RUN * 8 + 1,
                MAX_INDEX_RUN_LOGICAL_BYTES * 8 + 1
            )
            .is_ok()
        );
        assert!(
            GraphStateLoadLimits::new(1, 1, 1, 8, MAX_INDEX_PAGES_PER_RUN * 8 + 1, 4096).is_err()
        );
        assert!(limits(1, MAX_INDEX_GET_PAGE_VISITS, MAX_INDEX_VALUE_BYTES as u64).is_ok());
        assert!(limits(1, MAX_INDEX_GET_PAGE_VISITS + 1, 1).is_err());
        assert!(limits(1, 1, MAX_INDEX_VALUE_BYTES as u64 + 1).is_err());
        assert!(limits(0, 1, 1).is_err());
        assert!(limits(1, 0, 1).is_err());
        assert!(limits(1, 1, 0).is_err());
        let large_predecessor = IndexPredecessorLimits::new(
            uste_storage::MAX_INDEX_PREDECESSOR_PAGE_VISITS,
            uste_storage::MAX_INDEX_KEY_BYTES + MAX_INDEX_VALUE_BYTES,
        )
        .unwrap();
        assert!(
            GraphDiskBaseAdmissionLimits::new(
                scan,
                1,
                4096,
                1,
                1,
                large_predecessor.maximum_page_visits(),
                large_predecessor.maximum_result_bytes() as u64,
                large_predecessor,
            )
            .is_ok()
        );
        let tiny = limits(2, 2, 2).unwrap();
        let mut counted = GraphDiskBaseAdmissionReport::default();
        charge_lookup_stats(
            &mut counted,
            tiny,
            &IndexReadStats {
                pages_read: 1,
                cache_hits: 1,
                result_bytes: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(counted.lookup_page_visits, 2);
        assert!(remaining_exact_get_limits(&counted, tiny).is_err());
        assert!(remaining_predecessor_limits(&counted, tiny).is_err());
        assert!(
            charge_lookup_stats(
                &mut counted,
                tiny,
                &IndexReadStats {
                    cache_hits: 1,
                    ..Default::default()
                }
            )
            .is_err()
        );
        let max_operations = MAX_INDEX_ENTRIES_PER_RUN * crate::MAX_TRANSACTION_REFERENCES as u64;
        let maximal = limits(max_operations, u64::MAX, u64::MAX).unwrap();
        assert!(limits(max_operations + 1, 1, 1).is_err());
        // Configuration multiplication may saturate; runtime counters must never wrap.
        let mut report = GraphDiskBaseAdmissionReport {
            lookup_page_visits: u64::MAX,
            ..Default::default()
        };
        assert!(
            charge_lookup_stats(
                &mut report,
                maximal,
                &IndexReadStats {
                    cache_hits: 1,
                    ..Default::default()
                }
            )
            .is_err()
        );
        report.lookup_page_visits = 0;
        report.lookup_result_bytes = u64::MAX;
        assert!(
            charge_lookup_stats(
                &mut report,
                maximal,
                &IndexReadStats {
                    result_bytes: 1,
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(remaining_exact_get_limits(&report, maximal).is_err());
        assert!(remaining_predecessor_limits(&report, maximal).is_err());
    }

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

    fn validator_before(snapshot: &GraphSnapshot, target_family: u8) -> MergedGraphStateValidator {
        let revision = snapshot.revision().unwrap();
        let mut validator = MergedGraphStateValidator::new(
            scope(),
            revision,
            metadata_counts(snapshot).unwrap(),
            2 * 1024 * 1024,
        )
        .unwrap();
        for family in FAMILY_METADATA..target_family {
            for entry in family_entries(snapshot, revision, family).unwrap() {
                let entry = entry.unwrap();
                validator.observe(family, &entry.key, &entry.value).unwrap();
            }
            validator.finish_family(family).unwrap();
        }
        validator
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
    fn merged_output_stream_reproduces_canonical_digest_under_record_local_bound() {
        let (snapshot, _) = canonical_fixture();
        let revision = snapshot.revision().unwrap();
        let counts = metadata_counts(&snapshot).unwrap();
        let mut validator =
            MergedGraphStateValidator::new(scope(), revision, counts, 2 * 1024 * 1024).unwrap();
        for family in FAMILY_METADATA..=FAMILY_COUNT {
            for entry in family_entries(&snapshot, revision, family).unwrap() {
                let entry = entry.unwrap();
                validator.observe(family, &entry.key, &entry.value).unwrap();
            }
            validator.finish_family(family).unwrap();
        }
        assert_eq!(
            validator.finish().unwrap(),
            GraphState::logical_state_digest(&snapshot).unwrap()
        );

        let mut bounded = MergedGraphStateValidator::new(scope(), revision, counts, 1).unwrap();
        for family in FAMILY_METADATA..FAMILY_RECORD_HISTORY {
            for entry in family_entries(&snapshot, revision, family).unwrap() {
                let entry = entry.unwrap();
                bounded.observe(family, &entry.key, &entry.value).unwrap();
            }
            bounded.finish_family(family).unwrap();
        }
        let history = family_entries(&snapshot, revision, FAMILY_RECORD_HISTORY)
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert_eq!(
            bounded.observe(FAMILY_RECORD_HISTORY, &history.key, &history.value),
            Err(StorageError::ResourceLimit)
        );
    }

    #[test]
    fn merged_output_stream_rejects_malformed_secondary_entries_and_count_mismatch() {
        let (snapshot, _) = canonical_fixture();

        let mut outgoing = entries(&snapshot, FAMILY_OUTGOING).remove(0);
        outgoing.key.pop();
        assert_eq!(
            validator_before(&snapshot, FAMILY_OUTGOING).observe(
                FAMILY_OUTGOING,
                &outgoing.key,
                &outgoing.value,
            ),
            Err(StorageError::IntegrityFailure)
        );

        let mut incoming = entries(&snapshot, FAMILY_INCOMING).remove(0);
        incoming.value.pop();
        assert_eq!(
            validator_before(&snapshot, FAMILY_INCOMING).observe(
                FAMILY_INCOMING,
                &incoming.key,
                &incoming.value,
            ),
            Err(StorageError::IntegrityFailure)
        );

        let mut provenance = entries(&snapshot, FAMILY_PROVENANCE).remove(0);
        provenance.value.push(1);
        assert_eq!(
            validator_before(&snapshot, FAMILY_PROVENANCE).observe(
                FAMILY_PROVENANCE,
                &provenance.key,
                &provenance.value,
            ),
            Err(StorageError::IntegrityFailure)
        );

        let mut reverse = entries(&snapshot, FAMILY_REVERSE).remove(0);
        reverse.value[2..4].copy_from_slice(&REVERSE_ROLE_ENTITY_PROPERTY.to_be_bytes());
        assert_eq!(
            validator_before(&snapshot, FAMILY_REVERSE).observe(
                FAMILY_REVERSE,
                &reverse.key,
                &reverse.value,
            ),
            Err(StorageError::IntegrityFailure)
        );

        assert_eq!(
            validator_before(&snapshot, FAMILY_OUTGOING).finish_family(FAMILY_OUTGOING),
            Err(GraphDiskError::IndexCorrupt)
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
