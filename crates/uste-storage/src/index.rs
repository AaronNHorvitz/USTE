//! Encrypted immutable sorted runs and certificate-anchored derived index roots.
//!
//! Index roots are optional caches. The authenticated journal remains the only commit authority;
//! a missing, stale, unsupported or corrupt root must be rebuilt from retained authority.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use uste_crypto::{
    CryptoContext, CryptoError, CryptoObjectId, EncryptedEnvelope, EntropySource, FrameClass,
    KeyEpoch, KeyVault, ObjectRole, Scope, SecretBytes, WriterIncarnationId,
};
use uste_types::{CommitRevision, DatabaseId, NamespaceId, NamespaceRef};
use zeroize::Zeroizing;

use crate::{
    AdapterErrorKind, EntryName, FileSystem,
    journal::{DurableKeyEnvelope, StorageError},
    read_exact_at, write_all_at,
};

pub const INDEX_PAGE_BYTES: usize = 16 * 1024;
pub const MAX_INDEX_KEY_BYTES: usize = 4 * 1024;
pub const MAX_INDEX_VALUE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_INDEX_RUNS: usize = 16;
pub const MAX_INDEX_PAGES_PER_RUN: u64 = 16 * 1024 * 1024;
pub const MAX_INDEX_ENTRIES_PER_RUN: u64 = 1_000_000_000;
pub const MAX_INDEX_CACHE_BYTES: usize = 256 * 1024 * 1024;
pub const DEFAULT_INDEX_CACHE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_INDEX_SCAN_RESULTS: usize = 1_000_000;
pub const MAX_INDEX_RESULT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_INDEX_RUN_LOGICAL_BYTES: u64 = MAX_INDEX_PAGES_PER_RUN * INDEX_PAGE_BYTES as u64;
pub const MAX_INDEX_DELTA_LOGICAL_BYTES: u64 = MAX_INDEX_RUN_LOGICAL_BYTES * 2;
pub const MAX_INDEX_GET_PAGE_VISITS: u64 = MAX_INDEX_PAGES_PER_RUN + 64;
// Three full-run passes cover fragment-start backtracking, key selection and selected-value
// assembly. The additive allowance covers the binary-search path (at most 25 visits at the
// admitted run ceiling) and the repeated boundary page, including the one-page-run case.
pub const MAX_INDEX_PREDECESSOR_PAGE_VISITS: u64 = MAX_INDEX_PAGES_PER_RUN * 3 + 64;

const PAGE_HEADER_BYTES: usize = 80;
const FRAGMENT_HEADER_BYTES: usize = 16;
const PAGE_MAGIC: &[u8; 4] = b"UIPG";
const ROOT_MAGIC: &[u8; 4] = b"UIRT";
const MAJOR: u8 = 1;
const MINOR: u8 = 0;
pub(crate) const ENCODED_PAGE_BYTES: u64 = 20_545;
const SMALL_ENVELOPE_BYTES: usize = 4_161;
const ROOT_BYTES: usize = 2_048;
const ROOT_HEADER_BYTES: usize = 192;
const RUN_DESCRIPTOR_BYTES: usize = 72;
const ROOT_FILE_BYTES: u64 = 16 + SMALL_ENVELOPE_BYTES as u64;
// Logical admission allowances for both ordered maps, including sparse root nodes. These
// are not allocator/RSS measurements; qualification must still measure the process group.
const CACHE_FIXED_OVERHEAD: usize = 8 * 1024;
const CACHE_ENTRY_OVERHEAD: usize = 1024;
const CACHE_ENTRY_BYTES: usize = INDEX_PAGE_BYTES + CACHE_ENTRY_OVERHEAD;
pub const MIN_INDEX_CACHE_BYTES: usize = CACHE_FIXED_OVERHEAD + CACHE_ENTRY_BYTES;

#[derive(Clone, Eq, PartialEq)]
pub struct IndexEntry {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

/// One exact before/after mutation in a sorted merge overlay.
///
/// `before == None` requires the key to be absent. `after == None` is a tombstone. At least one
/// side must be present, and a present `before` is compared byte-for-byte with the authenticated
/// base run before any replacement is emitted.
#[derive(Clone, Eq, PartialEq)]
pub struct IndexDelta {
    key: Vec<u8>,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

impl core::fmt::Debug for IndexDelta {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("IndexDelta")
            .field("key", &"[REDACTED]")
            .field("before", &self.before.as_ref().map(|value| value.len()))
            .field("after", &self.after.as_ref().map(|value| value.len()))
            .finish()
    }
}

impl IndexDelta {
    pub fn new(
        key: Vec<u8>,
        before: Option<Vec<u8>>,
        after: Option<Vec<u8>>,
    ) -> Result<Self, StorageError> {
        if key.is_empty()
            || key.len() > MAX_INDEX_KEY_BYTES
            || before.is_none() && after.is_none()
            || before
                .as_ref()
                .is_some_and(|value| value.len() > MAX_INDEX_VALUE_BYTES)
            || after
                .as_ref()
                .is_some_and(|value| value.len() > MAX_INDEX_VALUE_BYTES)
        {
            return Err(StorageError::InvalidState);
        }
        Ok(Self { key, before, after })
    }

    #[must_use]
    pub fn key(&self) -> &[u8] {
        &self.key
    }

    #[must_use]
    pub fn before(&self) -> Option<&[u8]> {
        self.before.as_deref()
    }

    #[must_use]
    pub fn after(&self) -> Option<&[u8]> {
        self.after.as_deref()
    }

