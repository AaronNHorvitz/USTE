//! Journal-derived storage blob metadata, independent of transaction/principal schemas.
use super::*;

#[path = "journal_blob_metadata/admission.rs"]
mod admission;
#[path = "journal_blob_metadata/codec.rs"]
mod codec;
#[path = "journal_blob_metadata/rebuild.rs"]
mod rebuild;
use codec::*;

#[cfg(test)]
#[path = "journal_blob_metadata/tests.rs"]
mod tests;

/// SHA-256 of `USTE storage-blob-meta-v1`. No existing index profile is reinterpreted.
pub const BLOB_METADATA_PROFILE_V1: [u8; 32] = [
    0x11, 0x2c, 0x37, 0x59, 0x2f, 0xdf, 0x28, 0xc7, 0x9c, 0x14, 0x39, 0x01, 0x62, 0x88, 0x7c, 0x8d,
    0x9d, 0xd4, 0x11, 0xa7, 0x76, 0x57, 0x4d, 0xd1, 0xaf, 0x78, 0x66, 0x5e, 0xbc, 0x33, 0xe3, 0x7a,
];

const META: u8 = 1;
const BLOBS: u8 = 2;
const NAMESPACES: u8 = 3;
const INVENTORIES: u8 = 4;
const META_KEY: &[u8] = b"storage-blob-meta-v1";

/// Privileged cardinalities, not an authorized consumer quota response.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BlobMetadataCounts {
    pub blobs: u64,
    pub namespaces: u64,
    pub inventories: u64,
    pub reference_bindings: u64,
}

/// Independently admitted disk metadata; contains no complete blob/inventory/namespace map.
pub struct BlobMetadataBase {
    root: RecoveredIndexRoot,
    counts: BlobMetadataCounts,
}

impl fmt::Debug for BlobMetadataBase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BlobMetadataBase([REDACTED])")
    }
}

impl BlobMetadataBase {
    pub fn revision(&self) -> CommitRevision {
        self.root.revision()
    }
    pub fn counts(&self) -> BlobMetadataCounts {
        self.counts
    }
}

/// Bounds apply to each full family scan and lookup; the range allowance is shared across the
/// complete prefix. Range bytes exclude format-bounded inventories and index work, as documented
/// by `JournalRangeReadReport`. Certificate proof admission is separately explicit.
#[derive(Clone, Copy, Debug)]
pub struct BlobMetadataAdmissionLimits {
    pub maximum_blobs: u64,
    pub maximum_namespaces: u64,
    pub maximum_inventories: u64,
    pub maximum_reference_bindings: u64,
    pub run: IndexRunReadLimits,
    pub lookup: IndexGetLimits,
    pub certificates: CertificateAnchorReadLimits,
    pub maximum_journal_groups: u64,
    pub maximum_journal_encoded_bytes: u64,
}

/// Successful admission work, not filesystem/device I/O or payload authentication at journal open.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BlobMetadataAdmissionReport {
    pub journal: JournalRangeReadReport,
    pub run_pages: u64,
    pub run_entries: u64,
    pub lookup_pages: u64,
    pub lookup_fragments: u64,
    pub lookup_result_bytes: u64,
    pub certificate_bytes: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct BlobMetadataRebuildLimits {
    pub admission: BlobMetadataAdmissionLimits,
    pub merge: IndexRunMergeLimits,
    /// Sum of exact logical output bytes of all staged families, including unchanged rewrites
    /// required by index-v1's exact root/run revision binding.
    pub maximum_merge_output_bytes: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BlobMetadataRebuildReport {
    pub journal: JournalRangeReadReport,
    pub certificate_bytes: u64,
    pub merge_output_bytes: u64,
    pub merge_pages_read: u64,
    /// Only the lookup counters are populated here; independent admission is reported below.
    pub lookups: BlobMetadataAdmissionReport,
    pub admission: BlobMetadataAdmissionReport,
}

fn metadata_scope(database: DatabaseId) -> NamespaceRef {
    // A profile-separated internal carrier, not reservation/authorization of a user namespace.
    NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0; 16]))
}

fn checked_add(left: u64, right: u64) -> Result<u64, StorageError> {
    left.checked_add(right).ok_or(StorageError::ResourceLimit)
}

fn validate_counts(
    counts: BlobMetadataCounts,
    limits: BlobMetadataAdmissionLimits,
) -> Result<(), StorageError> {
    if limits.maximum_blobs > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64
        || limits.maximum_namespaces > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64
        || limits.maximum_inventories > CERTIFICATE_LOG_LIMIT / SMALL_ENVELOPE_BYTES - 1
        || limits.maximum_reference_bindings > MAX_BLOB_REFERENCE_BINDINGS_PER_JOURNAL
        || counts.blobs > limits.maximum_blobs
        || counts.namespaces > limits.maximum_namespaces
        || counts.inventories > limits.maximum_inventories
        || counts.reference_bindings > limits.maximum_reference_bindings
    {
        return Err(StorageError::ResourceLimit);
    }
    if counts.namespaces > counts.blobs
        || counts.blobs > counts.reference_bindings
        || counts.inventories > counts.reference_bindings
        || (counts.blobs == 0) != (counts.namespaces == 0)
        || (counts.blobs == 0) != (counts.inventories == 0)
        || (counts.blobs == 0 && counts.reference_bindings != 0)
    {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(())
}

fn state_digest(
    revision: CommitRevision,
    counts: BlobMetadataCounts,
    runs: &[IndexRunDescriptor],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"USTE-STORAGE-BLOB-STATE-V1\0");
    hash.update(revision.get().to_be_bytes());
    hash.update(encode_counts(counts));
    for run in runs {
        hash.update([run.family()]);
        hash.update(run.entry_count().to_be_bytes());
        hash.update(run.logical_digest());
    }
    hash.finalize().into()
}

fn has_family(root: &RecoveredIndexRoot, family: u8) -> bool {
    root.runs().any(|run| run.family() == family)
}
