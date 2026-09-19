//! Handle-relative storage capabilities, encrypted journal/recovery and deterministic fault models.
//!
//! T-13 includes the qualified x86_64 Linux adapter and format-1.0 commit-certificate journal. The
//! in-memory adapter and fault scripts remain verification tools rather than host durability claims.

#![forbid(unsafe_code)]

mod adapter;
pub mod blob;
pub mod checkpoint;
pub mod fault;
pub mod index;
pub mod journal;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub mod linux;
pub mod memory;
pub mod ordered_commitment;

pub use adapter::{
    AdapterError, AdapterErrorKind, Clock, ClockObservation, EntryName, EntryNameError,
    FileMetadata, FileSystem, MAX_ENTRY_NAME_BYTES, OwnershipFileSystem, RandomSource,
    RestartableFileSystem, read_exact_at, write_all_at,
};
pub use blob::{
    BLOB_CHUNK_BYTES, BlobId, BlobInventory, BlobReference, BlobUpload, BlobUploadToken,
    EMPTY_BLOB_INVENTORY_DIGEST, MAX_BLOB_BYTES, MAX_BLOB_REFERENCE_BINDINGS_PER_JOURNAL,
    MAX_BLOBS_PER_INVENTORY, MAX_COMMITTED_BLOBS_PER_JOURNAL, MAX_CONCURRENT_UPLOADS,
    MAX_NAMESPACE_BLOB_BYTES,
};
pub use checkpoint::{
    CHECKPOINT_CHUNK_BYTES, CheckpointInput, CheckpointStreamCandidate, CheckpointStreamInput,
    DurableCheckpoint, MAX_CHECKPOINT_BYTES, RecoveredCheckpoint,
};
pub use index::{
    DEFAULT_INDEX_CACHE_BYTES, DurableIndexRoot, INDEX_PAGE_BYTES, IndexDelta, IndexEntry,
    IndexGetLimits, IndexPredecessor, IndexPredecessorLimits, IndexReadStats, IndexReadTelemetry,
    IndexRootAnchor, IndexRootInput, IndexRunCursor, IndexRunDescriptor, IndexRunMergeLimits,
    IndexRunMergeReport, IndexRunReadLimits, IndexRunReadReport, IndexRunVisitor, IndexScan,
    IndexScanEntry, IndexScanLimits, IndexScrubReport, MAX_INDEX_CACHE_BYTES,
    MAX_INDEX_DELTA_LOGICAL_BYTES, MAX_INDEX_ENTRIES_PER_RUN, MAX_INDEX_GET_PAGE_VISITS,
    MAX_INDEX_KEY_BYTES, MAX_INDEX_PAGES_PER_RUN, MAX_INDEX_PREDECESSOR_PAGE_VISITS,
    MAX_INDEX_RESULT_BYTES, MAX_INDEX_RUN_LOGICAL_BYTES, MAX_INDEX_RUNS, MAX_INDEX_SCAN_RESULTS,
    MAX_INDEX_VALUE_BYTES, MIN_INDEX_CACHE_BYTES, MergedIndexRun, PageCache, RecoveredIndexRoot,
    StagedIndexRoot,
};