    fn logical_bytes(&self) -> Result<u64, StorageError> {
        u64::try_from(
            self.key
                .len()
                .checked_add(self.before.as_ref().map_or(0, Vec::len))
                .and_then(|bytes| bytes.checked_add(self.after.as_ref().map_or(0, Vec::len)))
                .ok_or(StorageError::ResourceLimit)?,
        )
        .map_err(|_| StorageError::ResourceLimit)
    }
}

impl core::fmt::Debug for IndexEntry {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("IndexEntry")
            .field("key", &"[REDACTED]")
            .field("value", &"[REDACTED]")
            .field("key_len", &self.key.len())
            .field("value_len", &self.value.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexRootInput {
    pub scope: NamespaceRef,
    pub revision: CommitRevision,
    pub certificate_digest: [u8; 32],
    pub reducer_profile: [u8; 32],
    pub logical_state_digest: [u8; 32],
    pub index_profile: [u8; 32],
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct IndexRunDescriptor {
    scope: NamespaceRef,
    revision: CommitRevision,
    index_profile: [u8; 32],
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    family: u8,
    object_id: [u8; 16],
    page_count: u64,
    entry_count: u64,
    logical_digest: [u8; 32],
}

impl core::fmt::Debug for IndexRunDescriptor {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("IndexRunDescriptor")
            .field("family", &self.family)
            .field("object_id", &"[REDACTED]")
            .field("page_count", &self.page_count)
            .field("entry_count", &self.entry_count)
            .field("logical_digest", &"[REDACTED]")
            .finish()
    }
}

impl IndexRunDescriptor {
    #[must_use]
    pub const fn family(&self) -> u8 {
        self.family
    }

    #[must_use]
    pub const fn page_count(&self) -> u64 {
        self.page_count
    }

    #[must_use]
    pub const fn entry_count(&self) -> u64 {
        self.entry_count
    }

    #[must_use]
    pub const fn logical_digest(&self) -> &[u8; 32] {
        &self.logical_digest
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DurableIndexRoot {
    pub revision: CommitRevision,
    pub generation: u64,
}

/// Cross-profile identity for pairing optional roots at one exact journal certificate and reducer
/// state. Local root generations and index profiles are intentionally excluded.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct IndexRootAnchor {
    scope: NamespaceRef,
    revision: CommitRevision,
    certificate_digest: [u8; 32],
    reducer_profile: [u8; 32],
    logical_state_digest: [u8; 32],
}

impl core::fmt::Debug for IndexRootAnchor {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("IndexRootAnchor")
            .field("scope", &"[REDACTED]")
            .field("revision", &self.revision)
            .field("certificate_digest", &"[REDACTED]")
            .field("reducer_profile", &"[REDACTED]")
            .field("logical_state_digest", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct RecoveredIndexRoot {
    scope: NamespaceRef,
    revision: CommitRevision,
    generation: u64,
    certificate_digest: [u8; 32],
    reducer_profile: [u8; 32],
    logical_state_digest: [u8; 32],
    index_profile: [u8; 32],
    runs: Vec<IndexRunDescriptor>,
}

impl core::fmt::Debug for RecoveredIndexRoot {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("RecoveredIndexRoot")
            .field("scope", &"[REDACTED]")
            .field("revision", &self.revision)
            .field("generation", &self.generation)
            .field("certificate_digest", &"[REDACTED]")
            .field("reducer_profile", &"[REDACTED]")
            .field("logical_state_digest", &"[REDACTED]")
            .field("index_profile", &"[REDACTED]")
            .field("run_count", &self.runs.len())
            .finish()
    }
}

impl RecoveredIndexRoot {
    #[must_use]
    pub const fn anchor(&self) -> IndexRootAnchor {
        IndexRootAnchor {
            scope: self.scope,
            revision: self.revision,
            certificate_digest: self.certificate_digest,
            reducer_profile: self.reducer_profile,
            logical_state_digest: self.logical_state_digest,
        }
    }

    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.scope
    }

    #[must_use]
    pub const fn revision(&self) -> CommitRevision {
        self.revision
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn certificate_digest(&self) -> &[u8; 32] {
        &self.certificate_digest
    }

    #[must_use]
    pub const fn reducer_profile(&self) -> &[u8; 32] {
        &self.reducer_profile
    }

    #[must_use]
    pub const fn logical_state_digest(&self) -> &[u8; 32] {
        &self.logical_state_digest
    }

    #[must_use]
    pub const fn index_profile(&self) -> &[u8; 32] {
        &self.index_profile
    }

    pub fn runs(&self) -> impl ExactSizeIterator<Item = &IndexRunDescriptor> {
        self.runs.iter()
    }

    fn run(&self, family: u8) -> Result<&IndexRunDescriptor, StorageError> {
        let mut matches = self.runs.iter().filter(|run| run.family == family);
        let run = matches.next().ok_or(StorageError::InvalidState)?;
        if matches.next().is_some() {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(run)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexScanEntry {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IndexReadStats {
    pub pages_read: u64,
    pub cache_hits: u64,
    pub fragments_visited: u64,
    pub result_bytes: u64,
}

/// Cumulative cached exact/predecessor/prefix operation work, including work before errors.
/// Excludes uncached cursors, scrubs, publication and checks before entering these primitives.
/// Result bytes on failed scans can include provisional visitor output, not accepted results.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IndexReadTelemetry {
    pub completed_operations: u64,
    pub failed_operations: u64,
    pub work: IndexReadStats,
}

impl IndexReadTelemetry {
    fn record(&mut self, stats: &IndexReadStats, success: bool) -> Option<()> {
        let mut next = self.clone();
        let count = if success {
            &mut next.completed_operations
        } else {
            &mut next.failed_operations
        };
        *count = count.checked_add(1)?;
        next.work.pages_read = next.work.pages_read.checked_add(stats.pages_read)?;
        next.work.cache_hits = next.work.cache_hits.checked_add(stats.cache_hits)?;
        next.work.fragments_visited = next
            .work
            .fragments_visited
            .checked_add(stats.fragments_visited)?;
        next.work.result_bytes = next.work.result_bytes.checked_add(stats.result_bytes)?;
        *self = next;
        Some(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexScan {
    pub entries: Vec<IndexScanEntry>,
    pub stats: IndexReadStats,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexPredecessor {
    pub entry: Option<IndexScanEntry>,
    pub stats: IndexReadStats,
}

/// Per-operation work bounds for an authenticated exact-key lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexGetLimits {
    maximum_page_visits: u64,
    maximum_result_bytes: usize,
}

impl IndexGetLimits {
    pub const fn new(
        maximum_page_visits: u64,
        maximum_result_bytes: usize,
    ) -> Result<Self, StorageError> {
        if maximum_page_visits == 0
            || maximum_page_visits > MAX_INDEX_GET_PAGE_VISITS
            || maximum_result_bytes == 0
            || maximum_result_bytes > MAX_INDEX_VALUE_BYTES
        {
            return Err(StorageError::ResourceLimit);
        }
        Ok(Self {
            maximum_page_visits,
            maximum_result_bytes,
        })
    }

    #[must_use]
    pub const fn maximum_page_visits(self) -> u64 {
        self.maximum_page_visits
    }

    #[must_use]
    pub const fn maximum_result_bytes(self) -> usize {
        self.maximum_result_bytes
    }
}

/// Prefix-scan work bounds, including binary-search and cache-hit page visits.
/// Result bytes count keys and values; zero result/count budgets can prove absence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexScanLimits {
    maximum_page_visits: u64,
    maximum_results: usize,
    maximum_result_bytes: usize,
}

impl IndexScanLimits {
    pub const fn new(
        maximum_page_visits: u64,
        maximum_results: usize,
        maximum_result_bytes: usize,
    ) -> Result<Self, StorageError> {
        if maximum_page_visits == 0
            || maximum_page_visits > MAX_INDEX_GET_PAGE_VISITS
            || maximum_results > MAX_INDEX_SCAN_RESULTS
            || maximum_result_bytes > MAX_INDEX_RESULT_BYTES
        {
            return Err(StorageError::ResourceLimit);
        }
        Ok(Self {
            maximum_page_visits,
            maximum_results,
            maximum_result_bytes,
        })
    }
    #[must_use]
    pub const fn maximum_page_visits(self) -> u64 {
        self.maximum_page_visits
    }
    #[must_use]
    pub const fn maximum_results(self) -> usize {
        self.maximum_results
    }
    #[must_use]
    pub const fn maximum_result_bytes(self) -> usize {
        self.maximum_result_bytes
    }
}

/// Per-operation work bounds for an authenticated greatest-key-at-or-before lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexPredecessorLimits {
    maximum_page_visits: u64,
    maximum_result_bytes: usize,
}

impl IndexPredecessorLimits {
    pub const fn new(
        maximum_page_visits: u64,
        maximum_result_bytes: usize,
    ) -> Result<Self, StorageError> {
        if maximum_page_visits == 0
            || maximum_page_visits > MAX_INDEX_PREDECESSOR_PAGE_VISITS
            || maximum_result_bytes == 0
            || maximum_result_bytes > MAX_INDEX_KEY_BYTES + MAX_INDEX_VALUE_BYTES
        {
            return Err(StorageError::ResourceLimit);
        }
        Ok(Self {
            maximum_page_visits,
            maximum_result_bytes,
        })
    }

    #[must_use]
    pub const fn maximum_page_visits(self) -> u64 {
        self.maximum_page_visits
    }

    #[must_use]
    pub const fn maximum_result_bytes(self) -> usize {
        self.maximum_result_bytes
    }
}

/// Explicit work bounds for a privileged, complete immutable-run read.
///
/// Unlike a consumer prefix scan, this permits more than one million entries and 64 MiB of
/// logical data, but never more than the immutable format's absolute run bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexRunReadLimits {
    maximum_pages: u64,
    maximum_entries: u64,
    maximum_logical_bytes: u64,
}

impl IndexRunReadLimits {
    pub const fn new(
        maximum_pages: u64,
        maximum_entries: u64,
        maximum_logical_bytes: u64,
    ) -> Result<Self, StorageError> {
        if maximum_pages == 0
            || maximum_pages > MAX_INDEX_PAGES_PER_RUN
            || maximum_entries == 0
            || maximum_entries > MAX_INDEX_ENTRIES_PER_RUN
            || maximum_logical_bytes == 0
            || maximum_logical_bytes > MAX_INDEX_RUN_LOGICAL_BYTES
        {
            return Err(StorageError::ResourceLimit);
        }
        Ok(Self {
            maximum_pages,
            maximum_entries,
            maximum_logical_bytes,
        })
    }

    #[must_use]
    pub const fn maximum_pages(self) -> u64 {
        self.maximum_pages
    }

    #[must_use]
    pub const fn maximum_entries(self) -> u64 {
        self.maximum_entries
    }

    #[must_use]
    pub const fn maximum_logical_bytes(self) -> u64 {
        self.maximum_logical_bytes
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IndexRunReadReport {
    pub entries: u64,
    pub logical_bytes: u64,
    pub stats: IndexReadStats,
}

/// Explicit input and output bounds for one authenticated base/delta merge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexRunMergeLimits {
    base: IndexRunReadLimits,
    maximum_deltas: u64,
    maximum_delta_logical_bytes: u64,
    maximum_output_entries: u64,
    maximum_output_logical_bytes: u64,
}

impl IndexRunMergeLimits {
    pub const fn new(
        base: IndexRunReadLimits,
        maximum_deltas: u64,
        maximum_delta_logical_bytes: u64,
        maximum_output_entries: u64,
        maximum_output_logical_bytes: u64,
    ) -> Result<Self, StorageError> {
        if maximum_deltas == 0
            || maximum_deltas > MAX_INDEX_ENTRIES_PER_RUN
            || maximum_delta_logical_bytes == 0
            || maximum_delta_logical_bytes > MAX_INDEX_DELTA_LOGICAL_BYTES
            || maximum_output_entries == 0
            || maximum_output_entries > MAX_INDEX_ENTRIES_PER_RUN
            || maximum_output_logical_bytes == 0
            || maximum_output_logical_bytes > MAX_INDEX_RUN_LOGICAL_BYTES
        {
            return Err(StorageError::ResourceLimit);
        }
        Ok(Self {
            base,
            maximum_deltas,
            maximum_delta_logical_bytes,
            maximum_output_entries,
            maximum_output_logical_bytes,
        })
    }

    #[must_use]
    pub const fn base(self) -> IndexRunReadLimits {
        self.base
    }

    #[must_use]
    pub const fn maximum_deltas(self) -> u64 {
        self.maximum_deltas
    }

    #[must_use]
    pub const fn maximum_delta_logical_bytes(self) -> u64 {
        self.maximum_delta_logical_bytes
    }

    #[must_use]
    pub const fn maximum_output_entries(self) -> u64 {
        self.maximum_output_entries
    }

    #[must_use]
    pub const fn maximum_output_logical_bytes(self) -> u64 {
        self.maximum_output_logical_bytes
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IndexRunMergeReport {
    pub base: IndexRunReadReport,
    pub deltas: u64,
    pub delta_logical_bytes: u64,
    pub insertions: u64,
    pub replacements: u64,
    pub deletions: u64,
    pub output_entries: u64,
    pub output_logical_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergedIndexRun {
    pub run: Option<IndexRunDescriptor>,
    pub report: IndexRunMergeReport,
}

pub type IndexRunVisitor<'a> = dyn FnMut(&[u8], &[u8]) -> Result<(), StorageError> + 'a;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IndexScrubReport {
    pub runs: u64,
    pub pages: u64,
    pub entries: u64,
    pub cache_hits: u64,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CacheKey {
    database: DatabaseId,
    namespace: NamespaceId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    revision: CommitRevision,
    index_profile: [u8; 32],
    generation: u64,
    object_id: [u8; 16],
    page: u64,
}

struct CachedPage {
    bytes: Zeroizing<Box<[u8]>>,
    last_used: u64,
}

/// Fixed logical-byte-budget decrypted page cache with logarithmic-time LRU maintenance.
/// Eviction zeroizes page buffers on normal drop. Accounting includes explicit map allowances,
/// not allocator or process RSS measurements; the configured process limit remains independent.
pub struct PageCache {
    budget: usize,
    accounted_bytes: usize,
    clock: u64,
    hits: u64,
    misses: u64,
    evictions: u64,
    pages: BTreeMap<CacheKey, CachedPage>,
    recency: BTreeMap<u64, CacheKey>,
    read_telemetry: IndexReadTelemetry,
    read_telemetry_overflowed: bool,
}

impl core::fmt::Debug for PageCache {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("PageCache")
            .field("budget", &self.budget)
            .field("accounted_bytes", &self.accounted_bytes)
            .field("hits", &self.hits)
            .field("misses", &self.misses)
            .field("evictions", &self.evictions)
            .field("page_count", &self.pages.len())
            .finish()
    }
}

impl PageCache {
    pub fn new(budget: usize) -> Result<Self, StorageError> {
        if !(MIN_INDEX_CACHE_BYTES..=MAX_INDEX_CACHE_BYTES).contains(&budget) {
            return Err(StorageError::ResourceLimit);
        }
        Ok(Self {
            budget,
            accounted_bytes: 0,
            clock: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
            pages: BTreeMap::new(),
            recency: BTreeMap::new(),
            read_telemetry: IndexReadTelemetry::default(),
            read_telemetry_overflowed: false,
        })
    }

    #[must_use]
    pub const fn budget(&self) -> usize {
        self.budget
    }

    #[must_use]
    pub const fn accounted_bytes(&self) -> usize {
        self.accounted_bytes
    }

    #[must_use]
    pub const fn hits(&self) -> u64 {
        self.hits
    }

    #[must_use]
    pub const fn misses(&self) -> u64 {
        self.misses
    }

    #[must_use]
    pub const fn evictions(&self) -> u64 {
        self.evictions
    }

    /// Privileged diagnostics; overflow invalidates measurement, never the storage operation.
    pub fn read_telemetry(&self) -> Result<IndexReadTelemetry, StorageError> {
        if self.read_telemetry_overflowed {
            return Err(StorageError::ResourceLimit);
        }
        Ok(self.read_telemetry.clone())
    }

    fn observe_read<T>(
        &mut self,
        read: impl FnOnce(&mut Self, &mut IndexReadStats) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        let mut stats = IndexReadStats::default();
        let result = read(self, &mut stats);
        if self.read_telemetry.record(&stats, result.is_ok()).is_none() {
            self.read_telemetry_overflowed = true;
        }
        result
    }

    pub fn clear(&mut self) {
        self.pages = BTreeMap::new();
        self.recency = BTreeMap::new();
        self.accounted_bytes = 0;
    }

    fn advance_clock(&mut self) {
        self.clock = self.clock.checked_add(1).unwrap_or_else(|| {
            let removed = u64::try_from(self.pages.len()).unwrap_or(u64::MAX);
            self.evictions = self.evictions.saturating_add(removed);
            self.clear();
            1
        });
    }

    fn touch(&mut self, key: CacheKey) -> Option<&[u8]> {
        self.advance_clock();
        match self.pages.get_mut(&key) {
            Some(page) => {
                self.hits = self.hits.saturating_add(1);
                let removed = self.recency.remove(&page.last_used);
                debug_assert!(removed == Some(key));
                let replaced = self.recency.insert(self.clock, key);
                debug_assert!(replaced.is_none());
                page.last_used = self.clock;
                Some(page.bytes.as_ref())
            }
            None => {
                self.misses = self.misses.saturating_add(1);
                None
            }
        }
    }

    fn insert(&mut self, key: CacheKey, bytes: Vec<u8>) -> Result<(), StorageError> {
        let bytes = Zeroizing::new(bytes.into_boxed_slice());
        if bytes.len() != INDEX_PAGE_BYTES || self.pages.contains_key(&key) {
            return Err(StorageError::IntegrityFailure);
        }
        self.advance_clock();
        let capacity = (self.budget - CACHE_FIXED_OVERHEAD) / CACHE_ENTRY_BYTES;
        if self.pages.len() == capacity {
            let (_, oldest) = self
                .recency
                .pop_first()
                .ok_or(StorageError::IntegrityFailure)?;
            self.pages
                .remove(&oldest)
                .ok_or(StorageError::IntegrityFailure)?;
            self.evictions = self.evictions.saturating_add(1);
        }
        self.pages.insert(
            key,
            CachedPage {
                bytes,
                last_used: self.clock,
            },
        );
        self.recency.insert(self.clock, key);
        self.accounted_bytes = CACHE_FIXED_OVERHEAD + self.pages.len() * CACHE_ENTRY_BYTES;
        debug_assert!(self.accounted_bytes <= self.budget);
        Ok(())
    }
}

impl Default for PageCache {
    fn default() -> Self {
        Self::new(DEFAULT_INDEX_CACHE_BYTES).expect("default cache budget is valid")
    }
}

pub(crate) struct IndexContext<'a, D> {
    pub database: DatabaseId,
    pub epoch: KeyEpoch,
    pub writer: WriterIncarnationId,
    pub directory: &'a D,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_run<F, W, E, I, T>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &mut KeyVault<W, E>,
    identity_entropy: &mut I,
    scope: NamespaceRef,
    revision: CommitRevision,
    index_profile: [u8; 32],
    family: u8,
    entries: T,
) -> Result<IndexRunDescriptor, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
    T: IntoIterator<Item = Result<IndexEntry, StorageError>>,
{
    if scope.database() != context.database || family == 0 {
        return Err(StorageError::InvalidState);
    }
    let object_id = random_nonzero_id(identity_entropy)?;
    let name = run_name(object_id)?;
    let file = filesystem.create_new(context.directory, &name)?;
    let mut writer = RunWriter::new(
        file,
        context,
        scope,
        revision,
        index_profile,
        family,
        object_id,
    );
    for entry in entries {
        writer.push(filesystem, vault, entry?)?;
    }
    let descriptor = writer.finish(filesystem, vault)?;
    filesystem.sync_directory(context.directory)?;
    Ok(descriptor)
}

/// Publish a root and return the exact authenticated handle assembled into its durable manifest.
///
/// This avoids a fallible post-publication rediscovery read for callers that already performed
/// terminal semantic validation before publication.
#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_root<F, W, E, I>(
    filesystem: &mut F,
    context: IndexContext<'_, F::Directory>,
    vault: &mut KeyVault<W, E>,
    identity_entropy: &mut I,
    input: IndexRootInput,
    runs: &[IndexRunDescriptor],
    fallback_limits: Option<IndexRunReadLimits>,
) -> Result<RecoveredIndexRoot, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if input.scope.database() != context.database {
        return Err(StorageError::InvalidState);
    }
    validate_run_bindings(runs, input, &context)?;
    let candidates = match fallback_limits {
        Some(limits) => load_candidates_bounded(
            filesystem,
            &context,
            vault,
            input.scope,
            input.index_profile,
            limits,
        )?,
        None => load_candidates(
            filesystem,
            &context,
            vault,
            input.scope,
            input.index_profile,
        )?,
    };
    let generation = candidates
        .iter()
        .map(|(_, root)| root.generation)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(StorageError::ResourceLimit)?;
    let slot = match candidates.as_slice() {
        [] => Slot::A,
        [(used, _)] => used.other(),
        [(newest, _), (older, _)] => {
            let _ = newest;
            *older
        }
        _ => return Err(StorageError::IntegrityFailure),
    };
    let name = root_name(vault, &context, input.scope, input.index_profile, slot)?;
    remove_if_present(filesystem, context.directory, &name)?;
    filesystem.sync_directory(context.directory)?;

    let object_id = random_nonzero_id(identity_entropy)?;
    let manifest = RootManifest {
        input,
        generation,
        object_id,
        runs: runs.to_vec(),
    };
    let plaintext = manifest.encode()?;
    let encoded = vault
        .encrypt(
            index_context(
                &context,
                input.scope.namespace(),
                object_id,
                0,
                FrameClass::Small4KiB,
                ObjectRole::IndexPage,
            ),
            &plaintext,
        )?
        .encode()?;
    if encoded.len() != SMALL_ENVELOPE_BYTES {
        return Err(StorageError::IntegrityFailure);
    }
    let file = filesystem.create_new(context.directory, &name)?;
    write_all_at(filesystem, &file, 0, &object_id)?;
    write_all_at(filesystem, &file, 16, &encoded)?;
    filesystem.set_len(&file, ROOT_FILE_BYTES)?;
    filesystem.sync_all(&file)?;
    filesystem.sync_directory(context.directory)?;
    Ok(RecoveredIndexRoot {
        scope: input.scope,
        revision: input.revision,
        generation,
        certificate_digest: input.certificate_digest,
        reducer_profile: input.reducer_profile,
        logical_state_digest: input.logical_state_digest,
        index_profile: input.index_profile,
        runs: runs.to_vec(),
    })
}

pub(crate) fn load<F, W, E>(
    filesystem: &mut F,
    context: IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    scope: NamespaceRef,
    index_profile: [u8; 32],
) -> Result<Vec<RecoveredIndexRoot>, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    Ok(
        load_candidates(filesystem, &context, vault, scope, index_profile)?
            .into_iter()
            .map(|(_, root)| root)
            .collect(),
    )
}

/// Load only authenticated fixed-size root manifests and run-file shape metadata.
///
/// Referenced run pages have not been authenticated by this operation. Callers must keep every
/// returned root provisional and use complete run cursors with explicit limits before publishing
/// any derived result. This is the bounded discovery half of recovery; [`load`] remains the
/// fully-scrubbed compatibility path.
pub(crate) fn load_manifests<F, W, E>(
    filesystem: &mut F,
    context: IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    scope: NamespaceRef,
    index_profile: [u8; 32],
) -> Result<Vec<RecoveredIndexRoot>, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    let candidates = load_manifest_candidates(filesystem, &context, vault, scope, index_profile)?;
    validate_candidate_generations(&candidates)?;
    Ok(candidates.into_iter().map(|(_, root)| root).collect())
}

/// Finish durability of an exact discovered root without rotating either root slot. A previous
/// failed publication may have left authenticated but unsynced bytes visible in this process.
pub(crate) fn resync_root<F, W, E>(
    filesystem: &mut F,
    context: IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    root: &RecoveredIndexRoot,
    limits: IndexRunReadLimits,
) -> Result<(), StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    let candidates =
        load_manifest_candidates(filesystem, &context, vault, root.scope, root.index_profile)?;
    validate_candidate_generations(&candidates)?;
    let (slot, _) = candidates
        .into_iter()
        .find(|(_, candidate)| candidate == root)
        .ok_or(StorageError::IntegrityFailure)?;
    for run in root.runs() {
        visit_run(
            filesystem,
            &context,
            vault,
            root,
            run.family,
            limits,
            &mut |_, _| Ok(()),
        )?;
        let file = filesystem.open_existing(context.directory, &run_name(run.object_id)?)?;
        filesystem.sync_all(&file)?;
    }
    let name = root_name(vault, &context, root.scope, root.index_profile, slot)?;
    let file = filesystem.open_existing(context.directory, &name)?;
    filesystem.sync_all(&file)?;
    filesystem.sync_directory(context.directory)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn get<F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    root: &RecoveredIndexRoot,
    family: u8,
    key: &[u8],
    cache: &mut PageCache,
) -> Result<(Option<Vec<u8>>, IndexReadStats), StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    get_bounded(
        filesystem,
        context,
        vault,
        root,
        family,
        key,
        IndexGetLimits {
            maximum_page_visits: MAX_INDEX_GET_PAGE_VISITS,
            maximum_result_bytes: MAX_INDEX_VALUE_BYTES,
        },
        cache,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn get_bounded<F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    root: &RecoveredIndexRoot,
    family: u8,
    key: &[u8],
    limits: IndexGetLimits,
    cache: &mut PageCache,
) -> Result<(Option<Vec<u8>>, IndexReadStats), StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    cache.observe_read(|cache, stats| {
        validate_read(root, context, key)?;
        let run = root.run(family)?;
        let mut page_visits = 0_u64;
        let start = lower_bound_page_bounded(
            filesystem,
            context,
            vault,
            root,
            run,
            key,
            limits,
            &mut page_visits,
            cache,
            stats,
        )?;
        let Some(mut page_index) = start else {
            return Ok((None, stats.clone()));
        };
        let mut value = Vec::new();
        let mut expected_len = None;
        while page_index < run.page_count {
            let page = load_get_page(
                filesystem,
                context,
                vault,
                root,
                run,
                page_index,
                limits,
                &mut page_visits,
                cache,
                stats,
            )?;
            let parsed = ParsedPage::new(page, root, run, page_index)?;
            for fragment in parsed.fragments() {
                let fragment = fragment?;
                stats.fragments_visited = stats.fragments_visited.saturating_add(1);
                match fragment.key.cmp(key) {
                    core::cmp::Ordering::Less => continue,
                    core::cmp::Ordering::Greater if expected_len.is_some() => {
                        return Err(StorageError::IntegrityFailure);
                    }
                    core::cmp::Ordering::Greater => return Ok((None, stats.clone())),
                    core::cmp::Ordering::Equal => {}
                }
                if expected_len.is_none() {
                    if fragment.offset != 0 {
                        return Err(StorageError::IntegrityFailure);
                    }
                    if fragment.total_len > limits.maximum_result_bytes {
                        return Err(StorageError::ResourceLimit);
                    }
                    expected_len = Some(fragment.total_len);
                    value
                        .try_reserve_exact(fragment.total_len)
                        .map_err(|_| StorageError::ResourceLimit)?;
                }
                if expected_len != Some(fragment.total_len) || fragment.offset != value.len() {
                    return Err(StorageError::IntegrityFailure);
                }
                value.extend_from_slice(fragment.value);
                if value.len() == fragment.total_len {
                    stats.result_bytes =
                        u64::try_from(value.len()).map_err(|_| StorageError::ResourceLimit)?;
                    return Ok((Some(value), stats.clone()));
                }
            }
            page_index = page_index
                .checked_add(1)
                .ok_or(StorageError::ResourceLimit)?;
        }
        if expected_len.is_some() {
            Err(StorageError::IntegrityFailure)
        } else {
            Ok((None, stats.clone()))
        }
    })
}

/// Return the greatest complete key with `prefix` that is at or before `upper_bound`.
///
/// This is an authenticated point proof, not a capped prefix scan. It retains at most one value
/// and visits only the binary-search path plus the adjacent/fragment-spanning pages required to
/// prove the predecessor. Page visits and returned bytes are explicitly bounded.
#[allow(clippy::too_many_arguments)]
pub(crate) fn get_predecessor<F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    root: &RecoveredIndexRoot,
    family: u8,
    prefix: &[u8],
    upper_bound: &[u8],
    limits: IndexPredecessorLimits,
    cache: &mut PageCache,
) -> Result<IndexPredecessor, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    cache.observe_read(|cache, stats| {
        validate_read(root, context, prefix)?;
        validate_read(root, context, upper_bound)?;
        if !upper_bound.starts_with(prefix) {
            return Err(StorageError::InvalidState);
        }
        let run = root.run(family)?;
        let mut page_visits = 0_u64;

        let mut low = 0_u64;
        let mut high = run.page_count;
        while low < high {
            let middle = low + (high - low) / 2;
            let page = load_predecessor_page(
                filesystem,
                context,
                vault,
                root,
                run,
                middle,
                limits,
                &mut page_visits,
                cache,
                stats,
            )?;
            let parsed = ParsedPage::new(page, root, run, middle)?;
            if parsed.last_key()? < upper_bound {
                low = middle.checked_add(1).ok_or(StorageError::ResourceLimit)?;
            } else {
                high = middle;
            }
        }
        let probe = if low < run.page_count {
            low
        } else {
            run.page_count
                .checked_sub(1)
                .ok_or(StorageError::IntegrityFailure)?
        };
        let mut start = probe.saturating_sub(1);
        loop {
            let page = load_predecessor_page(
                filesystem,
                context,
                vault,
                root,
                run,
                start,
                limits,
                &mut page_visits,
                cache,
                stats,
            )?;
            let parsed = ParsedPage::new(page, root, run, start)?;
            let first = parsed
                .fragments()
                .next()
                .ok_or(StorageError::IntegrityFailure)??;
            if first.offset == 0 || start == 0 {
                break;
            }
            start = start.checked_sub(1).ok_or(StorageError::IntegrityFailure)?;
        }

        // First pass proves the terminal matching key without retaining any value. This prevents an
        // earlier large match from consuming the result budget or coexisting with the final value.
        let mut candidate_key = None;
        let mut scan_key = Vec::new();
        let mut scan_total = None;
        let mut scan_offset = 0_usize;
        let mut page_index = start;
        let mut finished = false;
        while page_index < run.page_count && !finished {
            let page = load_predecessor_page(
                filesystem,
                context,
                vault,
                root,
                run,
                page_index,
                limits,
                &mut page_visits,
                cache,
                stats,
            )?;
            let parsed = ParsedPage::new(page, root, run, page_index)?;
            for fragment in parsed.fragments() {
                let fragment = fragment?;
                stats.fragments_visited = stats.fragments_visited.saturating_add(1);
                if scan_total.is_some() && scan_key.as_slice() != fragment.key {
                    return Err(StorageError::IntegrityFailure);
                }
                if fragment.key > upper_bound {
                    if scan_total.is_some() {
                        return Err(StorageError::IntegrityFailure);
                    }
                    finished = true;
                    break;
                }
                if !fragment.key.starts_with(prefix) {
                    continue;
                }
                if scan_total.is_none() {
                    if fragment.offset != 0 {
                        return Err(StorageError::IntegrityFailure);
                    }
                    scan_key.extend_from_slice(fragment.key);
                    scan_total = Some(fragment.total_len);
                    scan_offset = 0;
                }
                if scan_key.as_slice() != fragment.key
                    || scan_total != Some(fragment.total_len)
                    || scan_offset != fragment.offset
                {
                    return Err(StorageError::IntegrityFailure);
                }
                scan_offset = scan_offset
                    .checked_add(fragment.value.len())
                    .filter(|offset| *offset <= fragment.total_len)
                    .ok_or(StorageError::IntegrityFailure)?;
                if scan_offset == fragment.total_len {
                    candidate_key = Some(core::mem::take(&mut scan_key));
                    scan_total = None;
                    scan_offset = 0;
                }
            }
            page_index = page_index
                .checked_add(1)
                .ok_or(StorageError::ResourceLimit)?;
        }
        if scan_total.is_some() {
            return Err(StorageError::IntegrityFailure);
        }

        let Some(mut candidate_key) = candidate_key else {
            return Ok(IndexPredecessor {
                entry: None,
                stats: stats.clone(),
            });
        };
        let mut current_value = Vec::new();
        let mut expected_total = None;
        let mut selected = None;
        page_index = start;
        while page_index < run.page_count && selected.is_none() {
            let page = load_predecessor_page(
                filesystem,
                context,
                vault,
                root,
                run,
                page_index,
                limits,
                &mut page_visits,
                cache,
                stats,
            )?;
            let parsed = ParsedPage::new(page, root, run, page_index)?;
            for fragment in parsed.fragments() {
                let fragment = fragment?;
                stats.fragments_visited = stats.fragments_visited.saturating_add(1);
                match fragment.key.cmp(candidate_key.as_slice()) {
                    core::cmp::Ordering::Less => continue,
                    core::cmp::Ordering::Greater => {
                        return Err(StorageError::IntegrityFailure);
                    }
                    core::cmp::Ordering::Equal => {}
                }
                if expected_total.is_none() {
                    if fragment.offset != 0 {
                        return Err(StorageError::IntegrityFailure);
                    }
                    let result_bytes = candidate_key
                        .len()
                        .checked_add(fragment.total_len)
                        .ok_or(StorageError::ResourceLimit)?;
                    if result_bytes > limits.maximum_result_bytes {
                        return Err(StorageError::ResourceLimit);
                    }
                    expected_total = Some(fragment.total_len);
                    current_value
                        .try_reserve_exact(fragment.total_len)
                        .map_err(|_| StorageError::ResourceLimit)?;
                }
                if expected_total != Some(fragment.total_len)
                    || current_value.len() != fragment.offset
                {
                    return Err(StorageError::IntegrityFailure);
                }
                current_value.extend_from_slice(fragment.value);
                if current_value.len() == fragment.total_len {
                    stats.result_bytes = u64::try_from(candidate_key.len() + current_value.len())
                        .map_err(|_| StorageError::ResourceLimit)?;
                    selected = Some(IndexScanEntry {
                        key: core::mem::take(&mut candidate_key),
                        value: core::mem::take(&mut current_value),
                    });
                    expected_total = None;
                    break;
                }
            }
            page_index = page_index
                .checked_add(1)
                .ok_or(StorageError::ResourceLimit)?;
        }
        if selected.is_none() || expected_total.is_some() {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(IndexPredecessor {
            entry: selected,
            stats: stats.clone(),
        })
    })
}

pub(crate) fn scrub<F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    root: &RecoveredIndexRoot,
    cache: &mut PageCache,
) -> Result<IndexScrubReport, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    if root.scope.database() != context.database {
        return Err(StorageError::InvalidState);
    }
    // A scrub attests current durable bytes, not previously authenticated plaintext. Clearing the
    // caller's cache also prevents a removed or damaged run from passing on a stale cache hit.
    cache.clear();
    let mut report = IndexScrubReport::default();
    for run in &root.runs {
        let file = filesystem
            .open_existing(context.directory, &run_name(run.object_id)?)
            .map_err(|error| {
                if error.kind() == AdapterErrorKind::NotFound {
                    StorageError::IntegrityFailure
                } else {
                    error.into()
                }
            })?;
        let expected_len = run
            .page_count
            .checked_mul(ENCODED_PAGE_BYTES)
            .ok_or(StorageError::ResourceLimit)?;
        if filesystem.metadata(&file)?.len != expected_len {
            return Err(StorageError::IntegrityFailure);
        }
        let mut digest = Sha256::new();
        digest.update(b"USTE-INDEX-RUN-V1\0");
        digest.update(root.scope.namespace().as_bytes());
        digest.update(root.revision.get().to_be_bytes());
        digest.update(root.index_profile);
        digest.update([run.family]);
        let mut stats = IndexReadStats::default();
        let mut previous_key: Option<Vec<u8>> = None;
        let mut current_key = Vec::new();
        let mut current_value = Vec::new();
        let mut current_total = None;
        let mut entries = 0_u64;
        for page_index in 0..run.page_count {
            let page = load_page(
                filesystem, context, vault, root, run, page_index, cache, &mut stats,
            )?;
            let parsed = ParsedPage::new(page, root, run, page_index)?;
            for fragment in parsed.fragments() {
                let fragment = fragment?;
                if current_total.is_none() {
                    if fragment.offset != 0
                        || previous_key
                            .as_ref()
                            .is_some_and(|previous| previous.as_slice() >= fragment.key)
                    {
                        return Err(StorageError::IntegrityFailure);
                    }
                    current_key.extend_from_slice(fragment.key);
                    current_total = Some(fragment.total_len);
                    current_value
                        .try_reserve_exact(fragment.total_len)
                        .map_err(|_| StorageError::ResourceLimit)?;
                }
                if current_key.as_slice() != fragment.key
                    || current_total != Some(fragment.total_len)
                    || current_value.len() != fragment.offset
                {
                    return Err(StorageError::IntegrityFailure);
                }
                current_value.extend_from_slice(fragment.value);
                if current_value.len() == fragment.total_len {
                    digest.update(
                        u32::try_from(current_key.len())
                            .map_err(|_| StorageError::ResourceLimit)?
                            .to_be_bytes(),
                    );
                    digest.update(
                        u64::try_from(current_value.len())
                            .map_err(|_| StorageError::ResourceLimit)?
                            .to_be_bytes(),
                    );
                    digest.update(&current_key);
                    digest.update(&current_value);
                    previous_key = Some(core::mem::take(&mut current_key));
                    current_value.clear();
                    current_total = None;
                    entries = entries.checked_add(1).ok_or(StorageError::ResourceLimit)?;
                }
            }
        }
        if current_total.is_some()
            || entries != run.entry_count
            || <[u8; 32]>::from(digest.finalize()) != run.logical_digest
        {
            return Err(StorageError::IntegrityFailure);
        }
        report.runs = report.runs.saturating_add(1);
        report.pages = report.pages.saturating_add(run.page_count);
        report.entries = report.entries.saturating_add(entries);
        report.cache_hits = report.cache_hits.saturating_add(stats.cache_hits);
    }
    Ok(report)
}

/// Single-handle authenticated cursor used by both full-run visitors and bounded scratch merges.
/// Entries returned before `report` succeeds remain provisional.
/// Opaque authenticated cursor over one immutable index run.
///
/// Entries returned by `JournalStore` remain provisional until the cursor is exhausted and
/// explicitly finished. Dropping a cursor early produces no terminal authentication report.
pub struct IndexRunCursor<F>
where
    F: FileSystem,
{
    file: F::File,
    root: RecoveredIndexRoot,
    run: IndexRunDescriptor,
    limits: IndexRunReadLimits,
    expected_len: u64,
    page_index: u64,
    page: Option<SecretBytes>,
    page_used: usize,
    fragment_offset: usize,
    fragments_remaining: usize,
    previous_key: Option<Vec<u8>>,
    current_key: Vec<u8>,
    current_value: Vec<u8>,
    current_total: Option<usize>,
    entries: u64,
    logical_bytes: u64,
    stats: IndexReadStats,
    digest: Option<Sha256>,
    finished: bool,
}

impl<F> core::fmt::Debug for IndexRunCursor<F>
where
    F: FileSystem,
{
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("IndexRunCursor")
            .field("root", &"[REDACTED]")
            .field("family", &self.run.family)
            .field("entries_seen", &self.entries)
            .field("finished", &self.finished)
            .finish()
    }
}

impl<F> IndexRunCursor<F>
where
    F: FileSystem,
{
    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.root.scope
    }

    #[must_use]
    pub const fn revision(&self) -> CommitRevision {
        self.root.revision
    }

    fn open<W, E>(
        filesystem: &mut F,
        context: &IndexContext<'_, F::Directory>,
        _vault: &KeyVault<W, E>,
        root: &RecoveredIndexRoot,
        family: u8,
        limits: IndexRunReadLimits,
    ) -> Result<Self, StorageError>
    where
        W: DurableKeyEnvelope,
        E: EntropySource,
    {
        if root.scope.database() != context.database {
            return Err(StorageError::InvalidState);
        }
        let run = *root.run(family)?;
        if run.page_count > limits.maximum_pages || run.entry_count > limits.maximum_entries {
            return Err(StorageError::ResourceLimit);
        }
        let file = filesystem
            .open_existing(context.directory, &run_name(run.object_id)?)
            .map_err(|error| {
                if error.kind() == AdapterErrorKind::NotFound {
                    StorageError::IntegrityFailure
                } else {
                    error.into()
                }
            })?;
        let expected_len = run
            .page_count
            .checked_mul(ENCODED_PAGE_BYTES)
            .ok_or(StorageError::ResourceLimit)?;
        if filesystem.metadata(&file)?.len != expected_len {
            return Err(StorageError::IntegrityFailure);
        }
        let mut digest = Sha256::new();
        digest.update(b"USTE-INDEX-RUN-V1\0");
        digest.update(root.scope.namespace().as_bytes());
        digest.update(root.revision.get().to_be_bytes());
        digest.update(root.index_profile);
        digest.update([run.family]);
        Ok(Self {
            file,
            root: root.clone(),
            run,
            limits,
            expected_len,
            page_index: 0,
            page: None,
            page_used: 0,
            fragment_offset: 0,
            fragments_remaining: 0,
            previous_key: None,
            current_key: Vec::new(),
            current_value: Vec::new(),
            current_total: None,
            entries: 0,
            logical_bytes: 0,
            stats: IndexReadStats::default(),
            digest: Some(digest),
            finished: false,
        })
    }

    fn next<W, E>(
        &mut self,
        filesystem: &mut F,
        context: &IndexContext<'_, F::Directory>,
        vault: &KeyVault<W, E>,
    ) -> Result<Option<IndexEntry>, StorageError>
    where
        W: DurableKeyEnvelope,
        E: EntropySource,
    {
        loop {
            if self.page.is_none() && self.page_index == self.run.page_count {
                self.finish(filesystem)?;
                return Ok(None);
            }
            if self.page.is_none() {
                self.read_page(filesystem, context, vault)?;
            }
            let fragment = self.take_fragment()?;
            if self.accept_fragment(fragment)? {
                return Ok(Some(IndexEntry {
                    key: core::mem::take(&mut self.current_key),
                    value: core::mem::take(&mut self.current_value),
                }));
            }
        }
    }

    fn read_page<W, E>(
        &mut self,
        filesystem: &mut F,
        context: &IndexContext<'_, F::Directory>,
        vault: &KeyVault<W, E>,
    ) -> Result<(), StorageError>
    where
        W: DurableKeyEnvelope,
        E: EntropySource,
    {
        let page_index = self.page_index;
        let offset = page_index
            .checked_mul(ENCODED_PAGE_BYTES)
            .ok_or(StorageError::ResourceLimit)?;
        let mut encoded = vec![0_u8; ENCODED_PAGE_BYTES as usize];
        read_exact_at(filesystem, &self.file, offset, &mut encoded).map_err(|error| {
            if error.kind() == AdapterErrorKind::UnexpectedEof {
                StorageError::IntegrityFailure
            } else {
                error.into()
            }
        })?;
        let envelope = EncryptedEnvelope::decode(&encoded).map_err(index_crypto_error)?;
        let plaintext = vault
            .decrypt(
                index_context(
                    context,
                    self.root.scope.namespace(),
                    self.run.object_id,
                    page_index
                        .checked_add(1)
                        .ok_or(StorageError::ResourceLimit)?,
                    FrameClass::Small4KiB,
                    ObjectRole::IndexPage,
                ),
                &envelope,
            )
            .map_err(index_crypto_error)?;
        if plaintext.as_slice().len() != INDEX_PAGE_BYTES {
            return Err(StorageError::IntegrityFailure);
        }
        self.stats.pages_read = self.stats.pages_read.saturating_add(1);
        let parsed = ParsedPage::new(plaintext.as_slice(), &self.root, &self.run, page_index)?;
        self.page_used = parsed.used;
        self.fragment_offset = PAGE_HEADER_BYTES;
        self.fragments_remaining = parsed.fragment_count;
        self.page = Some(plaintext);
        Ok(())
    }

    fn take_fragment(&mut self) -> Result<OwnedFragment, StorageError> {
        let page = self.page.as_ref().ok_or(StorageError::InvalidState)?;
        let mut fragments = FragmentIter {
            remaining: page
                .as_slice()
                .get(self.fragment_offset..self.page_used)
                .ok_or(StorageError::IntegrityFailure)?,
            remaining_count: self.fragments_remaining,
        };
        let fragment = fragments.next().ok_or(StorageError::IntegrityFailure)??;
        let consumed = self
            .page_used
            .checked_sub(self.fragment_offset)
            .and_then(|remaining| remaining.checked_sub(fragments.remaining.len()))
            .ok_or(StorageError::IntegrityFailure)?;
        let owned = OwnedFragment {
            key: fragment.key.to_vec(),
            total_len: fragment.total_len,
            offset: fragment.offset,
            value: fragment.value.to_vec(),
        };
        self.fragment_offset = self
            .fragment_offset
            .checked_add(consumed)
            .ok_or(StorageError::ResourceLimit)?;
        self.fragments_remaining = self
            .fragments_remaining
            .checked_sub(1)
            .ok_or(StorageError::IntegrityFailure)?;
        if self.fragments_remaining == 0 {
            if self.fragment_offset != self.page_used || !fragments.remaining.is_empty() {
                return Err(StorageError::IntegrityFailure);
            }
            self.page = None;
            self.page_index = self
                .page_index
                .checked_add(1)
                .ok_or(StorageError::ResourceLimit)?;
        }
        Ok(owned)
    }

    fn accept_fragment(&mut self, fragment: OwnedFragment) -> Result<bool, StorageError> {
        self.stats.fragments_visited = self.stats.fragments_visited.saturating_add(1);
        if self.current_total.is_none() {
            if fragment.offset != 0
                || self
                    .previous_key
                    .as_ref()
                    .is_some_and(|previous| previous.as_slice() >= fragment.key.as_slice())
            {
                return Err(StorageError::IntegrityFailure);
            }
            let entry_bytes = u64::try_from(
                fragment
                    .key
                    .len()
                    .checked_add(fragment.total_len)
                    .ok_or(StorageError::ResourceLimit)?,
            )
            .map_err(|_| StorageError::ResourceLimit)?;
            let projected = self
                .logical_bytes
                .checked_add(entry_bytes)
                .ok_or(StorageError::ResourceLimit)?;
            if projected > self.limits.maximum_logical_bytes {
                return Err(StorageError::ResourceLimit);
            }
            self.current_key = fragment.key.clone();
            self.current_total = Some(fragment.total_len);
            self.current_value
                .try_reserve_exact(fragment.total_len)
                .map_err(|_| StorageError::ResourceLimit)?;
        }
        if self.current_key != fragment.key
            || self.current_total != Some(fragment.total_len)
            || self.current_value.len() != fragment.offset
        {
            return Err(StorageError::IntegrityFailure);
        }
        self.current_value.extend_from_slice(&fragment.value);
        if self.current_value.len() != fragment.total_len {
            return Ok(false);
        }
        self.entries = self
            .entries
            .checked_add(1)
            .ok_or(StorageError::ResourceLimit)?;
        if self.entries > self.run.entry_count {
            return Err(StorageError::IntegrityFailure);
        }
        if self.entries > self.limits.maximum_entries {
            return Err(StorageError::ResourceLimit);
        }
        let entry_bytes = u64::try_from(
            self.current_key
                .len()
                .checked_add(self.current_value.len())
                .ok_or(StorageError::ResourceLimit)?,
        )
        .map_err(|_| StorageError::ResourceLimit)?;
        self.logical_bytes = self
            .logical_bytes
            .checked_add(entry_bytes)
            .ok_or(StorageError::ResourceLimit)?;
        debug_assert!(self.logical_bytes <= self.limits.maximum_logical_bytes);
        let digest = self.digest.as_mut().ok_or(StorageError::InvalidState)?;
        digest.update(
            u32::try_from(self.current_key.len())
                .map_err(|_| StorageError::ResourceLimit)?
                .to_be_bytes(),
        );
        digest.update(
            u64::try_from(self.current_value.len())
                .map_err(|_| StorageError::ResourceLimit)?
                .to_be_bytes(),
        );
        digest.update(&self.current_key);
        digest.update(&self.current_value);
        self.previous_key = Some(self.current_key.clone());
        self.current_total = None;
        Ok(true)
    }

    fn finish(&mut self, filesystem: &mut F) -> Result<(), StorageError> {
        if self.finished {
            return Ok(());
        }
        let digest = self.digest.take().ok_or(StorageError::InvalidState)?;
        if self.current_total.is_some()
            || self.entries != self.run.entry_count
            || <[u8; 32]>::from(digest.finalize()) != self.run.logical_digest
            || filesystem.metadata(&self.file)?.len != self.expected_len
        {
            return Err(StorageError::IntegrityFailure);
        }
        self.stats.result_bytes = self.logical_bytes;
        self.finished = true;
        Ok(())
    }

    fn report(&self) -> Result<IndexRunReadReport, StorageError> {
        if !self.finished || self.page.is_some() {
            return Err(StorageError::InvalidState);
        }
        Ok(IndexRunReadReport {
            entries: self.entries,
            logical_bytes: self.logical_bytes,
            stats: self.stats.clone(),
        })
    }

    pub(crate) const fn root(&self) -> &RecoveredIndexRoot {
        &self.root
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn open_run_cursor<F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    root: &RecoveredIndexRoot,
    family: u8,
    limits: IndexRunReadLimits,
) -> Result<IndexRunCursor<F>, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    IndexRunCursor::open(filesystem, context, vault, root, family, limits)
}

pub(crate) fn next_run_entry<F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    cursor: &mut IndexRunCursor<F>,
) -> Result<Option<IndexEntry>, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    cursor.next(filesystem, context, vault)
}

pub(crate) fn finish_run_cursor<F>(
    cursor: IndexRunCursor<F>,
) -> Result<IndexRunReadReport, StorageError>
where
    F: FileSystem,
{
    cursor.report()
}

/// Visit one complete immutable run in canonical key order while re-authenticating every page
/// and recomputing the terminal logical digest.
///
/// The visitor observes entries before the terminal digest can be known. It must therefore stage
/// any externally visible result and publish it only after this function returns `Ok`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn visit_run<F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    root: &RecoveredIndexRoot,
    family: u8,
    limits: IndexRunReadLimits,
    visitor: &mut IndexRunVisitor<'_>,
) -> Result<IndexRunReadReport, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    let mut reader = open_run_cursor(filesystem, context, vault, root, family, limits)?;
    while let Some(entry) = reader.next(filesystem, context, vault)? {
        visitor(&entry.key, &entry.value)?;
    }
    reader.report()
}

/// Merge one authenticated optional base run with an exact ordered before/after delta stream into
/// one invisible encrypted run at `revision`. The caller may publish the returned descriptor only
/// after separately validating its domain semantics and exact journal anchor.
#[allow(clippy::too_many_arguments)]
pub(crate) fn merge_run<F, W, E, I, T>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &mut KeyVault<W, E>,
    identity_entropy: &mut I,
    scope: NamespaceRef,
    revision: CommitRevision,
    index_profile: [u8; 32],
    family: u8,
    base_root: Option<&RecoveredIndexRoot>,
    limits: IndexRunMergeLimits,
    deltas: T,
) -> Result<MergedIndexRun, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
    T: IntoIterator<Item = Result<IndexDelta, StorageError>>,
{
    merge_run_visit(
        filesystem,
        context,
        vault,
        identity_entropy,
        scope,
        revision,
        index_profile,
        family,
        base_root,
        limits,
        deltas,
        &mut |_, _| Ok(()),
    )
}

/// Merge while provisionally visiting every exact output entry before it is encrypted.
///
/// Visitor effects must remain private until this function returns successfully: a later source
/// authentication, output write, or sync failure can still reject the provisional output.
#[allow(clippy::too_many_arguments)]
pub(crate) fn merge_run_visit<F, W, E, I, T>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &mut KeyVault<W, E>,
    identity_entropy: &mut I,
    scope: NamespaceRef,
    revision: CommitRevision,
    index_profile: [u8; 32],
    family: u8,
    base_root: Option<&RecoveredIndexRoot>,
    limits: IndexRunMergeLimits,
    deltas: T,
    visitor: &mut IndexRunVisitor<'_>,
) -> Result<MergedIndexRun, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
    T: IntoIterator<Item = Result<IndexDelta, StorageError>>,
{
    if scope.database() != context.database || family == 0 {
        return Err(StorageError::InvalidState);
    }
    if let Some(root) = base_root
        && (root.scope != scope || root.index_profile != index_profile)
    {
        return Err(StorageError::InvalidState);
    }

    let mut base_reader = match base_root {
        Some(root) => Some(IndexRunCursor::open(
            filesystem,
            context,
            vault,
            root,
            family,
            limits.base,
        )?),
        None => None,
    };
    let mut base = match base_reader.as_mut() {
        Some(reader) => reader.next(filesystem, context, vault)?,
        None => None,
    };
    let mut delta_iter = deltas.into_iter();
    let mut previous_delta_key = None;
    let mut report = IndexRunMergeReport::default();
    let mut delta = next_delta(
        &mut delta_iter,
        &mut previous_delta_key,
        &mut report,
        limits,
    )?;
    let mut writer = None;

    while base.is_some() || delta.is_some() {
        match (base.as_ref(), delta.as_ref()) {
            (Some(base_entry), Some(change)) => match base_entry.key.cmp(&change.key) {
                core::cmp::Ordering::Less => {
                    emit_merged_entry(
                        filesystem,
                        context,
                        vault,
                        identity_entropy,
                        scope,
                        revision,
                        index_profile,
                        family,
                        limits,
                        &mut report,
                        &mut writer,
                        visitor,
                        base.take().ok_or(StorageError::InvalidState)?,
                    )?;
                    base = base_reader
                        .as_mut()
                        .ok_or(StorageError::InvalidState)?
                        .next(filesystem, context, vault)?;
                }
                core::cmp::Ordering::Equal => {
                    let base_entry = base.take().ok_or(StorageError::InvalidState)?;
                    let change = delta.take().ok_or(StorageError::InvalidState)?;
                    if change.before.as_deref() != Some(base_entry.value.as_slice()) {
                        return Err(StorageError::InvalidState);
                    }
                    match change.after {
                        Some(value) => {
                            report.replacements = report.replacements.saturating_add(1);
                            emit_merged_entry(
                                filesystem,
                                context,
                                vault,
                                identity_entropy,
                                scope,
                                revision,
                                index_profile,
                                family,
                                limits,
                                &mut report,
                                &mut writer,
                                visitor,
                                IndexEntry {
                                    key: change.key,
                                    value,
                                },
                            )?;
                        }
                        None => report.deletions = report.deletions.saturating_add(1),
                    }
                    base = base_reader
                        .as_mut()
                        .ok_or(StorageError::InvalidState)?
                        .next(filesystem, context, vault)?;
                    delta = next_delta(
                        &mut delta_iter,
                        &mut previous_delta_key,
                        &mut report,
                        limits,
                    )?;
                }
                core::cmp::Ordering::Greater => {
                    let change = delta.take().ok_or(StorageError::InvalidState)?;
                    apply_absent_delta(
                        filesystem,
                        context,
                        vault,
                        identity_entropy,
                        scope,
                        revision,
                        index_profile,
                        family,
                        limits,
                        &mut report,
                        &mut writer,
                        visitor,
                        change,
                    )?;
                    delta = next_delta(
                        &mut delta_iter,
                        &mut previous_delta_key,
                        &mut report,
                        limits,
                    )?;
                }
            },
            (Some(_), None) => {
                emit_merged_entry(
                    filesystem,
                    context,
                    vault,
                    identity_entropy,
                    scope,
                    revision,
                    index_profile,
                    family,
                    limits,
                    &mut report,
                    &mut writer,
                    visitor,
                    base.take().ok_or(StorageError::InvalidState)?,
                )?;
                base = base_reader
                    .as_mut()
                    .ok_or(StorageError::InvalidState)?
                    .next(filesystem, context, vault)?;
            }
            (None, Some(_)) => {
                let change = delta.take().ok_or(StorageError::InvalidState)?;
                apply_absent_delta(
                    filesystem,
                    context,
                    vault,
                    identity_entropy,
                    scope,
                    revision,
                    index_profile,
                    family,
                    limits,
                    &mut report,
                    &mut writer,
                    visitor,
                    change,
                )?;
                delta = next_delta(
                    &mut delta_iter,
                    &mut previous_delta_key,
                    &mut report,
                    limits,
                )?;
            }
            (None, None) => break,
        }
    }

    if let Some(reader) = base_reader.as_ref() {
        report.base = reader.report()?;
    }
    let run = match writer {
        Some(writer) => {
            let descriptor = writer.finish(filesystem, vault)?;
            filesystem.sync_directory(context.directory)?;
            Some(descriptor)
        }
        None => None,
    };
    Ok(MergedIndexRun { run, report })
}

fn next_delta<T>(
    deltas: &mut T,
    previous_key: &mut Option<Vec<u8>>,
    report: &mut IndexRunMergeReport,
    limits: IndexRunMergeLimits,
) -> Result<Option<IndexDelta>, StorageError>
where
    T: Iterator<Item = Result<IndexDelta, StorageError>>,
{
    let Some(delta) = deltas.next() else {
        return Ok(None);
    };
    let delta = delta?;
    if previous_key
        .as_ref()
        .is_some_and(|previous| previous.as_slice() >= delta.key.as_slice())
    {
        return Err(StorageError::InvalidState);
    }
    let logical_bytes = delta.logical_bytes()?;
    report.deltas = report
        .deltas
        .checked_add(1)
        .filter(|count| *count <= limits.maximum_deltas)
        .ok_or(StorageError::ResourceLimit)?;
    report.delta_logical_bytes = report
        .delta_logical_bytes
        .checked_add(logical_bytes)
        .filter(|bytes| *bytes <= limits.maximum_delta_logical_bytes)
        .ok_or(StorageError::ResourceLimit)?;
    *previous_key = Some(delta.key.clone());
    Ok(Some(delta))
}

#[allow(clippy::too_many_arguments)]
fn apply_absent_delta<F, W, E, I>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &mut KeyVault<W, E>,
    identity_entropy: &mut I,
    scope: NamespaceRef,
    revision: CommitRevision,
    index_profile: [u8; 32],
    family: u8,
    limits: IndexRunMergeLimits,
    report: &mut IndexRunMergeReport,
    writer: &mut Option<RunWriter<F>>,
    visitor: &mut IndexRunVisitor<'_>,
    delta: IndexDelta,
) -> Result<(), StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if delta.before.is_some() {
        return Err(StorageError::InvalidState);
    }
    let value = delta.after.ok_or(StorageError::InvalidState)?;
    report.insertions = report.insertions.saturating_add(1);
    emit_merged_entry(
        filesystem,
        context,
        vault,
        identity_entropy,
        scope,
        revision,
        index_profile,
        family,
        limits,
        report,
        writer,
        visitor,
        IndexEntry {
            key: delta.key,
            value,
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn emit_merged_entry<F, W, E, I>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &mut KeyVault<W, E>,
    identity_entropy: &mut I,
    scope: NamespaceRef,
    revision: CommitRevision,
    index_profile: [u8; 32],
    family: u8,
    limits: IndexRunMergeLimits,
    report: &mut IndexRunMergeReport,
    writer: &mut Option<RunWriter<F>>,
    visitor: &mut IndexRunVisitor<'_>,
    entry: IndexEntry,
) -> Result<(), StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let logical_bytes = u64::try_from(
        entry
            .key
            .len()
            .checked_add(entry.value.len())
            .ok_or(StorageError::ResourceLimit)?,
    )
    .map_err(|_| StorageError::ResourceLimit)?;
    report.output_entries = report
        .output_entries
        .checked_add(1)
        .filter(|count| *count <= limits.maximum_output_entries)
        .ok_or(StorageError::ResourceLimit)?;
    report.output_logical_bytes = report
        .output_logical_bytes
        .checked_add(logical_bytes)
        .filter(|bytes| *bytes <= limits.maximum_output_logical_bytes)
        .ok_or(StorageError::ResourceLimit)?;
    visitor(&entry.key, &entry.value)?;
    if writer.is_none() {
        let object_id = random_nonzero_id(identity_entropy)?;
        let file = filesystem.create_new(context.directory, &run_name(object_id)?)?;
        *writer = Some(RunWriter::new(
            file,
            context,
            scope,
            revision,
            index_profile,
            family,
            object_id,
        ));
    }
    writer
        .as_mut()
        .ok_or(StorageError::InvalidState)?
        .push(filesystem, vault, entry)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn scan_prefix<F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    root: &RecoveredIndexRoot,
    family: u8,
    prefix: &[u8],
    maximum: usize,
    maximum_result_bytes: usize,
    cache: &mut PageCache,
) -> Result<IndexScan, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    scan_prefix_bounded(
        filesystem,
        context,
        vault,
        root,
        family,
        prefix,
        IndexScanLimits::new(MAX_INDEX_GET_PAGE_VISITS, maximum, maximum_result_bytes)?,
        cache,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn scan_prefix_bounded<F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    root: &RecoveredIndexRoot,
    family: u8,
    prefix: &[u8],
    limits: IndexScanLimits,
    cache: &mut PageCache,
) -> Result<IndexScan, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    let mut entries = Vec::new();
    let stats = scan_prefix_visit_bounded(
        filesystem,
        context,
        vault,
        root,
        family,
        prefix,
        limits,
        cache,
        &mut |entry| {
            entries
                .try_reserve(1)
                .map_err(|_| StorageError::ResourceLimit)?;
            entries.push(entry);
            Ok(())
        },
    )?;
    Ok(IndexScan { entries, stats })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn scan_prefix_visit<F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    root: &RecoveredIndexRoot,
    family: u8,
    prefix: &[u8],
    maximum: usize,
    maximum_result_bytes: usize,
    cache: &mut PageCache,
    visitor: &mut dyn FnMut(IndexScanEntry) -> Result<(), StorageError>,
) -> Result<IndexReadStats, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    scan_prefix_visit_bounded(
        filesystem,
        context,
        vault,
        root,
        family,
        prefix,
        IndexScanLimits::new(MAX_INDEX_GET_PAGE_VISITS, maximum, maximum_result_bytes)?,
        cache,
        visitor,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn scan_prefix_visit_bounded<F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    root: &RecoveredIndexRoot,
    family: u8,
    prefix: &[u8],
    limits: IndexScanLimits,
    cache: &mut PageCache,
    visitor: &mut dyn FnMut(IndexScanEntry) -> Result<(), StorageError>,
) -> Result<IndexReadStats, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    cache.observe_read(|cache, stats| {
        validate_read(root, context, prefix)?;
        let run = root.run(family)?;
        let mut page_visits = 0;
        let page_limits = IndexGetLimits {
            maximum_page_visits: limits.maximum_page_visits,
            maximum_result_bytes: MAX_INDEX_VALUE_BYTES,
        };
        let Some(mut page_index) = lower_bound_page_bounded(
            filesystem,
            context,
            vault,
            root,
            run,
            prefix,
            page_limits,
            &mut page_visits,
            cache,
            stats,
        )?
        else {
            return Ok(stats.clone());
        };
        let mut entry_count = 0_usize;
        let mut current_key = Vec::new();
        let mut current_value = Vec::new();
        let mut current_total = None;
        let mut finished = false;
        while page_index < run.page_count && !finished {
            let page = load_get_page(
                filesystem,
                context,
                vault,
                root,
                run,
                page_index,
                page_limits,
                &mut page_visits,
                cache,
                stats,
            )?;
            let parsed = ParsedPage::new(page, root, run, page_index)?;
            for fragment in parsed.fragments() {
                let fragment = fragment?;
                stats.fragments_visited = stats.fragments_visited.saturating_add(1);
                if fragment.key < prefix {
                    continue;
                }
                if !fragment.key.starts_with(prefix) {
                    finished = true;
                    break;
                }
                if current_total.is_none() {
                    if fragment.offset != 0 {
                        return Err(StorageError::IntegrityFailure);
                    }
                    let next_bytes = fragment
                        .key
                        .len()
                        .checked_add(fragment.total_len)
                        .and_then(|next| {
                            usize::try_from(stats.result_bytes).ok()?.checked_add(next)
                        })
                        .ok_or(StorageError::ResourceLimit)?;
                    if entry_count == limits.maximum_results
                        || next_bytes > limits.maximum_result_bytes
                    {
                        return Err(StorageError::ResourceLimit);
                    }
                    current_key.extend_from_slice(fragment.key);
                    current_total = Some(fragment.total_len);
                    current_value
                        .try_reserve_exact(fragment.total_len)
                        .map_err(|_| StorageError::ResourceLimit)?;
                }
                if current_key.as_slice() != fragment.key
                    || current_total != Some(fragment.total_len)
                    || current_value.len() != fragment.offset
                {
                    return Err(StorageError::IntegrityFailure);
                }
                current_value.extend_from_slice(fragment.value);
                if current_value.len() == fragment.total_len {
                    if entry_count == limits.maximum_results {
                        return Err(StorageError::ResourceLimit);
                    }
                    let next_bytes = current_key
                        .len()
                        .checked_add(current_value.len())
                        .and_then(|value| {
                            usize::try_from(stats.result_bytes)
                                .ok()
                                .and_then(|current| current.checked_add(value))
                        })
                        .ok_or(StorageError::ResourceLimit)?;
                    if next_bytes > limits.maximum_result_bytes {
                        return Err(StorageError::ResourceLimit);
                    }
                    stats.result_bytes =
                        u64::try_from(next_bytes).map_err(|_| StorageError::ResourceLimit)?;
                    visitor(IndexScanEntry {
                        key: core::mem::take(&mut current_key),
                        value: core::mem::take(&mut current_value),
                    })?;
                    entry_count = entry_count
                        .checked_add(1)
                        .ok_or(StorageError::ResourceLimit)?;
                    current_total = None;
                }
            }
            page_index = page_index
                .checked_add(1)
                .ok_or(StorageError::ResourceLimit)?;
        }
        if current_total.is_some() {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(stats.clone())
    })
}

struct RunWriter<F>
where
    F: FileSystem,
{
    file: F::File,
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    directory: F::Directory,
    scope: NamespaceRef,
    revision: CommitRevision,
    index_profile: [u8; 32],
    family: u8,
    object_id: [u8; 16],
    page: Vec<u8>,
    fragment_count: u32,
    page_count: u64,
    entry_count: u64,
    previous_key: Option<Vec<u8>>,
    digest: Sha256,
}

impl<F> RunWriter<F>
where
    F: FileSystem,
{
    #[allow(clippy::too_many_arguments)]
    fn new(
        file: F::File,
        context: &IndexContext<'_, F::Directory>,
        scope: NamespaceRef,
        revision: CommitRevision,
        index_profile: [u8; 32],
        family: u8,
        object_id: [u8; 16],
    ) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"USTE-INDEX-RUN-V1\0");
        digest.update(scope.namespace().as_bytes());
        digest.update(revision.get().to_be_bytes());
        digest.update(index_profile);
        digest.update([family]);
        Self {
            file,
            database: context.database,
            epoch: context.epoch,
            writer: context.writer,
            directory: context.directory.clone(),
            scope,
            revision,
            index_profile,
            family,
            object_id,
            page: new_page(revision, index_profile, family, object_id, 0),
            fragment_count: 0,
            page_count: 0,
            entry_count: 0,
            previous_key: None,
            digest,
        }
    }

    fn push<W, E>(
        &mut self,
        filesystem: &mut F,
        vault: &mut KeyVault<W, E>,
        entry: IndexEntry,
    ) -> Result<(), StorageError>
    where
        W: DurableKeyEnvelope,
        E: EntropySource,
    {
        if entry.key.is_empty()
            || entry.key.len() > MAX_INDEX_KEY_BYTES
            || entry.value.len() > MAX_INDEX_VALUE_BYTES
            || self
                .previous_key
                .as_ref()
                .is_some_and(|previous| previous.as_slice() >= entry.key.as_slice())
        {
            return Err(StorageError::InvalidState);
        }
        self.entry_count = self
            .entry_count
            .checked_add(1)
            .filter(|count| *count <= MAX_INDEX_ENTRIES_PER_RUN)
            .ok_or(StorageError::ResourceLimit)?;
        self.digest.update(
            u32::try_from(entry.key.len())
                .map_err(|_| StorageError::ResourceLimit)?
                .to_be_bytes(),
        );
        self.digest.update(
            u64::try_from(entry.value.len())
                .map_err(|_| StorageError::ResourceLimit)?
                .to_be_bytes(),
        );
        self.digest.update(&entry.key);
        self.digest.update(&entry.value);

        let mut offset = 0_usize;
        loop {
            let required_without_value = FRAGMENT_HEADER_BYTES
                .checked_add(entry.key.len())
                .ok_or(StorageError::ResourceLimit)?;
            if self
                .page
                .len()
                .checked_add(required_without_value)
                .ok_or(StorageError::ResourceLimit)?
                > INDEX_PAGE_BYTES
            {
                self.flush_page(filesystem, vault)?;
            }
            let available = INDEX_PAGE_BYTES
                .checked_sub(self.page.len())
                .and_then(|value| value.checked_sub(required_without_value))
                .ok_or(StorageError::ResourceLimit)?;
            let remaining = entry.value.len() - offset;
            let fragment_len = remaining.min(available);
            if remaining != 0 && fragment_len == 0 {
                self.flush_page(filesystem, vault)?;
                continue;
            }
            extend(&mut self.page, &(entry.key.len() as u32).to_be_bytes())?;
            extend(
                &mut self.page,
                &u32::try_from(entry.value.len())
                    .map_err(|_| StorageError::ResourceLimit)?
                    .to_be_bytes(),
            )?;
            extend(
                &mut self.page,
                &u32::try_from(offset)
                    .map_err(|_| StorageError::ResourceLimit)?
                    .to_be_bytes(),
            )?;
            extend(
                &mut self.page,
                &u32::try_from(fragment_len)
                    .map_err(|_| StorageError::ResourceLimit)?
                    .to_be_bytes(),
            )?;
            extend(&mut self.page, &entry.key)?;
            extend(&mut self.page, &entry.value[offset..offset + fragment_len])?;
            self.fragment_count = self
                .fragment_count
                .checked_add(1)
                .ok_or(StorageError::ResourceLimit)?;
            offset = offset
                .checked_add(fragment_len)
                .ok_or(StorageError::ResourceLimit)?;
            if offset == entry.value.len() {
                break;
            }
            self.flush_page(filesystem, vault)?;
        }
        self.previous_key = Some(entry.key);
        Ok(())
    }

    fn flush_page<W, E>(
        &mut self,
        filesystem: &mut F,
        vault: &mut KeyVault<W, E>,
    ) -> Result<(), StorageError>
    where
        W: DurableKeyEnvelope,
        E: EntropySource,
    {
        if self.fragment_count == 0 {
            return Err(StorageError::InvalidState);
        }
        if self.page_count >= MAX_INDEX_PAGES_PER_RUN {
            return Err(StorageError::ResourceLimit);
        }
        let used = self.page.len();
        self.page[24..28].copy_from_slice(&self.fragment_count.to_be_bytes());
        self.page[28..32].copy_from_slice(
            &u32::try_from(used)
                .map_err(|_| StorageError::ResourceLimit)?
                .to_be_bytes(),
        );
        self.page.resize(INDEX_PAGE_BYTES, 0);
        let sequence = self
            .page_count
            .checked_add(1)
            .ok_or(StorageError::ResourceLimit)?;
        let context = IndexContext {
            database: self.database,
            epoch: self.epoch,
            writer: self.writer,
            directory: &self.directory,
        };
        let encoded = vault
            .encrypt(
                index_context(
                    &context,
                    self.scope.namespace(),
                    self.object_id,
                    sequence,
                    FrameClass::Small4KiB,
                    ObjectRole::IndexPage,
                ),
                &self.page,
            )?
            .encode()?;
        if u64::try_from(encoded.len()).map_err(|_| StorageError::ResourceLimit)?
            != ENCODED_PAGE_BYTES
        {
            return Err(StorageError::IntegrityFailure);
        }
        let file_offset = self
            .page_count
            .checked_mul(ENCODED_PAGE_BYTES)
            .ok_or(StorageError::ResourceLimit)?;
        write_all_at(filesystem, &self.file, file_offset, &encoded)?;
        self.page_count = sequence;
        self.page = new_page(
            self.revision,
            self.index_profile,
            self.family,
            self.object_id,
            self.page_count,
        );
        self.fragment_count = 0;
        Ok(())
    }

    fn finish<W, E>(
        mut self,
        filesystem: &mut F,
        vault: &mut KeyVault<W, E>,
    ) -> Result<IndexRunDescriptor, StorageError>
    where
        W: DurableKeyEnvelope,
        E: EntropySource,
    {
        if self.entry_count == 0 {
            return Err(StorageError::InvalidState);
        }
        if self.fragment_count != 0 {
            self.flush_page(filesystem, vault)?;
        }
        let exact_len = self
            .page_count
            .checked_mul(ENCODED_PAGE_BYTES)
            .ok_or(StorageError::ResourceLimit)?;
        filesystem.set_len(&self.file, exact_len)?;
        filesystem.sync_all(&self.file)?;
        Ok(IndexRunDescriptor {
            scope: self.scope,
            revision: self.revision,
            index_profile: self.index_profile,
            epoch: self.epoch,
            writer: self.writer,
            family: self.family,
            object_id: self.object_id,
            page_count: self.page_count,
            entry_count: self.entry_count,
            logical_digest: self.digest.finalize().into(),
        })
    }
}

fn new_page(
    revision: CommitRevision,
    index_profile: [u8; 32],
    family: u8,
    object_id: [u8; 16],
    page_index: u64,
) -> Vec<u8> {
    let mut page = Vec::with_capacity(INDEX_PAGE_BYTES);
    page.extend_from_slice(PAGE_MAGIC);
    page.push(MAJOR);
    page.push(MINOR);
    page.push(family);
    page.push(0);
    page.extend_from_slice(&revision.get().to_be_bytes());
    page.extend_from_slice(&page_index.to_be_bytes());
    page.extend_from_slice(&[0; 8]);
    page.extend_from_slice(&index_profile);
    page.extend_from_slice(&object_id);
    debug_assert_eq!(page.len(), PAGE_HEADER_BYTES);
    page
}

struct RootManifest {
    input: IndexRootInput,
    generation: u64,
    object_id: [u8; 16],
    runs: Vec<IndexRunDescriptor>,
}

impl RootManifest {
    fn encode(&self) -> Result<[u8; ROOT_BYTES], StorageError> {
        validate_runs(&self.runs)?;
        let mut bytes = [0_u8; ROOT_BYTES];
        bytes[..4].copy_from_slice(ROOT_MAGIC);
        bytes[4] = MAJOR;
        bytes[5] = MINOR;
        bytes[6] = u8::try_from(self.runs.len()).map_err(|_| StorageError::ResourceLimit)?;
        bytes[8..24].copy_from_slice(self.input.scope.namespace().as_bytes());
        bytes[24..32].copy_from_slice(&self.input.revision.get().to_be_bytes());
        bytes[32..40].copy_from_slice(&self.generation.to_be_bytes());
        bytes[40..72].copy_from_slice(&self.input.certificate_digest);
        bytes[72..104].copy_from_slice(&self.input.reducer_profile);
        bytes[104..136].copy_from_slice(&self.input.logical_state_digest);
        bytes[136..168].copy_from_slice(&self.input.index_profile);
        bytes[168..184].copy_from_slice(&self.object_id);
        for (index, run) in self.runs.iter().enumerate() {
            let start = ROOT_HEADER_BYTES + index * RUN_DESCRIPTOR_BYTES;
            bytes[start] = run.family;
            bytes[start + 8..start + 24].copy_from_slice(&run.object_id);
            bytes[start + 24..start + 32].copy_from_slice(&run.page_count.to_be_bytes());
            bytes[start + 32..start + 40].copy_from_slice(&run.entry_count.to_be_bytes());
            bytes[start + 40..start + 72].copy_from_slice(&run.logical_digest);
        }
        Ok(bytes)
    }

    fn decode(
        bytes: &[u8],
        database: DatabaseId,
        epoch: KeyEpoch,
        writer: WriterIncarnationId,
    ) -> Result<Self, StorageError> {
        if bytes.len() != ROOT_BYTES
            || &bytes[..4] != ROOT_MAGIC
            || bytes[4] != MAJOR
            || bytes[5] != MINOR
            || bytes[7] != 0
            || bytes[184..192].iter().any(|byte| *byte != 0)
        {
            return Err(StorageError::IntegrityFailure);
        }
        let run_count = usize::from(bytes[6]);
        if run_count == 0 || run_count > MAX_INDEX_RUNS {
            return Err(StorageError::IntegrityFailure);
        }
        let revision = CommitRevision::new(read_u64(bytes, 24)?)
            .map_err(|_| StorageError::IntegrityFailure)?;
        let generation = read_u64(bytes, 32)?;
        let object_id = read_array(bytes, 168)?;
        if generation == 0 || object_id == [0; 16] {
            return Err(StorageError::IntegrityFailure);
        }
        let mut runs = Vec::new();
        runs.try_reserve_exact(run_count)
            .map_err(|_| StorageError::ResourceLimit)?;
        for index in 0..run_count {
            let start = ROOT_HEADER_BYTES + index * RUN_DESCRIPTOR_BYTES;
            if bytes[start + 1..start + 8].iter().any(|byte| *byte != 0) {
                return Err(StorageError::IntegrityFailure);
            }
            runs.push(IndexRunDescriptor {
                scope: NamespaceRef::new(database, NamespaceId::from_bytes(read_array(bytes, 8)?)),
                revision,
                index_profile: read_array(bytes, 136)?,
                epoch,
                writer,
                family: bytes[start],
                object_id: read_array(bytes, start + 8)?,
                page_count: read_u64(bytes, start + 24)?,
                entry_count: read_u64(bytes, start + 32)?,
                logical_digest: read_array(bytes, start + 40)?,
            });
        }
        let used = ROOT_HEADER_BYTES + run_count * RUN_DESCRIPTOR_BYTES;
        if bytes[used..].iter().any(|byte| *byte != 0) {
            return Err(StorageError::IntegrityFailure);
        }
        validate_runs(&runs)?;
        Ok(Self {
            input: IndexRootInput {
                scope: NamespaceRef::new(database, NamespaceId::from_bytes(read_array(bytes, 8)?)),
                revision,
                certificate_digest: read_array(bytes, 40)?,
                reducer_profile: read_array(bytes, 72)?,
                logical_state_digest: read_array(bytes, 104)?,
                index_profile: read_array(bytes, 136)?,
            },
            generation,
            object_id,
            runs,
        })
    }

    fn recovered(&self) -> RecoveredIndexRoot {
        RecoveredIndexRoot {
            scope: self.input.scope,
            revision: self.input.revision,
            generation: self.generation,
            certificate_digest: self.input.certificate_digest,
            reducer_profile: self.input.reducer_profile,
            logical_state_digest: self.input.logical_state_digest,
            index_profile: self.input.index_profile,
            runs: self.runs.clone(),
        }
    }
}

fn validate_runs(runs: &[IndexRunDescriptor]) -> Result<(), StorageError> {
    if runs.is_empty() || runs.len() > MAX_INDEX_RUNS {
        return Err(StorageError::ResourceLimit);
    }
    let mut previous = 0_u8;
    for run in runs {
        if run.family == 0
            || run.family <= previous
            || run.object_id == [0; 16]
            || run.page_count == 0
            || run.page_count > MAX_INDEX_PAGES_PER_RUN
            || run.entry_count == 0
            || run.entry_count > MAX_INDEX_ENTRIES_PER_RUN
        {
            return Err(StorageError::IntegrityFailure);
        }
        previous = run.family;
    }
    Ok(())
}

fn validate_run_bindings<D>(
    runs: &[IndexRunDescriptor],
    input: IndexRootInput,
    context: &IndexContext<'_, D>,
) -> Result<(), StorageError> {
    validate_runs(runs)?;
    if runs.iter().any(|run| {
        run.scope != input.scope
            || run.revision != input.revision
            || run.index_profile != input.index_profile
            || run.epoch != context.epoch
            || run.writer != context.writer
    }) {
        return Err(StorageError::InvalidState);
    }
    Ok(())
}

fn load_candidates<F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    scope: NamespaceRef,
    index_profile: [u8; 32],
) -> Result<Vec<(Slot, RecoveredIndexRoot)>, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    let manifests = load_manifest_candidates(filesystem, context, vault, scope, index_profile)?;
    let mut candidates = Vec::new();
    for (slot, root) in manifests {
        // A root is usable only when all referenced durable pages authenticate and reproduce
        // the committed run digests. Publication must make its overwrite choice from usable
        // roots so a corrupt newest run cannot cause the sole good fallback to be removed.
        let mut cache = PageCache::default();
        match scrub(filesystem, context, vault, &root, &mut cache) {
            Ok(_) => candidates.push((slot, root)),
            Err(StorageError::IntegrityFailure | StorageError::UnsupportedProfile) => {}
            Err(error) => return Err(error),
        }
    }
    validate_candidate_generations(&candidates)?;
    Ok(candidates)
}

fn load_candidates_bounded<F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    scope: NamespaceRef,
    index_profile: [u8; 32],
    limits: IndexRunReadLimits,
) -> Result<Vec<(Slot, RecoveredIndexRoot)>, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    let manifests = load_manifest_candidates(filesystem, context, vault, scope, index_profile)?;
    let mut candidates = Vec::new();
    for (slot, root) in manifests {
        let mut usable = true;
        for run in root.runs() {
            match visit_run(
                filesystem,
                context,
                vault,
                &root,
                run.family,
                limits,
                &mut |_, _| Ok(()),
            ) {
                Ok(_) => {}
                Err(StorageError::IntegrityFailure | StorageError::UnsupportedProfile) => {
                    usable = false;
                    break;
                }
                // A budget or I/O refusal is not evidence of corruption. Do not overwrite a
                // potentially sole usable fallback that was merely too expensive to validate.
                Err(error) => return Err(error),
            }
        }
        if usable {
            candidates.push((slot, root));
        }
    }
    validate_candidate_generations(&candidates)?;
    Ok(candidates)
}

fn load_manifest_candidates<F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    scope: NamespaceRef,
    index_profile: [u8; 32],
) -> Result<Vec<(Slot, RecoveredIndexRoot)>, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    let mut candidates = Vec::new();
    for slot in [Slot::A, Slot::B] {
        if let Some(root) = load_slot(filesystem, context, vault, scope, index_profile, slot)? {
            candidates.push((slot, root));
        }
    }
    candidates.sort_by_key(|candidate| core::cmp::Reverse(candidate.1.generation));
    Ok(candidates)
}

fn validate_candidate_generations(
    candidates: &[(Slot, RecoveredIndexRoot)],
) -> Result<(), StorageError> {
    if candidates.len() == 2
        && candidates[0].1.generation == candidates[1].1.generation
        && candidates[0].1 != candidates[1].1
    {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(())
}

fn load_slot<F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    expected_scope: NamespaceRef,
    expected_profile: [u8; 32],
    slot: Slot,
) -> Result<Option<RecoveredIndexRoot>, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    let name = root_name(vault, context, expected_scope, expected_profile, slot)?;
    let file = match filesystem.open_existing(context.directory, &name) {
        Ok(file) => file,
        Err(error) if error.kind() == AdapterErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if filesystem.metadata(&file)?.len != ROOT_FILE_BYTES {
        return Ok(None);
    }
    let mut object_id = [0_u8; 16];
    if let Err(error) = read_exact_at(filesystem, &file, 0, &mut object_id) {
        return if error.kind() == AdapterErrorKind::UnexpectedEof {
            Ok(None)
        } else {
            Err(error.into())
        };
    }
    if object_id == [0; 16] {
        return Ok(None);
    }
    let mut encoded = vec![0_u8; SMALL_ENVELOPE_BYTES];
    if let Err(error) = read_exact_at(filesystem, &file, 16, &mut encoded) {
        return if error.kind() == AdapterErrorKind::UnexpectedEof {
            Ok(None)
        } else {
            Err(error.into())
        };
    }
    let envelope = match EncryptedEnvelope::decode(&encoded) {
        Ok(envelope) => envelope,
        Err(
            CryptoError::IntegrityFailure
            | CryptoError::InvalidEnvelope
            | CryptoError::UnsupportedProfile,
        ) => {
            return Ok(None);
        }
        Err(error) => return Err(StorageError::Crypto(error)),
    };
    let plaintext = match vault.decrypt(
        index_context(
            context,
            expected_scope.namespace(),
            object_id,
            0,
            FrameClass::Small4KiB,
            ObjectRole::IndexPage,
        ),
        &envelope,
    ) {
        Ok(plaintext) => plaintext,
        Err(
            CryptoError::IntegrityFailure
            | CryptoError::InvalidEnvelope
            | CryptoError::UnsupportedProfile,
        ) => {
            return Ok(None);
        }
        Err(error) => return Err(StorageError::Crypto(error)),
    };
    let manifest = match RootManifest::decode(
        plaintext.as_slice(),
        context.database,
        context.epoch,
        context.writer,
    ) {
        Ok(manifest) => manifest,
        Err(StorageError::IntegrityFailure | StorageError::UnsupportedProfile) => return Ok(None),
        Err(error) => return Err(error),
    };
    if manifest.input.scope != expected_scope
        || manifest.input.index_profile != expected_profile
        || manifest.object_id != object_id
    {
        return Ok(None);
    }
    for run in &manifest.runs {
        let run_file = match filesystem.open_existing(context.directory, &run_name(run.object_id)?)
        {
            Ok(file) => file,
            Err(error) if error.kind() == AdapterErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let expected_len = run
            .page_count
            .checked_mul(ENCODED_PAGE_BYTES)
            .ok_or(StorageError::ResourceLimit)?;
        if filesystem.metadata(&run_file)?.len != expected_len {
            return Ok(None);
        }
    }
    Ok(Some(manifest.recovered()))
}

#[allow(clippy::too_many_arguments)]
fn lower_bound_page_bounded<F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    root: &RecoveredIndexRoot,
    run: &IndexRunDescriptor,
    key: &[u8],
    limits: IndexGetLimits,
    page_visits: &mut u64,
    cache: &mut PageCache,
    stats: &mut IndexReadStats,
) -> Result<Option<u64>, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    let mut low = 0_u64;
    let mut high = run.page_count;
    while low < high {
        let middle = low + (high - low) / 2;
        let page = load_get_page(
            filesystem,
            context,
            vault,
            root,
            run,
            middle,
            limits,
            page_visits,
            cache,
            stats,
        )?;
        let parsed = ParsedPage::new(page, root, run, middle)?;
        if parsed.last_key()? < key {
            low = middle.checked_add(1).ok_or(StorageError::ResourceLimit)?;
        } else {
            high = middle;
        }
    }
    Ok((low < run.page_count).then_some(low))
}

#[allow(clippy::too_many_arguments)]
fn load_get_page<'a, F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    root: &RecoveredIndexRoot,
    run: &IndexRunDescriptor,
    page_index: u64,
    limits: IndexGetLimits,
    page_visits: &mut u64,
    cache: &'a mut PageCache,
    stats: &mut IndexReadStats,
) -> Result<&'a [u8], StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    *page_visits = page_visits
        .checked_add(1)
        .filter(|visits| *visits <= limits.maximum_page_visits)
        .ok_or(StorageError::ResourceLimit)?;
    load_page(
        filesystem, context, vault, root, run, page_index, cache, stats,
    )
}

#[allow(clippy::too_many_arguments)]
fn load_predecessor_page<'a, F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    root: &RecoveredIndexRoot,
    run: &IndexRunDescriptor,
    page_index: u64,
    limits: IndexPredecessorLimits,
    page_visits: &mut u64,
    cache: &'a mut PageCache,
    stats: &mut IndexReadStats,
) -> Result<&'a [u8], StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    *page_visits = page_visits
        .checked_add(1)
        .filter(|visits| *visits <= limits.maximum_page_visits)
        .ok_or(StorageError::ResourceLimit)?;
    load_page(
        filesystem, context, vault, root, run, page_index, cache, stats,
    )
}

#[allow(clippy::too_many_arguments)]
fn load_page<'a, F, W, E>(
    filesystem: &mut F,
    context: &IndexContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    root: &RecoveredIndexRoot,
    run: &IndexRunDescriptor,
    page_index: u64,
    cache: &'a mut PageCache,
    stats: &mut IndexReadStats,
) -> Result<&'a [u8], StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    if page_index >= run.page_count {
        return Err(StorageError::IntegrityFailure);
    }
    let key = CacheKey {
        database: context.database,
        namespace: root.scope.namespace(),
        epoch: context.epoch,
        writer: context.writer,
        revision: root.revision,
        index_profile: root.index_profile,
        generation: root.generation,
        object_id: run.object_id,
        page: page_index,
    };
    if cache.touch(key).is_none() {
        let file = filesystem.open_existing(context.directory, &run_name(run.object_id)?)?;
        let offset = page_index
            .checked_mul(ENCODED_PAGE_BYTES)
            .ok_or(StorageError::ResourceLimit)?;
        let mut encoded = vec![0_u8; ENCODED_PAGE_BYTES as usize];
        read_exact_at(filesystem, &file, offset, &mut encoded)?;
        let envelope = EncryptedEnvelope::decode(&encoded).map_err(index_crypto_error)?;
        let plaintext = vault
            .decrypt(
                index_context(
                    context,
                    root.scope.namespace(),
                    run.object_id,
                    page_index
                        .checked_add(1)
                        .ok_or(StorageError::ResourceLimit)?,
                    FrameClass::Small4KiB,
                    ObjectRole::IndexPage,
                ),
                &envelope,
            )
            .map_err(index_crypto_error)?;
        if plaintext.as_slice().len() != INDEX_PAGE_BYTES {
            return Err(StorageError::IntegrityFailure);
        }
        let mut page = Vec::new();
        page.try_reserve_exact(INDEX_PAGE_BYTES)
            .map_err(|_| StorageError::ResourceLimit)?;
        page.extend_from_slice(plaintext.as_slice());
        cache.insert(key, page)?;
        stats.pages_read = stats.pages_read.saturating_add(1);
    } else {
        stats.cache_hits = stats.cache_hits.saturating_add(1);
    }
    cache
        .pages
        .get(&key)
        .map(|page| page.bytes.as_ref())
        .ok_or(StorageError::IntegrityFailure)
}

fn validate_read<D>(
    root: &RecoveredIndexRoot,
    context: &IndexContext<'_, D>,
    key: &[u8],
) -> Result<(), StorageError> {
    if root.scope.database() != context.database
        || key.is_empty()
        || key.len() > MAX_INDEX_KEY_BYTES
    {
        return Err(StorageError::InvalidState);
    }
    Ok(())
}

struct ParsedPage<'a> {
    bytes: &'a [u8],
    used: usize,
    fragment_count: usize,
}

impl<'a> ParsedPage<'a> {
    fn new(
        bytes: &'a [u8],
        root: &RecoveredIndexRoot,
        run: &IndexRunDescriptor,
        page_index: u64,
    ) -> Result<Self, StorageError> {
        if bytes.len() != INDEX_PAGE_BYTES
            || &bytes[..4] != PAGE_MAGIC
            || bytes[4] != MAJOR
            || bytes[5] != MINOR
            || bytes[6] != run.family
            || bytes[7] != 0
            || read_u64(bytes, 8)? != root.revision.get()
            || read_u64(bytes, 16)? != page_index
            || read_array::<32>(bytes, 32)? != root.index_profile
            || read_array::<16>(bytes, 64)? != run.object_id
        {
            return Err(StorageError::IntegrityFailure);
        }
        let fragment_count =
            usize::try_from(read_u32(bytes, 24)?).map_err(|_| StorageError::ResourceLimit)?;
        let used =
            usize::try_from(read_u32(bytes, 28)?).map_err(|_| StorageError::ResourceLimit)?;
        if fragment_count == 0
            || !(PAGE_HEADER_BYTES..=INDEX_PAGE_BYTES).contains(&used)
            || bytes[used..].iter().any(|byte| *byte != 0)
        {
            return Err(StorageError::IntegrityFailure);
        }
        let page = Self {
            bytes,
            used,
            fragment_count,
        };
        let fragments = page.fragments().collect::<Result<Vec<_>, _>>()?;
        if fragments.len() != fragment_count
            || fragments.windows(2).any(|pair| {
                pair[0].key > pair[1].key
                    || (pair[0].key == pair[1].key && pair[0].offset >= pair[1].offset)
            })
        {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(page)
    }

    fn fragments(&self) -> FragmentIter<'a> {
        FragmentIter {
            remaining: &self.bytes[PAGE_HEADER_BYTES..self.used],
            remaining_count: self.fragment_count,
        }
    }

    fn last_key(&self) -> Result<&'a [u8], StorageError> {
        self.fragments()
            .last()
            .ok_or(StorageError::IntegrityFailure)?
            .map(|fragment| fragment.key)
    }
}

#[derive(Clone, Copy)]
struct Fragment<'a> {
    key: &'a [u8],
    total_len: usize,
    offset: usize,
    value: &'a [u8],
}

struct OwnedFragment {
    key: Vec<u8>,
    total_len: usize,
    offset: usize,
    value: Vec<u8>,
}

struct FragmentIter<'a> {
    remaining: &'a [u8],
    remaining_count: usize,
}

impl<'a> Iterator for FragmentIter<'a> {
    type Item = Result<Fragment<'a>, StorageError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining_count == 0 {
            return if self.remaining.is_empty() {
                None
            } else {
                self.remaining = &[];
                Some(Err(StorageError::IntegrityFailure))
            };
        }
        let result = (|| {
            if self.remaining.len() < FRAGMENT_HEADER_BYTES {
                return Err(StorageError::IntegrityFailure);
            }
            let key_len = usize::try_from(read_u32(self.remaining, 0)?)
                .map_err(|_| StorageError::ResourceLimit)?;
            let total_len = usize::try_from(read_u32(self.remaining, 4)?)
                .map_err(|_| StorageError::ResourceLimit)?;
            let offset = usize::try_from(read_u32(self.remaining, 8)?)
                .map_err(|_| StorageError::ResourceLimit)?;
            let fragment_len = usize::try_from(read_u32(self.remaining, 12)?)
                .map_err(|_| StorageError::ResourceLimit)?;
            if key_len == 0
                || key_len > MAX_INDEX_KEY_BYTES
                || total_len > MAX_INDEX_VALUE_BYTES
                || offset > total_len
                || fragment_len > total_len.saturating_sub(offset)
                || (total_len != 0 && fragment_len == 0)
            {
                return Err(StorageError::IntegrityFailure);
            }
            let payload_len = key_len
                .checked_add(fragment_len)
                .ok_or(StorageError::ResourceLimit)?;
            let consumed = FRAGMENT_HEADER_BYTES
                .checked_add(payload_len)
                .ok_or(StorageError::ResourceLimit)?;
            let payload = self
                .remaining
                .get(FRAGMENT_HEADER_BYTES..consumed)
                .ok_or(StorageError::IntegrityFailure)?;
            let (key, value) = payload.split_at(key_len);
            self.remaining = &self.remaining[consumed..];
            self.remaining_count -= 1;
            Ok(Fragment {
                key,
                total_len,
                offset,
                value,
            })
        })();
        if result.is_err() {
            self.remaining = &[];
            self.remaining_count = 0;
        }
        Some(result)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Slot {
    A,
    B,
}

impl Slot {
    const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }

    const fn tag(self) -> u8 {
        match self {
            Self::A => 0,
            Self::B => 1,
        }
    }
}

fn root_name<W, E: EntropySource, D>(
    vault: &KeyVault<W, E>,
    context: &IndexContext<'_, D>,
    scope: NamespaceRef,
    profile: [u8; 32],
    slot: Slot,
) -> Result<EntryName, StorageError> {
    let mut digest = Sha256::new();
    digest.update(b"USTE-INDEX-ROOT-NAME-V1\0");
    digest.update(scope.namespace().as_bytes());
    digest.update(profile);
    digest.update([slot.tag()]);
    let input: [u8; 32] = digest.finalize().into();
    let token = vault.derive_opaque_identifier(
        index_context(
            context,
            scope.namespace(),
            [0; 16],
            u64::from(slot.tag()),
            FrameClass::Small4KiB,
            ObjectRole::IndexName,
        ),
        &input,
    )?;
    EntryName::new(format!("x-{}", hex(&token))).map_err(|_| StorageError::IntegrityFailure)
}

fn run_name(object_id: [u8; 16]) -> Result<EntryName, StorageError> {
    if object_id == [0; 16] {
        return Err(StorageError::IntegrityFailure);
    }
    EntryName::new(format!("i-{}", hex(&object_id))).map_err(|_| StorageError::IntegrityFailure)
}

#[allow(clippy::too_many_arguments)]
fn index_context<D>(
    context: &IndexContext<'_, D>,
    namespace: NamespaceId,
    object_id: [u8; 16],
    sequence: u64,
    frame: FrameClass,
    role: ObjectRole,
) -> CryptoContext {
    CryptoContext::new(
        context.database,
        Scope::Namespace(namespace),
        context.epoch,
        role,
        CryptoObjectId::from_bytes(object_id),
        sequence,
        context.writer,
        MAJOR,
        MINOR,
        frame,
    )
}

fn remove_if_present<F: FileSystem>(
    filesystem: &mut F,
    directory: &F::Directory,
    name: &EntryName,
) -> Result<(), StorageError> {
    match filesystem.remove_file(directory, name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == AdapterErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn random_nonzero_id(entropy: &mut impl EntropySource) -> Result<[u8; 16], StorageError> {
    for _ in 0..8 {
        let mut bytes = [0_u8; 16];
        entropy
            .fill(&mut bytes)
            .map_err(|_| StorageError::Crypto(CryptoError::RetryableUnavailable))?;
        if bytes != [0; 16] {
            return Ok(bytes);
        }
    }
    Err(StorageError::Crypto(CryptoError::IntegrityFailure))
}

fn extend(output: &mut Vec<u8>, value: &[u8]) -> Result<(), StorageError> {
    let next = output
        .len()
        .checked_add(value.len())
        .ok_or(StorageError::ResourceLimit)?;
    if next > INDEX_PAGE_BYTES {
        return Err(StorageError::ResourceLimit);
    }
    output
        .try_reserve(value.len())
        .map_err(|_| StorageError::ResourceLimit)?;
    output.extend_from_slice(value);
    Ok(())
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, StorageError> {
    Ok(u32::from_be_bytes(read_array(bytes, offset)?))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, StorageError> {
    Ok(u64::from_be_bytes(read_array(bytes, offset)?))
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], StorageError> {
    bytes
        .get(
            offset
                ..offset
                    .checked_add(N)
                    .ok_or(StorageError::IntegrityFailure)?,
        )
        .ok_or(StorageError::IntegrityFailure)?
        .try_into()
        .map_err(|_| StorageError::IntegrityFailure)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

const fn index_crypto_error(error: CryptoError) -> StorageError {
    match error {
        CryptoError::ResourceLimit => StorageError::ResourceLimit,
        CryptoError::UnsupportedProfile => StorageError::UnsupportedProfile,
        _ => StorageError::IntegrityFailure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_key(page: u64) -> CacheKey {
        CacheKey {
            database: DatabaseId::from_bytes([0x11; 16]),
            namespace: NamespaceId::from_bytes([0x22; 16]),
            epoch: KeyEpoch::new(1).unwrap(),
            writer: WriterIncarnationId::from_bytes([0x33; 16]),
            revision: CommitRevision::FIRST,
            index_profile: [0x44; 32],
            generation: 1,
            object_id: [0x55; 16],
            page,
        }
    }

    fn assert_cache_invariants(cache: &PageCache) {
        assert_eq!(cache.pages.len(), cache.recency.len());
        for (key, page) in &cache.pages {
            assert!(cache.recency.get(&page.last_used) == Some(key));
            assert_eq!(page.bytes.len(), INDEX_PAGE_BYTES);
        }
        let expected_bytes = if cache.pages.is_empty() {
            0
        } else {
            CACHE_FIXED_OVERHEAD + cache.pages.len() * CACHE_ENTRY_BYTES
        };
        assert_eq!(cache.accounted_bytes, expected_bytes);
        assert!(cache.accounted_bytes <= cache.budget);
    }

    #[test]
    fn ordered_cache_eviction_matches_independent_lru_traces() {
        for capacity in [1, 2, 17, 128] {
            let mut cache =
                PageCache::new(CACHE_FIXED_OVERHEAD + capacity * CACHE_ENTRY_BYTES).unwrap();
            let mut reference = Vec::new();
            let (mut hits, mut misses, mut evictions) = (0, 0, 0);
            let mut state = 19_u64;
            for step in 0..5000 {
                if step % 257 == 0 {
                    cache.clear();
                    reference.clear();
                }
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                let key = cache_key((state >> 32) % 193);
                let position = reference.iter().position(|candidate| *candidate == key);
                let actual = cache.touch(key);
                assert_eq!(actual.is_some(), position.is_some());
                if let Some(position) = position {
                    assert_eq!(actual.unwrap()[0], key.page as u8);
                    reference.remove(position);
                    hits += 1;
                } else {
                    misses += 1;
                    if reference.len() == capacity {
                        reference.remove(0);
                        evictions += 1;
                    }
                    cache
                        .insert(key, vec![key.page as u8; INDEX_PAGE_BYTES])
                        .unwrap();
                }
                reference.push(key);
                assert!(
                    cache
                        .recency
                        .values()
                        .copied()
                        .eq(reference.iter().copied())
                );
                assert_cache_invariants(&cache);
                assert_eq!(
                    (cache.hits, cache.misses, cache.evictions),
                    (hits, misses, evictions)
                );
            }
        }
    }

    #[test]
    fn cache_admission_clock_overflow_and_duplicate_insert_preserve_invariants() {
        assert_eq!(MIN_INDEX_CACHE_BYTES, 25_600);
        assert!(PageCache::new(MIN_INDEX_CACHE_BYTES - 1).is_err());
        assert!(PageCache::new(MAX_INDEX_CACHE_BYTES + 1).is_err());
        assert!(PageCache::new(MAX_INDEX_CACHE_BYTES).is_ok());
        let mut cache = PageCache::new(MIN_INDEX_CACHE_BYTES).unwrap();
        let key = cache_key(1);
        cache.insert(key, vec![7; INDEX_PAGE_BYTES]).unwrap();
        let before_clock = cache.clock;
        assert_eq!(
            cache.insert(key, vec![9; INDEX_PAGE_BYTES]),
            Err(StorageError::IntegrityFailure)
        );
        assert_eq!(
            cache.insert(cache_key(2), vec![9; INDEX_PAGE_BYTES - 1]),
            Err(StorageError::IntegrityFailure)
        );
        assert_eq!(cache.clock, before_clock);
        assert_eq!(cache.touch(key).unwrap()[0], 7);
        assert_eq!(cache.accounted_bytes, MIN_INDEX_CACHE_BYTES);
        cache.clock = u64::MAX;
        assert!(cache.touch(key).is_none());
        assert_eq!(cache.clock, 1);
        assert_eq!(cache.evictions, 1);
        assert_cache_invariants(&cache);
        cache.insert(key, vec![7; INDEX_PAGE_BYTES]).unwrap();
        cache.clock = u64::MAX;
        cache
            .insert(cache_key(2), vec![9; INDEX_PAGE_BYTES])
            .unwrap();
        assert_eq!(cache.clock, 1);
        assert_eq!(cache.evictions, 2);
        assert_eq!(cache.touch(cache_key(2)).unwrap()[0], 9);
        assert_cache_invariants(&cache);
        cache.clear();
        assert_cache_invariants(&cache);
    }

    #[test]
    fn ordered_cache_keeps_every_context_field_distinct() {
        let original = cache_key(0);
        let keys = [
            original,
            CacheKey {
                database: DatabaseId::from_bytes([1; 16]),
                ..original
            },
            CacheKey {
                namespace: NamespaceId::from_bytes([2; 16]),
                ..original
            },
            CacheKey {
                epoch: KeyEpoch::new(2).unwrap(),
                ..original
            },
            CacheKey {
                writer: WriterIncarnationId::from_bytes([3; 16]),
                ..original
            },
            CacheKey {
                revision: CommitRevision::new(2).unwrap(),
                ..original
            },
            CacheKey {
                index_profile: [4; 32],
                ..original
            },
            CacheKey {
                generation: 2,
                ..original
            },
            CacheKey {
                object_id: [5; 16],
                ..original
            },
            cache_key(1),
        ];
        let mut cache =
            PageCache::new(CACHE_FIXED_OVERHEAD + keys.len() * CACHE_ENTRY_BYTES).unwrap();
        for (i, key) in keys.iter().enumerate() {
            cache.insert(*key, vec![i as u8; INDEX_PAGE_BYTES]).unwrap();
        }
        for (i, key) in keys.iter().enumerate().rev() {
            assert_eq!(cache.touch(*key).unwrap()[0], i as u8);
        }
        assert_cache_invariants(&cache);
        assert_eq!(cache.evictions, 0);
    }

    #[test]
    fn cache_layout_allowances_and_default_capacity_are_explicit() {
        // These checks cover owned inline types, not opaque std collection allocations.
        // The separate root/node allowances must still be checked against measured RSS.
        let inline_pair = size_of::<(CacheKey, CachedPage)>() + size_of::<(u64, CacheKey)>();
        assert!(inline_pair <= CACHE_ENTRY_OVERHEAD);
        assert!(size_of::<PageCache>() <= CACHE_FIXED_OVERHEAD);
        let mut cache = PageCache::default();
        let capacity = (DEFAULT_INDEX_CACHE_BYTES - CACHE_FIXED_OVERHEAD) / CACHE_ENTRY_BYTES;
        assert_eq!(capacity, 3854);
        for page in 0..capacity {
            cache
                .insert(cache_key(page as u64), vec![0x5a; INDEX_PAGE_BYTES])
                .unwrap();
        }
        assert!(
            cache
                .pages
                .values()
                .all(|page| page.bytes.iter().all(|byte| *byte == 0x5a))
        );
        assert_cache_invariants(&cache);
        assert_eq!(cache.pages.len(), capacity);
        assert_eq!(cache.evictions, 0);
        cache
            .insert(cache_key(capacity as u64), vec![1; INDEX_PAGE_BYTES])
            .unwrap();
        assert_eq!(cache.evictions, 1);
        assert!(!cache.pages.contains_key(&cache_key(0)));
        assert_cache_invariants(&cache);
        cache.clear();
        assert_cache_invariants(&cache);
    }

    #[test]
    fn cached_read_telemetry_preserves_errors_clear_and_overflow_semantics() {
        let mut cache = PageCache::default();
        let work = IndexReadStats {
            pages_read: 2,
            cache_hits: 3,
            fragments_visited: 5,
            result_bytes: 7,
        };
        assert_eq!(
            cache.observe_read(|_, stats| {
                *stats = work.clone();
                Ok(42)
            }),
            Ok(42)
        );
        assert_eq!(
            cache.observe_read::<()>(|_, stats| {
                *stats = work.clone();
                Err(StorageError::IntegrityFailure)
            }),
            Err(StorageError::IntegrityFailure)
        );
        let report = cache.read_telemetry().unwrap();
        assert_eq!(
            (report.completed_operations, report.failed_operations),
            (1, 1)
        );
        assert_eq!(
            report.work,
            IndexReadStats {
                pages_read: 4,
                cache_hits: 6,
                fragments_visited: 10,
                result_bytes: 14
            }
        );
        cache.clear();
        assert_eq!(cache.read_telemetry().unwrap(), report);
        for field in 0..6 {
            let mut overflowing = report.clone();
            match field {
                0 => overflowing.completed_operations = u64::MAX,
                1 => overflowing.failed_operations = u64::MAX,
                2 => overflowing.work.pages_read = u64::MAX,
                3 => overflowing.work.cache_hits = u64::MAX,
                4 => overflowing.work.fragments_visited = u64::MAX,
                _ => overflowing.work.result_bytes = u64::MAX,
            }
            let before = overflowing.clone();
            assert_eq!(overflowing.record(&work, field != 1), None);
            assert_eq!(overflowing, before, "overflow must not partially aggregate");
        }
        cache.read_telemetry.completed_operations = u64::MAX;
        assert_eq!(cache.observe_read(|_, _| Ok(99)), Ok(99));
        assert_eq!(cache.read_telemetry(), Err(StorageError::ResourceLimit));
        assert_eq!(
            cache.observe_read::<()>(|_, _| Err(StorageError::IntegrityFailure)),
            Err(StorageError::IntegrityFailure)
        );
        cache.clear();
        assert_eq!(cache.read_telemetry(), Err(StorageError::ResourceLimit));
    }

    #[test]
    fn page_cache_debug_redacts_keys_and_plaintext() {
        let mut cache = PageCache::new(MIN_INDEX_CACHE_BYTES).unwrap();
        let key = CacheKey {
            database: DatabaseId::from_bytes([0x11; 16]),
            namespace: NamespaceId::from_bytes([0x22; 16]),
            epoch: KeyEpoch::new(1).unwrap(),
            writer: WriterIncarnationId::from_bytes([0x33; 16]),
            revision: CommitRevision::new(1).unwrap(),
            index_profile: [0x44; 32],
            generation: 1,
            object_id: [0x55; 16],
            page: 0,
        };
        let mut plaintext = vec![0_u8; INDEX_PAGE_BYTES];
        plaintext[..6].copy_from_slice(b"SECRET");
        cache.insert(key, plaintext).unwrap();

        let debug = format!("{cache:?}");
        assert!(!debug.contains("CacheKey"));
        assert!(!debug.contains("CachedPage"));
        assert!(!debug.contains("83, 69, 67, 82, 69, 84"));
        assert!(debug.contains("page_count: 1"));
    }

    #[test]
    fn index_v1_plaintext_goldens_and_root_malformed_matrix() {
        let database = DatabaseId::from_bytes([0x11; 16]);
        let namespace = NamespaceId::from_bytes([0x22; 16]);
        let scope = NamespaceRef::new(database, namespace);
        let epoch = KeyEpoch::new(1).unwrap();
        let writer = WriterIncarnationId::from_bytes([0x33; 16]);
        let revision = CommitRevision::new(7).unwrap();
        let profile = [0x44; 32];
        let run_object = [0x55; 16];

        let mut page = new_page(revision, profile, 2, run_object, 0);
        page.extend_from_slice(&1_u32.to_be_bytes());
        page.extend_from_slice(&1_u32.to_be_bytes());
        page.extend_from_slice(&0_u32.to_be_bytes());
        page.extend_from_slice(&1_u32.to_be_bytes());
        page.extend_from_slice(b"k");
        page.extend_from_slice(b"v");
        let used = page.len();
        page[24..28].copy_from_slice(&1_u32.to_be_bytes());
        page[28..32].copy_from_slice(&u32::try_from(used).unwrap().to_be_bytes());
        page.resize(INDEX_PAGE_BYTES, 0);

        let mut run_digest = Sha256::new();
        run_digest.update(b"USTE-INDEX-RUN-V1\0");
        run_digest.update(namespace.as_bytes());
        run_digest.update(revision.get().to_be_bytes());
        run_digest.update(profile);
        run_digest.update([2]);
        run_digest.update(1_u32.to_be_bytes());
        run_digest.update(1_u64.to_be_bytes());
        run_digest.update(b"k");
        run_digest.update(b"v");
        let descriptor = IndexRunDescriptor {
            scope,
            revision,
            index_profile: profile,
            epoch,
            writer,
            family: 2,
            object_id: run_object,
            page_count: 1,
            entry_count: 1,
            logical_digest: run_digest.finalize().into(),
        };
        let root = RootManifest {
            input: IndexRootInput {
                scope,
                revision,
                certificate_digest: [0x66; 32],
                reducer_profile: [0x77; 32],
                logical_state_digest: [0x88; 32],
                index_profile: profile,
            },
            generation: 3,
            object_id: [0x99; 16],
            runs: vec![descriptor],
        }
        .encode()
        .unwrap();

        assert_eq!(
            hex(&Sha256::digest(&page)),
            "63644b344cb1d88f467deb8211c8cb623e94feff8ecf217941a3583c676274e6"
        );
        assert_eq!(
            hex(&Sha256::digest(root)),
            "25ac0ae741cdf825bbe56f3f1fb7edcbec167919a5bd2c63ca0df6a3291c773e"
        );
        let vectors = include_str!("../../../acceptance/r1/index-v1.tsv");
        assert!(
            vectors.contains("63644b344cb1d88f467deb8211c8cb623e94feff8ecf217941a3583c676274e6")
        );
        assert!(
            vectors.contains("25ac0ae741cdf825bbe56f3f1fb7edcbec167919a5bd2c63ca0df6a3291c773e")
        );

        for length in 0..ROOT_BYTES {
            assert!(RootManifest::decode(&root[..length], database, epoch, writer).is_err());
        }
        let mut unknown = root;
        unknown[4] = 2;
        assert!(RootManifest::decode(&unknown, database, epoch, writer).is_err());
    }
}
