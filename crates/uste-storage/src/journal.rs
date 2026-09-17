//! Encrypted commit-certificate journal and streaming recovery.
//!
//! This layer publishes opaque transaction groups. Transaction coordination, conflicts and
//! idempotency are deliberately owned by T-14 rather than inferred here.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};
use uste_crypto::{
    CryptoContext, CryptoError, CryptoObjectId, EncryptedEnvelope, EntropySource, FrameClass,
    KeyAdapter, KeyEpoch, KeyVault, MAX_PLAINTEXT_BYTES, ObjectRole, RecoveryEnvelope, Scope,
    WriterIncarnationId,
};
use uste_types::{CommitRevision, DatabaseId, NamespaceRef};

use crate::{
    AdapterError, AdapterErrorKind, EntryName, FileSystem, OwnershipFileSystem,
    blob::{
        BlobInventory, BlobReference, BlobUpload, BlobUploadToken,
        MAX_BLOB_REFERENCE_BINDINGS_PER_JOURNAL, MAX_COMMITTED_BLOBS_PER_JOURNAL,
        MAX_NAMESPACE_BLOB_BYTES, abort_upload, finish_upload, inventory_context, inventory_name,
        read_range, resume_upload, verify_reference, write_upload,
    },
    checkpoint::{
        self, CheckpointContext, CheckpointInput, DurableCheckpoint, RecoveredCheckpoint,
    },
    read_exact_at, write_all_at,
};

const STORAGE_MAJOR: u8 = 1;
const STORAGE_MINOR: u8 = 0;
const SMALL_ENVELOPE_BYTES: u64 = 4_161;
const MANIFEST_PLAINTEXT_BYTES: usize = 128;
const CERTIFICATE_HEADER_PLAINTEXT_BYTES: usize = 64;
const SEGMENT_HEADER_PLAINTEXT_BYTES: usize = 80;
const CERTIFICATE_PLAINTEXT_BYTES: usize = 192;
const MAX_KEY_ENVELOPE_BYTES: u64 = 64 * 1024;
const CERTIFICATE_LOG_LIMIT: u64 = 1024 * 1024 * 1024;
const STORAGE_PROFILE_LINUX_LOCAL_V1: u8 = 1;
const CRYPTO_SUITE_V1: u8 = 1;
const SMALL_FRAME_TAG: u8 = 1;
const RANDOM_ATTEMPTS: usize = 16;

/// Fixed journal segment bound recorded by `linux-local-v1` format 1.0.
pub const JOURNAL_SEGMENT_LIMIT: u64 = 256 * 1024 * 1024;

pub use crate::blob::EMPTY_BLOB_INVENTORY_DIGEST;

/// Bounded durable encoding used by a trusted key adapter.
pub trait DurableKeyEnvelope: Sized {
    fn encode_durable(&self) -> Result<Vec<u8>, CryptoError>;
    fn decode_durable(encoded: &[u8]) -> Result<Self, CryptoError>;
}

impl DurableKeyEnvelope for RecoveryEnvelope {
    fn encode_durable(&self) -> Result<Vec<u8>, CryptoError> {
        self.encode()
    }

    fn decode_durable(encoded: &[u8]) -> Result<Self, CryptoError> {
        Self::decode(encoded)
    }
}

fn entry(value: &str) -> EntryName {
    EntryName::new(value).expect("fixed storage entry name is valid")
}

/// Immutable database creation parameters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreationOptions {
    pub database: DatabaseId,
    pub final_name: EntryName,
}

/// One opaque transaction publication request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommitInput<'a> {
    pub encoded_group: &'a [u8],
    pub logical_event_digest: [u8; 32],
}

/// A commit acknowledged only after its certificate data sync succeeds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DurableCommit {
    pub revision: CommitRevision,
    pub certificate_digest: [u8; 32],
}

/// One authenticated committed group borrowed during the post-validation replay pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveredGroup<'a> {
    pub revision: CommitRevision,
    pub certificate_digest: [u8; 32],
    pub encoded_group: &'a [u8],
    pub blob_inventory_digest: [u8; 32],
    pub blob_inventory: Option<&'a BlobInventory>,
    pub logical_event_digest: [u8; 32],
}

/// Bounded facts established while opening and streaming the durable prefix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryReport {
    pub frontier: Option<CommitRevision>,
    pub certificate_digest: [u8; 32],
    pub repaired_certificate_tail_bytes: u64,
    pub ignored_uncommitted_journal_bytes: u64,
}

/// Stable, content-free storage error classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageError {
    Adapter(AdapterErrorKind),
    Crypto(CryptoError),
    IntegrityFailure,
    UnsupportedProfile,
    ResourceLimit,
    InvalidState,
    NeedsRecovery,
    RevisionExhausted,
}

impl StorageError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Adapter(_) => "USTE_STORAGE_ADAPTER",
            Self::Crypto(_) => "USTE_STORAGE_CRYPTO",
            Self::IntegrityFailure => "USTE_STORAGE_INTEGRITY_FAILURE",
            Self::UnsupportedProfile => "USTE_STORAGE_UNSUPPORTED_PROFILE",
            Self::ResourceLimit => "USTE_STORAGE_RESOURCE_LIMIT",
            Self::InvalidState => "USTE_STORAGE_INVALID_STATE",
            Self::NeedsRecovery => "USTE_STORAGE_NEEDS_RECOVERY",
            Self::RevisionExhausted => "USTE_STORAGE_REVISION_EXHAUSTED",
        }
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for StorageError {}

impl From<AdapterError> for StorageError {
    fn from(error: AdapterError) -> Self {
        Self::Adapter(error.kind())
    }
}

impl From<CryptoError> for StorageError {
    fn from(error: CryptoError) -> Self {
        Self::Crypto(error)
    }
}

/// One exclusively owned writer plus its authenticated durable frontier.
pub struct JournalStore<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    certificate_log_id: [u8; 16],
    database_directory: F::Directory,
    certificate_file: F::File,
    current_segment_id: [u8; 16],
    current_segment_file: F::File,
    current_segment_offset: u64,
    frontier: Option<CommitRevision>,
    previous_certificate_digest: [u8; 32],
    certificate_anchors: BTreeMap<CommitRevision, [u8; 32]>,
    committed_blob_inventories: BTreeSet<[u8; 32]>,
    committed_blobs: BTreeMap<(NamespaceRef, crate::blob::BlobId), BlobReference>,
    committed_blob_bytes: BTreeMap<NamespaceRef, u64>,
    committed_blob_reference_bindings: u64,
    poisoned: bool,
    vault: KeyVault<W, E>,
    identity_entropy: I,
    _ownership: F::OwnershipGuard,
}

impl<F, W, E, I> fmt::Debug for JournalStore<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JournalStore")
            .field("frontier", &self.frontier)
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}

impl<F, W, E, I> JournalStore<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Create, fully flush and atomically publish a database directory.
    pub fn create(
        filesystem: &mut F,
        options: CreationOptions,
        mut vault: KeyVault<W, E>,
        mut identity_entropy: I,
    ) -> Result<Self, StorageError> {
        let root = filesystem.root();
        let writer_bytes = random_nonzero_id(&mut identity_entropy)?;
        let certificate_log_id = random_nonzero_id(&mut identity_entropy)?;
        let initial_segment_id = random_nonzero_id(&mut identity_entropy)?;
        let writer = WriterIncarnationId::from_bytes(writer_bytes);
        let epoch = KeyEpoch::FIRST;

        let (temporary_name, temporary_directory) =
            create_temporary_directory(filesystem, &root, &mut identity_entropy)?;
        let lock_file = filesystem.create_new(&temporary_directory, &entry("LOCK"))?;
        filesystem.sync_all(&lock_file)?;
        let ownership = filesystem.try_lock_exclusive(&temporary_directory, &entry("LOCK"))?;

        let key_bytes = vault.wrapped().encode_durable()?;
        if key_bytes.is_empty()
            || u64::try_from(key_bytes.len()).map_err(|_| StorageError::ResourceLimit)?
                > MAX_KEY_ENVELOPE_BYTES
        {
            return Err(StorageError::ResourceLimit);
        }
        let key_file = filesystem.create_new(&temporary_directory, &entry("KEY"))?;
        write_all_at(filesystem, &key_file, 0, &key_bytes)?;
        filesystem.sync_all(&key_file)?;

        let manifest = Manifest {
            database: options.database,
            writer: writer_bytes,
            epoch,
            certificate_log_id,
            initial_segment_id,
        };
        let manifest_bytes = vault
            .encrypt(
                manifest_context(options.database, epoch),
                &manifest.encode(),
            )?
            .encode()?;
        require_small_envelope(&manifest_bytes)?;
        let manifest_file = filesystem.create_new(&temporary_directory, &entry("MANIFEST"))?;
        write_all_at(filesystem, &manifest_file, 0, &manifest_bytes)?;
        filesystem.sync_all(&manifest_file)?;

        let certificate_header =
            certificate_header(options.database, certificate_log_id, writer_bytes);
        let certificate_header_bytes = vault
            .encrypt(
                certificate_context(options.database, epoch, certificate_log_id, writer, 0),
                &certificate_header,
            )?
            .encode()?;
        require_small_envelope(&certificate_header_bytes)?;
        let certificate_file =
            filesystem.create_new(&temporary_directory, &entry("CERTIFICATES"))?;
        write_all_at(filesystem, &certificate_file, 0, &certificate_header_bytes)?;
        filesystem.sync_all(&certificate_file)?;

        let segment_header = SegmentHeader {
            database: options.database,
            segment_id: initial_segment_id,
            writer: writer_bytes,
            previous_segment_id: [0; 16],
            first_revision: 1,
        };
        let segment_header_bytes = vault
            .encrypt(
                segment_context(options.database, epoch, initial_segment_id, writer, 0),
                &segment_header.encode(),
            )?
            .encode()?;
        require_small_envelope(&segment_header_bytes)?;
        let initial_segment_file =
            filesystem.create_new(&temporary_directory, &segment_name(initial_segment_id)?)?;
        write_all_at(filesystem, &initial_segment_file, 0, &segment_header_bytes)?;
        filesystem.sync_all(&initial_segment_file)?;

        filesystem.sync_directory(&temporary_directory)?;
        filesystem.rename_no_replace(&root, &temporary_name, &root, &options.final_name)?;
        filesystem.sync_directory(&root)?;

        Ok(Self {
            database: options.database,
            epoch,
            writer,
            certificate_log_id,
            database_directory: temporary_directory,
            certificate_file,
            current_segment_id: initial_segment_id,
            current_segment_file: initial_segment_file,
            current_segment_offset: SMALL_ENVELOPE_BYTES,
            frontier: None,
            previous_certificate_digest: [0; 32],
            certificate_anchors: BTreeMap::new(),
            committed_blob_inventories: BTreeSet::new(),
            committed_blobs: BTreeMap::new(),
            committed_blob_bytes: BTreeMap::new(),
            committed_blob_reference_bindings: 0,
            poisoned: false,
            vault,
            identity_entropy,
            _ownership: ownership,
        })
    }

    /// Open one writer, authenticate the complete committed frontier, then stream exact groups.
    ///
    /// The visitor receives no group until the first validation pass succeeds. It must still build
    /// disposable state and publish it only after this method returns `Ok`, because its own error or
    /// a second-pass I/O failure can stop replay after earlier callbacks.
    pub fn open<A, V>(
        filesystem: &mut F,
        final_name: &EntryName,
        expected_database: DatabaseId,
        vault_entropy: E,
        identity_entropy: I,
        key_adapter: &mut A,
        mut visitor: V,
    ) -> Result<(Self, RecoveryReport), StorageError>
    where
        A: KeyAdapter<Envelope = W>,
        V: FnMut(RecoveredGroup<'_>) -> Result<(), StorageError>,
    {
        let root = filesystem.root();
        let database_directory = filesystem
            .open_directory(&root, final_name)
            .map_err(recovery_adapter_error)?;
        let ownership = filesystem
            .try_lock_exclusive(&database_directory, &entry("LOCK"))
            .map_err(recovery_adapter_error)?;

        let key_file = filesystem
            .open_existing(&database_directory, &entry("KEY"))
            .map_err(recovery_adapter_error)?;
        let key_len = filesystem.metadata(&key_file)?.len;
        if key_len == 0 || key_len > MAX_KEY_ENVELOPE_BYTES {
            return Err(StorageError::IntegrityFailure);
        }
        let key_bytes = read_bounded(filesystem, &key_file, 0, key_len)?;
        let wrapper = W::decode_durable(&key_bytes).map_err(recovery_crypto_error)?;
        let mut vault = KeyVault::from_locked(expected_database, wrapper, vault_entropy);
        vault.unlock(key_adapter)?;

        let manifest_file = filesystem
            .open_existing(&database_directory, &entry("MANIFEST"))
            .map_err(recovery_adapter_error)?;
        require_exact_len(filesystem, &manifest_file, SMALL_ENVELOPE_BYTES)?;
        let manifest_encoded = read_bounded(filesystem, &manifest_file, 0, SMALL_ENVELOPE_BYTES)?;
        let manifest_envelope =
            EncryptedEnvelope::decode(&manifest_encoded).map_err(recovery_crypto_error)?;
        let epoch = manifest_envelope.epoch();
        let manifest_plaintext = vault
            .decrypt(
                manifest_context(expected_database, epoch),
                &manifest_envelope,
            )
            .map_err(recovery_crypto_error)?;
        let manifest = Manifest::decode(manifest_plaintext.as_slice())?;
        if manifest.database != expected_database || manifest.epoch != epoch {
            return Err(StorageError::IntegrityFailure);
        }
        let writer = WriterIncarnationId::from_bytes(manifest.writer);

        let certificate_file = filesystem
            .open_existing(&database_directory, &entry("CERTIFICATES"))
            .map_err(recovery_adapter_error)?;
        let certificate_len = filesystem.metadata(&certificate_file)?.len;
        if !(SMALL_ENVELOPE_BYTES..=CERTIFICATE_LOG_LIMIT).contains(&certificate_len) {
            return Err(StorageError::IntegrityFailure);
        }
        let certificate_header_bytes =
            read_bounded(filesystem, &certificate_file, 0, SMALL_ENVELOPE_BYTES)?;
        verify_certificate_header(
            &vault,
            expected_database,
            epoch,
            manifest.certificate_log_id,
            writer,
            &certificate_header_bytes,
        )?;

        let initial_segment_file = filesystem
            .open_existing(
                &database_directory,
                &segment_name(manifest.initial_segment_id)?,
            )
            .map_err(recovery_adapter_error)?;
        let initial_header = verify_segment_header(
            filesystem,
            &vault,
            &initial_segment_file,
            expected_database,
            epoch,
            manifest.initial_segment_id,
            writer,
        )?;
        if initial_header.previous_segment_id != [0; 16] || initial_header.first_revision != 1 {
            return Err(StorageError::IntegrityFailure);
        }

        let certificate_payload_bytes = certificate_len - SMALL_ENVELOPE_BYTES;
        let complete_certificate_bytes =
            certificate_payload_bytes / SMALL_ENVELOPE_BYTES * SMALL_ENVELOPE_BYTES;
        let repaired_certificate_tail_bytes =
            certificate_payload_bytes - complete_certificate_bytes;
        let certificate_count = complete_certificate_bytes / SMALL_ENVELOPE_BYTES;

        // First pass authenticates the complete committed frontier without exposing logical state.
        // This prevents a later corrupt certificate from producing an observable valid-prefix
        // replay. The second bounded streaming pass invokes the visitor only after validation.
        let no_trusted_inventories = BTreeMap::new();
        let validated = scan_certificates(
            filesystem,
            &vault,
            &database_directory,
            &certificate_file,
            initial_segment_file.clone(),
            manifest,
            epoch,
            writer,
            certificate_count,
            &no_trusted_inventories,
            false,
            &mut |_group| Ok(()),
        )?;

        if repaired_certificate_tail_bytes != 0 {
            filesystem.set_len(
                &certificate_file,
                SMALL_ENVELOPE_BYTES + complete_certificate_bytes,
            )?;
        }
        // A previous certificate sync may have returned an ambiguous error while leaving a
        // complete authenticated certificate visible. Recovery adopts that exact frontier only
        // after re-synchronizing it, so a successful reopen cannot acknowledge volatile state.
        filesystem.sync_data(&certificate_file)?;

        let mut ignored_uncommitted_journal_bytes = 0_u64;
        for tail in &validated.uncommitted_tails {
            let file = filesystem
                .open_existing(&database_directory, &segment_name(tail.segment_id)?)
                .map_err(recovery_adapter_error)?;
            filesystem.set_len(&file, tail.committed_end)?;
            filesystem.sync_data(&file)?;
            ignored_uncommitted_journal_bytes = ignored_uncommitted_journal_bytes
                .checked_add(tail.extra_bytes)
                .ok_or(StorageError::ResourceLimit)?;
        }

        let replayed = scan_certificates(
            filesystem,
            &vault,
            &database_directory,
            &certificate_file,
            initial_segment_file,
            manifest,
            epoch,
            writer,
            certificate_count,
            &validated.verified_blob_inventories,
            true,
            &mut visitor,
        )?;
        if replayed.current_segment_id != validated.current_segment_id
            || replayed.current_segment_offset != validated.current_segment_offset
            || replayed.frontier != validated.frontier
            || replayed.previous_certificate_digest != validated.previous_certificate_digest
            || replayed.certificate_anchors != validated.certificate_anchors
            || replayed.committed_blob_inventories != validated.committed_blob_inventories
            || replayed.committed_blobs != validated.committed_blobs
            || replayed.committed_blob_bytes != validated.committed_blob_bytes
            || replayed.committed_blob_reference_bindings
                != validated.committed_blob_reference_bindings
        {
            return Err(StorageError::IntegrityFailure);
        }

        let report = RecoveryReport {
            frontier: replayed.frontier,
            certificate_digest: replayed.previous_certificate_digest,
            repaired_certificate_tail_bytes,
            ignored_uncommitted_journal_bytes,
        };
        Ok((
            Self {
                database: expected_database,
                epoch,
                writer,
                certificate_log_id: manifest.certificate_log_id,
                database_directory,
                certificate_file,
                current_segment_id: replayed.current_segment_id,
                current_segment_file: replayed.current_segment_file,
                current_segment_offset: replayed.current_segment_offset,
                frontier: replayed.frontier,
                previous_certificate_digest: replayed.previous_certificate_digest,
                certificate_anchors: replayed.certificate_anchors,
                committed_blob_inventories: replayed.committed_blob_inventories,
                committed_blobs: replayed.committed_blobs,
                committed_blob_bytes: replayed.committed_blob_bytes,
                committed_blob_reference_bindings: replayed.committed_blob_reference_bindings,
                poisoned: false,
                vault,
                identity_entropy,
                _ownership: ownership,
            },
            report,
        ))
    }

    /// Start a namespace-scoped resumable blob upload with random opaque identities.
    pub fn start_blob_upload(&mut self, scope: NamespaceRef) -> Result<BlobUpload, StorageError> {
        if scope.database() != self.database {
            return Err(StorageError::InvalidState);
        }
        let upload = random_nonzero_id(&mut self.identity_entropy)?;
        BlobUpload::new(
            BlobUploadToken::from_upload_id(scope, upload),
            self.epoch,
            self.writer,
        )
    }

    /// Rebuild a resumable upload from authenticated durable chunks after interruption.
    pub fn resume_blob_upload(
        &mut self,
        filesystem: &mut F,
        token: BlobUploadToken,
    ) -> Result<BlobUpload, StorageError> {
        resume_upload(
            filesystem,
            &self.database_directory,
            &mut self.vault,
            self.database,
            self.epoch,
            self.writer,
            token,
        )
    }

    /// Accept bytes into a bounded upload buffer, flushing complete encrypted chunks durably.
    pub fn write_blob_upload(
        &mut self,
        filesystem: &mut F,
        upload: &mut BlobUpload,
        input: &[u8],
    ) -> Result<(), StorageError> {
        write_upload(
            filesystem,
            &self.database_directory,
            &mut self.vault,
            self.database,
            self.epoch,
            self.writer,
            upload,
            input,
        )
    }

    /// Seal all chunks under immutable names and return their canonical content reference.
    pub fn finish_blob_upload(
        &mut self,
        filesystem: &mut F,
        upload: &mut BlobUpload,
    ) -> Result<BlobReference, StorageError> {
        finish_upload(
            filesystem,
            &self.database_directory,
            &mut self.vault,
            self.database,
            self.epoch,
            self.writer,
            upload,
        )
    }

    /// Remove the known durable staging chunks for an upload that has not begun finalization.
    pub fn abort_blob_upload(
        &mut self,
        filesystem: &mut F,
        upload: &mut BlobUpload,
    ) -> Result<(), StorageError> {
        abort_upload(
            filesystem,
            &self.database_directory,
            &mut self.vault,
            self.database,
            self.epoch,
            self.writer,
            upload,
        )
    }

    /// Read at most one bounded chunk-sized range from an immutable blob reference.
    pub fn read_blob_range(
        &self,
        filesystem: &mut F,
        reference: BlobReference,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, StorageError> {
        if self
            .committed_blobs
            .get(&(reference.scope(), reference.id()))
            != Some(&reference)
        {
            return Err(StorageError::InvalidState);
        }
        read_range(
            filesystem,
            &self.database_directory,
            &self.vault,
            self.epoch,
            self.writer,
            reference,
            offset,
            output,
        )
    }

    /// Publish one encrypted group followed by its authenticated commit certificate.
    pub fn append_group(
        &mut self,
        filesystem: &mut F,
        input: CommitInput<'_>,
    ) -> Result<DurableCommit, StorageError> {
        self.append_group_internal(filesystem, input, EMPTY_BLOB_INVENTORY_DIGEST)
    }

    /// Durably verify and bind a nonempty canonical blob inventory to the commit certificate.
    pub fn append_group_with_inventory(
        &mut self,
        filesystem: &mut F,
        input: CommitInput<'_>,
        inventory: &BlobInventory,
    ) -> Result<DurableCommit, StorageError> {
        if inventory.scope().database() != self.database {
            return Err(StorageError::InvalidState);
        }
        if inventory.is_empty() {
            return self.append_group(filesystem, input);
        }
        for reference in inventory.references() {
            if self
                .committed_blobs
                .get(&(reference.scope(), reference.id()))
                .is_some_and(|existing| existing != reference)
            {
                return Err(StorageError::IntegrityFailure);
            }
        }
        check_blob_capacity(
            &self.committed_blobs,
            &self.committed_blob_bytes,
            inventory.references(),
        )?;
        let added_bindings =
            u64::try_from(inventory.references().len()).map_err(|_| StorageError::ResourceLimit)?;
        let projected_bindings = self
            .committed_blob_reference_bindings
            .checked_add(added_bindings)
            .ok_or(StorageError::ResourceLimit)?;
        check_blob_reference_binding_limit(projected_bindings)?;
        self.publish_blob_inventory(filesystem, inventory)?;
        let durable = self.append_group_internal(filesystem, input, inventory.digest())?;
        for reference in inventory.references() {
            if register_committed_blob(
                &mut self.committed_blobs,
                &mut self.committed_blob_bytes,
                *reference,
            )
            .is_err()
            {
                self.poisoned = true;
                return Err(StorageError::NeedsRecovery);
            }
        }
        self.committed_blob_reference_bindings = projected_bindings;
        Ok(durable)
    }

    fn append_group_internal(
        &mut self,
        filesystem: &mut F,
        input: CommitInput<'_>,
        blob_inventory_digest: [u8; 32],
    ) -> Result<DurableCommit, StorageError> {
        if self.poisoned {
            return Err(StorageError::NeedsRecovery);
        }
        if input.encoded_group.len() > MAX_PLAINTEXT_BYTES {
            return Err(StorageError::ResourceLimit);
        }
        let revision = match self.frontier {
            None => CommitRevision::FIRST,
            Some(current) => current
                .checked_next()
                .map_err(|_| StorageError::RevisionExhausted)?,
        };
        let sequence = revision.get();
        let certificate_offset = SMALL_ENVELOPE_BYTES
            .checked_add(
                (sequence - 1)
                    .checked_mul(SMALL_ENVELOPE_BYTES)
                    .ok_or(StorageError::ResourceLimit)?,
            )
            .ok_or(StorageError::ResourceLimit)?;
        let certificate_end = certificate_offset
            .checked_add(SMALL_ENVELOPE_BYTES)
            .ok_or(StorageError::ResourceLimit)?;
        if certificate_end > CERTIFICATE_LOG_LIMIT {
            return Err(StorageError::ResourceLimit);
        }
        let mut encoded_group = self
            .vault
            .encrypt(
                segment_context(
                    self.database,
                    self.epoch,
                    self.current_segment_id,
                    self.writer,
                    sequence,
                ),
                input.encoded_group,
            )?
            .encode()?;
        let group_length =
            u64::try_from(encoded_group.len()).map_err(|_| StorageError::ResourceLimit)?;
        validate_group_encoded_length(group_length)?;

        let projected_end = self
            .current_segment_offset
            .checked_add(group_length)
            .ok_or(StorageError::ResourceLimit)?;
        if projected_end > JOURNAL_SEGMENT_LIMIT {
            self.poisoned = true;
            self.rollover(filesystem, sequence)?;
            encoded_group = self
                .vault
                .encrypt(
                    segment_context(
                        self.database,
                        self.epoch,
                        self.current_segment_id,
                        self.writer,
                        sequence,
                    ),
                    input.encoded_group,
                )?
                .encode()?;
        }
        let group_offset = self.current_segment_offset;
        self.poisoned = true;
        write_all_at(
            filesystem,
            &self.current_segment_file,
            group_offset,
            &encoded_group,
        )?;
        filesystem.sync_data(&self.current_segment_file)?;

        let certificate = Certificate {
            revision: sequence,
            previous_digest: self.previous_certificate_digest,
            segment_id: self.current_segment_id,
            group_sequence: sequence,
            group_offset,
            group_length,
            group_digest: sha256(&encoded_group),
            blob_inventory_digest,
            logical_event_digest: input.logical_event_digest,
        };
        let encoded_certificate = self
            .vault
            .encrypt(
                certificate_context(
                    self.database,
                    self.epoch,
                    self.certificate_log_id,
                    self.writer,
                    sequence,
                ),
                &certificate.encode(),
            )?
            .encode()?;
        require_small_envelope(&encoded_certificate)?;
        write_all_at(
            filesystem,
            &self.certificate_file,
            certificate_offset,
            &encoded_certificate,
        )?;
        filesystem.sync_data(&self.certificate_file)?;

        let certificate_digest = sha256(&encoded_certificate);
        self.current_segment_offset = group_offset + group_length;
        self.frontier = Some(revision);
        self.previous_certificate_digest = certificate_digest;
        self.certificate_anchors
            .insert(revision, certificate_digest);
        if blob_inventory_digest != EMPTY_BLOB_INVENTORY_DIGEST {
            self.committed_blob_inventories
                .insert(blob_inventory_digest);
        }
        self.poisoned = false;
        Ok(DurableCommit {
            revision,
            certificate_digest,
        })
    }

    fn publish_blob_inventory(
        &mut self,
        filesystem: &mut F,
        inventory: &BlobInventory,
    ) -> Result<(), StorageError> {
        for reference in inventory.references() {
            verify_reference(
                filesystem,
                &self.database_directory,
                &self.vault,
                self.database,
                self.epoch,
                self.writer,
                *reference,
            )?;
        }
        let digest = inventory.digest();
        let name = inventory_name(&self.vault, self.database, self.epoch, self.writer, digest)?;
        match filesystem.open_existing(&self.database_directory, &name) {
            Ok(file) => {
                if load_blob_inventory(
                    filesystem,
                    &self.vault,
                    &self.database_directory,
                    self.database,
                    self.epoch,
                    self.writer,
                    digest,
                    true,
                )
                .is_ok()
                {
                    filesystem.sync_all(&file)?;
                    filesystem.sync_directory(&self.database_directory)?;
                    return Ok(());
                }
                if self.committed_blob_inventories.contains(&digest) {
                    return Err(StorageError::IntegrityFailure);
                }
                filesystem.set_len(&file, 0)?;
                let encoded = self
                    .vault
                    .encrypt(
                        inventory_context(self.database, self.epoch, self.writer, digest),
                        inventory.encoded(),
                    )?
                    .encode()?;
                write_all_at(filesystem, &file, 0, &encoded)?;
                filesystem.set_len(
                    &file,
                    u64::try_from(encoded.len()).map_err(|_| StorageError::ResourceLimit)?,
                )?;
                filesystem.sync_all(&file)?;
            }
            Err(error) if error.kind() == AdapterErrorKind::NotFound => {
                let encoded = self
                    .vault
                    .encrypt(
                        inventory_context(self.database, self.epoch, self.writer, digest),
                        inventory.encoded(),
                    )?
                    .encode()?;
                let file = filesystem.create_new(&self.database_directory, &name)?;
                write_all_at(filesystem, &file, 0, &encoded)?;
                filesystem.set_len(
                    &file,
                    u64::try_from(encoded.len()).map_err(|_| StorageError::ResourceLimit)?,
                )?;
                filesystem.sync_all(&file)?;
            }
            Err(error) => return Err(error.into()),
        }
        filesystem.sync_directory(&self.database_directory)?;
        Ok(())
    }

    #[must_use]
    pub const fn frontier(&self) -> Option<CommitRevision> {
        self.frontier
    }

    /// Exact authenticated journal anchor required for publishing a cache at the current frontier.
    #[must_use]
    pub const fn checkpoint_anchor(&self) -> Option<(CommitRevision, [u8; 32])> {
        match self.frontier {
            Some(revision) => Some((revision, self.previous_certificate_digest)),
            None => None,
        }
    }

    /// Publish an optional encrypted checkpoint cache without changing journal authority.
    pub fn publish_checkpoint(
        &mut self,
        filesystem: &mut F,
        input: CheckpointInput<'_>,
    ) -> Result<DurableCheckpoint, StorageError> {
        if self.poisoned
            || self.frontier != Some(input.revision)
            || self.previous_certificate_digest != input.certificate_digest
            || input.scope.database() != self.database
        {
            return Err(StorageError::InvalidState);
        }
        checkpoint::publish(
            filesystem,
            CheckpointContext {
                database: self.database,
                epoch: self.epoch,
                writer: self.writer,
                directory: &self.database_directory,
            },
            &mut self.vault,
            &mut self.identity_entropy,
            input,
        )
    }

    /// Load up to two authenticated cache candidates, newest first. Invalid cache objects are
    /// omitted so callers can cold-replay the authoritative journal.
    #[must_use]
    pub fn load_checkpoints(
        &self,
        filesystem: &mut F,
        scope: NamespaceRef,
    ) -> Vec<RecoveredCheckpoint> {
        checkpoint::load(
            filesystem,
            CheckpointContext {
                database: self.database,
                epoch: self.epoch,
                writer: self.writer,
                directory: &self.database_directory,
            },
            &self.vault,
            scope,
        )
        .into_iter()
        .filter(|checkpoint| {
            self.certificate_anchors.get(&checkpoint.revision())
                == Some(checkpoint.certificate_digest())
        })
        .collect()
    }

    fn rollover(&mut self, filesystem: &mut F, first_revision: u64) -> Result<(), StorageError> {
        let next_segment_id = random_nonzero_id(&mut self.identity_entropy)?;
        if next_segment_id == self.current_segment_id {
            return Err(StorageError::Crypto(CryptoError::IntegrityFailure));
        }
        let header = SegmentHeader {
            database: self.database,
            segment_id: next_segment_id,
            writer: *self.writer.as_bytes(),
            previous_segment_id: self.current_segment_id,
            first_revision,
        };
        let encoded = self
            .vault
            .encrypt(
                segment_context(self.database, self.epoch, next_segment_id, self.writer, 0),
                &header.encode(),
            )?
            .encode()?;
        require_small_envelope(&encoded)?;
        let file =
            filesystem.create_new(&self.database_directory, &segment_name(next_segment_id)?)?;
        write_all_at(filesystem, &file, 0, &encoded)?;
        filesystem.sync_all(&file)?;
        filesystem.sync_directory(&self.database_directory)?;
        self.current_segment_id = next_segment_id;
        self.current_segment_file = file;
        self.current_segment_offset = SMALL_ENVELOPE_BYTES;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Manifest {
    database: DatabaseId,
    writer: [u8; 16],
    epoch: KeyEpoch,
    certificate_log_id: [u8; 16],
    initial_segment_id: [u8; 16],
}

impl Manifest {
    fn encode(self) -> [u8; MANIFEST_PLAINTEXT_BYTES] {
        let mut bytes = [0_u8; MANIFEST_PLAINTEXT_BYTES];
        bytes[..4].copy_from_slice(b"UMAN");
        bytes[4] = STORAGE_MAJOR;
        bytes[5] = STORAGE_MINOR;
        bytes[6] = 1;
        bytes[7] = 0;
        bytes[8..24].copy_from_slice(self.database.as_bytes());
        bytes[24..40].copy_from_slice(&self.writer);
        bytes[40..48].copy_from_slice(&self.epoch.get().to_be_bytes());
        bytes[48..64].copy_from_slice(&self.certificate_log_id);
        bytes[64..80].copy_from_slice(&self.initial_segment_id);
        bytes[80] = STORAGE_PROFILE_LINUX_LOCAL_V1;
        bytes[81] = CRYPTO_SUITE_V1;
        bytes[82] = SMALL_FRAME_TAG;
        bytes[84..92].copy_from_slice(&JOURNAL_SEGMENT_LIMIT.to_be_bytes());
        bytes[92..100].copy_from_slice(&SMALL_ENVELOPE_BYTES.to_be_bytes());
        bytes[100..108].copy_from_slice(
            &u64::try_from(MAX_PLAINTEXT_BYTES)
                .expect("crypto bound fits u64")
                .to_be_bytes(),
        );
        bytes
    }

    fn decode(bytes: &[u8]) -> Result<Self, StorageError> {
        if bytes.len() != MANIFEST_PLAINTEXT_BYTES
            || &bytes[..4] != b"UMAN"
            || bytes[4..8] != [STORAGE_MAJOR, STORAGE_MINOR, 1, 0]
            || bytes[80] != STORAGE_PROFILE_LINUX_LOCAL_V1
            || bytes[81] != CRYPTO_SUITE_V1
            || bytes[82] != SMALL_FRAME_TAG
            || bytes[83] != 0
            || bytes[108..].iter().any(|byte| *byte != 0)
        {
            return Err(StorageError::UnsupportedProfile);
        }
        if read_u64(bytes, 84)? != JOURNAL_SEGMENT_LIMIT
            || read_u64(bytes, 92)? != SMALL_ENVELOPE_BYTES
            || read_u64(bytes, 100)?
                != u64::try_from(MAX_PLAINTEXT_BYTES).map_err(|_| StorageError::ResourceLimit)?
        {
            return Err(StorageError::UnsupportedProfile);
        }
        let epoch =
            KeyEpoch::new(read_u64(bytes, 40)?).map_err(|_| StorageError::IntegrityFailure)?;
        let manifest = Self {
            database: DatabaseId::from_bytes(read_array(bytes, 8)?),
            writer: read_array(bytes, 24)?,
            epoch,
            certificate_log_id: read_array(bytes, 48)?,
            initial_segment_id: read_array(bytes, 64)?,
        };
        if manifest.writer == [0; 16]
            || manifest.certificate_log_id == [0; 16]
            || manifest.initial_segment_id == [0; 16]
        {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(manifest)
    }
}

#[derive(Clone, Copy)]
struct SegmentHeader {
    database: DatabaseId,
    segment_id: [u8; 16],
    writer: [u8; 16],
    previous_segment_id: [u8; 16],
    first_revision: u64,
}

impl SegmentHeader {
    fn encode(self) -> [u8; SEGMENT_HEADER_PLAINTEXT_BYTES] {
        let mut bytes = [0_u8; SEGMENT_HEADER_PLAINTEXT_BYTES];
        bytes[..4].copy_from_slice(b"USEG");
        bytes[4] = STORAGE_MAJOR;
        bytes[5] = STORAGE_MINOR;
        bytes[8..24].copy_from_slice(self.database.as_bytes());
        bytes[24..40].copy_from_slice(&self.segment_id);
        bytes[40..56].copy_from_slice(&self.writer);
        bytes[56..72].copy_from_slice(&self.previous_segment_id);
        bytes[72..80].copy_from_slice(&self.first_revision.to_be_bytes());
        bytes
    }

    fn decode(bytes: &[u8]) -> Result<Self, StorageError> {
        if bytes.len() != SEGMENT_HEADER_PLAINTEXT_BYTES
            || &bytes[..4] != b"USEG"
            || bytes[4] != STORAGE_MAJOR
            || bytes[5] != STORAGE_MINOR
            || bytes[6..8] != [0, 0]
        {
            return Err(StorageError::IntegrityFailure);
        }
        let header = Self {
            database: DatabaseId::from_bytes(read_array(bytes, 8)?),
            segment_id: read_array(bytes, 24)?,
            writer: read_array(bytes, 40)?,
            previous_segment_id: read_array(bytes, 56)?,
            first_revision: read_u64(bytes, 72)?,
        };
        if header.segment_id == [0; 16] || header.writer == [0; 16] || header.first_revision == 0 {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(header)
    }
}

#[derive(Clone, Copy)]
struct Certificate {
    revision: u64,
    previous_digest: [u8; 32],
    segment_id: [u8; 16],
    group_sequence: u64,
    group_offset: u64,
    group_length: u64,
    group_digest: [u8; 32],
    blob_inventory_digest: [u8; 32],
    logical_event_digest: [u8; 32],
}

impl Certificate {
    fn encode(self) -> [u8; CERTIFICATE_PLAINTEXT_BYTES] {
        let mut bytes = [0_u8; CERTIFICATE_PLAINTEXT_BYTES];
        bytes[..4].copy_from_slice(b"UCER");
        bytes[4] = STORAGE_MAJOR;
        bytes[5] = STORAGE_MINOR;
        bytes[8..16].copy_from_slice(&self.revision.to_be_bytes());
        bytes[16..48].copy_from_slice(&self.previous_digest);
        bytes[48..64].copy_from_slice(&self.segment_id);
        bytes[64..72].copy_from_slice(&self.group_sequence.to_be_bytes());
        bytes[72..80].copy_from_slice(&self.group_offset.to_be_bytes());
        bytes[80..88].copy_from_slice(&self.group_length.to_be_bytes());
        bytes[88..120].copy_from_slice(&self.group_digest);
        bytes[120..152].copy_from_slice(&self.blob_inventory_digest);
        bytes[152..184].copy_from_slice(&self.logical_event_digest);
        bytes
    }

    fn decode(bytes: &[u8]) -> Result<Self, StorageError> {
        if bytes.len() != CERTIFICATE_PLAINTEXT_BYTES
            || &bytes[..4] != b"UCER"
            || bytes[4] != STORAGE_MAJOR
            || bytes[5] != STORAGE_MINOR
            || bytes[6..8] != [0, 0]
            || bytes[184..].iter().any(|byte| *byte != 0)
        {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(Self {
            revision: read_u64(bytes, 8)?,
            previous_digest: read_array(bytes, 16)?,
            segment_id: read_array(bytes, 48)?,
            group_sequence: read_u64(bytes, 64)?,
            group_offset: read_u64(bytes, 72)?,
            group_length: read_u64(bytes, 80)?,
            group_digest: read_array(bytes, 88)?,
            blob_inventory_digest: read_array(bytes, 120)?,
            logical_event_digest: read_array(bytes, 152)?,
        })
    }
}

struct ScanState<H> {
    current_segment_id: [u8; 16],
    current_segment_file: H,
    current_segment_offset: u64,
    frontier: Option<CommitRevision>,
    previous_certificate_digest: [u8; 32],
    certificate_anchors: BTreeMap<CommitRevision, [u8; 32]>,
    committed_blob_inventories: BTreeSet<[u8; 32]>,
    committed_blobs: BTreeMap<(NamespaceRef, crate::blob::BlobId), BlobReference>,
    committed_blob_bytes: BTreeMap<NamespaceRef, u64>,
    committed_blob_reference_bindings: u64,
    verified_blob_inventories: BTreeMap<[u8; 32], u32>,
    uncommitted_tails: Vec<SegmentTail>,
}

struct SegmentTail {
    segment_id: [u8; 16],
    committed_end: u64,
    extra_bytes: u64,
}

fn check_unique_blob_limit(count: usize) -> Result<(), StorageError> {
    if count > MAX_COMMITTED_BLOBS_PER_JOURNAL {
        return Err(StorageError::ResourceLimit);
    }
    Ok(())
}

fn check_blob_reference_binding_limit(count: u64) -> Result<(), StorageError> {
    if count > MAX_BLOB_REFERENCE_BINDINGS_PER_JOURNAL {
        return Err(StorageError::ResourceLimit);
    }
    Ok(())
}

fn check_namespace_blob_byte_limit(bytes: u64) -> Result<(), StorageError> {
    if bytes > MAX_NAMESPACE_BLOB_BYTES {
        return Err(StorageError::ResourceLimit);
    }
    Ok(())
}

fn check_blob_capacity(
    committed_blobs: &BTreeMap<(NamespaceRef, crate::blob::BlobId), BlobReference>,
    committed_blob_bytes: &BTreeMap<NamespaceRef, u64>,
    references: &[BlobReference],
) -> Result<(), StorageError> {
    let mut projected_count = committed_blobs.len();
    let mut additions = BTreeMap::<NamespaceRef, u64>::new();
    let mut new_keys = BTreeSet::new();
    for reference in references {
        let key = (reference.scope(), reference.id());
        if let Some(existing) = committed_blobs.get(&key) {
            if existing != reference {
                return Err(StorageError::IntegrityFailure);
            }
            continue;
        }
        if !new_keys.insert(key) {
            return Err(StorageError::IntegrityFailure);
        }
        projected_count = projected_count
            .checked_add(1)
            .ok_or(StorageError::ResourceLimit)?;
        check_unique_blob_limit(projected_count)?;
        let namespace_addition = additions.entry(reference.scope()).or_default();
        *namespace_addition = namespace_addition
            .checked_add(reference.byte_len())
            .ok_or(StorageError::ResourceLimit)?;
    }
    for (scope, addition) in additions {
        let projected = committed_blob_bytes
            .get(&scope)
            .copied()
            .unwrap_or_default()
            .checked_add(addition)
            .ok_or(StorageError::ResourceLimit)?;
        check_namespace_blob_byte_limit(projected)?;
    }
    Ok(())
}

fn register_committed_blob(
    committed_blobs: &mut BTreeMap<(NamespaceRef, crate::blob::BlobId), BlobReference>,
    committed_blob_bytes: &mut BTreeMap<NamespaceRef, u64>,
    reference: BlobReference,
) -> Result<(), StorageError> {
    let key = (reference.scope(), reference.id());
    if let Some(existing) = committed_blobs.get(&key) {
        return if *existing == reference {
            Ok(())
        } else {
            Err(StorageError::IntegrityFailure)
        };
    }
    check_unique_blob_limit(
        committed_blobs
            .len()
            .checked_add(1)
            .ok_or(StorageError::ResourceLimit)?,
    )?;
    let current_bytes = committed_blob_bytes
        .get(&reference.scope())
        .copied()
        .unwrap_or_default();
    let projected_bytes = current_bytes
        .checked_add(reference.byte_len())
        .ok_or(StorageError::ResourceLimit)?;
    check_namespace_blob_byte_limit(projected_bytes)?;
    committed_blobs.insert(key, reference);
    committed_blob_bytes.insert(reference.scope(), projected_bytes);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn load_blob_inventory<F, W, E>(
    filesystem: &mut F,
    vault: &KeyVault<W, E>,
    database_directory: &F::Directory,
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    digest: [u8; 32],
    verify_blobs: bool,
) -> Result<BlobInventory, StorageError>
where
    F: FileSystem,
    E: EntropySource,
{
    if digest == EMPTY_BLOB_INVENTORY_DIGEST {
        return Err(StorageError::IntegrityFailure);
    }
    let file = filesystem
        .open_existing(
            database_directory,
            &inventory_name(vault, database, epoch, writer, digest)?,
        )
        .map_err(|_| StorageError::IntegrityFailure)?;
    let len = filesystem.metadata(&file)?.len;
    let maximum = u64::try_from(MAX_PLAINTEXT_BYTES)
        .map_err(|_| StorageError::ResourceLimit)?
        .checked_add(8 * 1024)
        .ok_or(StorageError::ResourceLimit)?;
    if len == 0 || len > maximum {
        return Err(StorageError::IntegrityFailure);
    }
    let encoded = read_bounded(filesystem, &file, 0, len)?;
    let envelope = EncryptedEnvelope::decode(&encoded).map_err(committed_crypto_error)?;
    let plaintext = vault
        .decrypt(
            inventory_context(database, epoch, writer, digest),
            &envelope,
        )
        .map_err(committed_crypto_error)?;
    let inventory = BlobInventory::decode(database, plaintext.as_slice())?;
    if inventory.digest() != digest {
        return Err(StorageError::IntegrityFailure);
    }
    if verify_blobs {
        for reference in inventory.references() {
            verify_reference(
                filesystem,
                database_directory,
                vault,
                database,
                epoch,
                writer,
                *reference,
            )?;
        }
    }
    Ok(inventory)
}

#[allow(clippy::too_many_arguments)]
fn scan_certificates<F, W, E, V>(
    filesystem: &mut F,
    vault: &KeyVault<W, E>,
    database_directory: &F::Directory,
    certificate_file: &F::File,
    initial_segment_file: F::File,
    manifest: Manifest,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    certificate_count: u64,
    trusted_blob_inventories: &BTreeMap<[u8; 32], u32>,
    invoke_visitor: bool,
    visitor: &mut V,
) -> Result<ScanState<F::File>, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    V: FnMut(RecoveredGroup<'_>) -> Result<(), StorageError>,
{
    let mut current_segment_id = manifest.initial_segment_id;
    let mut current_segment_file = initial_segment_file;
    let mut current_segment_offset = SMALL_ENVELOPE_BYTES;
    let mut previous_certificate_digest = [0_u8; 32];
    let mut certificate_anchors = BTreeMap::new();
    let mut frontier = None;
    let mut committed_blob_inventories = BTreeSet::new();
    let mut committed_blobs = BTreeMap::new();
    let mut committed_blob_bytes = BTreeMap::new();
    let mut committed_blob_reference_bindings = 0_u64;
    let mut verified_blob_inventories = BTreeMap::new();
    let mut uncommitted_tails = Vec::new();

    for sequence in 1..=certificate_count {
        let revision = CommitRevision::new(sequence).map_err(|_| StorageError::IntegrityFailure)?;
        let offset = SMALL_ENVELOPE_BYTES
            .checked_add(
                (sequence - 1)
                    .checked_mul(SMALL_ENVELOPE_BYTES)
                    .ok_or(StorageError::ResourceLimit)?,
            )
            .ok_or(StorageError::ResourceLimit)?;
        let encoded_certificate =
            read_bounded(filesystem, certificate_file, offset, SMALL_ENVELOPE_BYTES)?;
        let certificate = decode_certificate(
            vault,
            manifest.database,
            epoch,
            manifest.certificate_log_id,
            writer,
            revision,
            &encoded_certificate,
        )?;
        if certificate.revision != sequence
            || certificate.previous_digest != previous_certificate_digest
        {
            return Err(StorageError::IntegrityFailure);
        }

        if certificate.segment_id != current_segment_id {
            record_uncommitted_tail(
                filesystem,
                &current_segment_file,
                current_segment_id,
                current_segment_offset,
                &mut uncommitted_tails,
            )?;
            let next_file = filesystem
                .open_existing(database_directory, &segment_name(certificate.segment_id)?)
                .map_err(recovery_adapter_error)?;
            let next_header = verify_segment_header(
                filesystem,
                vault,
                &next_file,
                manifest.database,
                epoch,
                certificate.segment_id,
                writer,
            )?;
            if next_header.previous_segment_id != current_segment_id
                || next_header.first_revision != sequence
            {
                return Err(StorageError::IntegrityFailure);
            }
            current_segment_id = certificate.segment_id;
            current_segment_file = next_file;
            current_segment_offset = SMALL_ENVELOPE_BYTES;
        }
        if certificate.group_sequence != sequence
            || certificate.group_offset != current_segment_offset
        {
            return Err(StorageError::IntegrityFailure);
        }
        validate_group_encoded_length(certificate.group_length)?;
        let group_end = certificate
            .group_offset
            .checked_add(certificate.group_length)
            .ok_or(StorageError::IntegrityFailure)?;
        if group_end > JOURNAL_SEGMENT_LIMIT
            || filesystem.metadata(&current_segment_file)?.len < group_end
        {
            return Err(StorageError::IntegrityFailure);
        }
        let encoded_group = read_bounded(
            filesystem,
            &current_segment_file,
            certificate.group_offset,
            certificate.group_length,
        )?;
        if sha256(&encoded_group) != certificate.group_digest {
            return Err(StorageError::IntegrityFailure);
        }
        let group_envelope =
            EncryptedEnvelope::decode(&encoded_group).map_err(committed_crypto_error)?;
        let plaintext = vault
            .decrypt(
                segment_context(
                    manifest.database,
                    epoch,
                    current_segment_id,
                    writer,
                    sequence,
                ),
                &group_envelope,
            )
            .map_err(committed_crypto_error)?;
        if plaintext.as_slice().len() > MAX_PLAINTEXT_BYTES {
            return Err(StorageError::IntegrityFailure);
        }
        let blob_inventory: Option<BlobInventory> =
            if certificate.blob_inventory_digest == EMPTY_BLOB_INVENTORY_DIGEST {
                None
            } else {
                let digest = certificate.blob_inventory_digest;
                let trusted_count = trusted_blob_inventories
                    .get(&digest)
                    .copied()
                    .or_else(|| verified_blob_inventories.get(&digest).copied());
                let already_verified = trusted_count.is_some();
                let inventory = if !already_verified || invoke_visitor {
                    Some(load_blob_inventory(
                        filesystem,
                        vault,
                        database_directory,
                        manifest.database,
                        epoch,
                        writer,
                        digest,
                        !already_verified,
                    )?)
                } else {
                    None
                };
                if !already_verified {
                    let count = u32::try_from(
                        inventory
                            .as_ref()
                            .ok_or(StorageError::IntegrityFailure)?
                            .references()
                            .len(),
                    )
                    .map_err(|_| StorageError::ResourceLimit)?;
                    verified_blob_inventories.insert(digest, count);
                } else if inventory.as_ref().is_some_and(|value| {
                    usize::try_from(trusted_count.unwrap_or_default())
                        .ok()
                        .is_none_or(|count| count != value.references().len())
                }) {
                    return Err(StorageError::IntegrityFailure);
                }
                committed_blob_inventories.insert(digest);
                let reference_count = match inventory.as_ref() {
                    Some(inventory) => u64::try_from(inventory.references().len())
                        .map_err(|_| StorageError::ResourceLimit)?,
                    None => u64::from(trusted_count.ok_or(StorageError::IntegrityFailure)?),
                };
                committed_blob_reference_bindings = committed_blob_reference_bindings
                    .checked_add(reference_count)
                    .ok_or(StorageError::ResourceLimit)?;
                check_blob_reference_binding_limit(committed_blob_reference_bindings)?;
                if let Some(inventory) = inventory.as_ref() {
                    for reference in inventory.references() {
                        register_committed_blob(
                            &mut committed_blobs,
                            &mut committed_blob_bytes,
                            *reference,
                        )?;
                    }
                }
                inventory
            };
        let certificate_digest = sha256(&encoded_certificate);
        certificate_anchors.insert(revision, certificate_digest);
        if invoke_visitor {
            visitor(RecoveredGroup {
                revision,
                certificate_digest,
                encoded_group: plaintext.as_slice(),
                blob_inventory_digest: certificate.blob_inventory_digest,
                blob_inventory: blob_inventory.as_ref(),
                logical_event_digest: certificate.logical_event_digest,
            })?;
        }
        current_segment_offset = group_end;
        previous_certificate_digest = certificate_digest;
        frontier = Some(revision);
    }

    record_uncommitted_tail(
        filesystem,
        &current_segment_file,
        current_segment_id,
        current_segment_offset,
        &mut uncommitted_tails,
    )?;

    Ok(ScanState {
        current_segment_id,
        current_segment_file,
        current_segment_offset,
        frontier,
        previous_certificate_digest,
        certificate_anchors,
        committed_blob_inventories,
        committed_blobs,
        committed_blob_bytes,
        committed_blob_reference_bindings,
        verified_blob_inventories,
        uncommitted_tails,
    })
}

fn record_uncommitted_tail<F: FileSystem>(
    filesystem: &mut F,
    file: &F::File,
    segment_id: [u8; 16],
    committed_end: u64,
    tails: &mut Vec<SegmentTail>,
) -> Result<(), StorageError> {
    let file_len = filesystem.metadata(file)?.len;
    if file_len < committed_end || file_len > JOURNAL_SEGMENT_LIMIT {
        return Err(StorageError::IntegrityFailure);
    }
    if file_len > committed_end {
        tails
            .try_reserve(1)
            .map_err(|_| StorageError::ResourceLimit)?;
        tails.push(SegmentTail {
            segment_id,
            committed_end,
            extra_bytes: file_len - committed_end,
        });
    }
    Ok(())
}

fn manifest_context(database: DatabaseId, epoch: KeyEpoch) -> CryptoContext {
    CryptoContext::new(
        database,
        Scope::Database,
        epoch,
        ObjectRole::CreationManifest,
        CryptoObjectId::from_bytes([0; 16]),
        0,
        WriterIncarnationId::from_bytes([0; 16]),
        STORAGE_MAJOR,
        STORAGE_MINOR,
        FrameClass::Small4KiB,
    )
}

fn certificate_context(
    database: DatabaseId,
    epoch: KeyEpoch,
    certificate_log_id: [u8; 16],
    writer: WriterIncarnationId,
    sequence: u64,
) -> CryptoContext {
    CryptoContext::new(
        database,
        Scope::Database,
        epoch,
        ObjectRole::CommitCertificate,
        CryptoObjectId::from_bytes(certificate_log_id),
        sequence,
        writer,
        STORAGE_MAJOR,
        STORAGE_MINOR,
        FrameClass::Small4KiB,
    )
}

fn segment_context(
    database: DatabaseId,
    epoch: KeyEpoch,
    segment_id: [u8; 16],
    writer: WriterIncarnationId,
    sequence: u64,
) -> CryptoContext {
    CryptoContext::new(
        database,
        Scope::Database,
        epoch,
        ObjectRole::JournalGroup,
        CryptoObjectId::from_bytes(segment_id),
        sequence,
        writer,
        STORAGE_MAJOR,
        STORAGE_MINOR,
        FrameClass::Small4KiB,
    )
}

fn certificate_header(
    database: DatabaseId,
    log_id: [u8; 16],
    writer: [u8; 16],
) -> [u8; CERTIFICATE_HEADER_PLAINTEXT_BYTES] {
    let mut bytes = [0_u8; CERTIFICATE_HEADER_PLAINTEXT_BYTES];
    bytes[..4].copy_from_slice(b"UCLG");
    bytes[4] = STORAGE_MAJOR;
    bytes[5] = STORAGE_MINOR;
    bytes[8..24].copy_from_slice(database.as_bytes());
    bytes[24..40].copy_from_slice(&log_id);
    bytes[40..56].copy_from_slice(&writer);
    bytes
}

fn verify_certificate_header<W, E>(
    vault: &KeyVault<W, E>,
    database: DatabaseId,
    epoch: KeyEpoch,
    log_id: [u8; 16],
    writer: WriterIncarnationId,
    encoded: &[u8],
) -> Result<(), StorageError>
where
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    let envelope = EncryptedEnvelope::decode(encoded).map_err(committed_crypto_error)?;
    let plaintext = vault
        .decrypt(
            certificate_context(database, epoch, log_id, writer, 0),
            &envelope,
        )
        .map_err(committed_crypto_error)?;
    if plaintext.as_slice() != certificate_header(database, log_id, *writer.as_bytes()) {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(())
}

fn verify_segment_header<F, W, E>(
    filesystem: &mut F,
    vault: &KeyVault<W, E>,
    file: &F::File,
    database: DatabaseId,
    epoch: KeyEpoch,
    segment_id: [u8; 16],
    writer: WriterIncarnationId,
) -> Result<SegmentHeader, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    let file_len = filesystem.metadata(file)?.len;
    if !(SMALL_ENVELOPE_BYTES..=JOURNAL_SEGMENT_LIMIT).contains(&file_len) {
        return Err(StorageError::IntegrityFailure);
    }
    let encoded = read_bounded(filesystem, file, 0, SMALL_ENVELOPE_BYTES)?;
    let envelope = EncryptedEnvelope::decode(&encoded).map_err(committed_crypto_error)?;
    let plaintext = vault
        .decrypt(
            segment_context(database, epoch, segment_id, writer, 0),
            &envelope,
        )
        .map_err(committed_crypto_error)?;
    let header = SegmentHeader::decode(plaintext.as_slice())?;
    if header.database != database
        || header.segment_id != segment_id
        || header.writer != *writer.as_bytes()
    {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(header)
}

fn decode_certificate<W, E>(
    vault: &KeyVault<W, E>,
    database: DatabaseId,
    epoch: KeyEpoch,
    log_id: [u8; 16],
    writer: WriterIncarnationId,
    revision: CommitRevision,
    encoded: &[u8],
) -> Result<Certificate, StorageError>
where
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    let envelope = EncryptedEnvelope::decode(encoded).map_err(committed_crypto_error)?;
    let plaintext = vault
        .decrypt(
            certificate_context(database, epoch, log_id, writer, revision.get()),
            &envelope,
        )
        .map_err(committed_crypto_error)?;
    Certificate::decode(plaintext.as_slice())
}

fn create_temporary_directory<F: FileSystem, I: EntropySource>(
    filesystem: &mut F,
    root: &F::Directory,
    entropy: &mut I,
) -> Result<(EntryName, F::Directory), StorageError> {
    for _ in 0..RANDOM_ATTEMPTS {
        let random = random_nonzero_id(entropy)?;
        let candidate = entry(&format!(".uste-create-{}", hex_id(random)));
        match filesystem.create_directory(root, &candidate) {
            Ok(directory) => return Ok((candidate, directory)),
            Err(error) if error.kind() == AdapterErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err(StorageError::ResourceLimit)
}

fn random_nonzero_id(entropy: &mut impl EntropySource) -> Result<[u8; 16], StorageError> {
    for _ in 0..RANDOM_ATTEMPTS {
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

fn segment_name(id: [u8; 16]) -> Result<EntryName, StorageError> {
    EntryName::new(format!("j-{}", hex_id(id))).map_err(|_| StorageError::IntegrityFailure)
}

fn hex_id(id: [u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(32);
    for byte in id {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}

fn read_bounded<F: FileSystem>(
    filesystem: &mut F,
    file: &F::File,
    offset: u64,
    len: u64,
) -> Result<Vec<u8>, StorageError> {
    let len = usize::try_from(len).map_err(|_| StorageError::ResourceLimit)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(len)
        .map_err(|_| StorageError::ResourceLimit)?;
    output.resize(len, 0);
    read_exact_at(filesystem, file, offset, &mut output).map_err(recovery_adapter_error)?;
    Ok(output)
}

fn require_exact_len<F: FileSystem>(
    filesystem: &mut F,
    file: &F::File,
    expected: u64,
) -> Result<(), StorageError> {
    if filesystem.metadata(file)?.len != expected {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(())
}

fn require_small_envelope(encoded: &[u8]) -> Result<(), StorageError> {
    if u64::try_from(encoded.len()).map_err(|_| StorageError::ResourceLimit)?
        != SMALL_ENVELOPE_BYTES
    {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(())
}

fn validate_group_encoded_length(len: u64) -> Result<(), StorageError> {
    let minimum = SMALL_ENVELOPE_BYTES;
    let maximum = u64::try_from(MAX_PLAINTEXT_BYTES)
        .map_err(|_| StorageError::ResourceLimit)?
        .checked_add(SMALL_ENVELOPE_BYTES)
        .ok_or(StorageError::ResourceLimit)?;
    if len < minimum || len > maximum || !(len - 65).is_multiple_of(4 * 1024) {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(())
}

fn recovery_adapter_error(error: AdapterError) -> StorageError {
    match error.kind() {
        AdapterErrorKind::NotFound
        | AdapterErrorKind::UnexpectedEof
        | AdapterErrorKind::WrongEntryType => StorageError::IntegrityFailure,
        kind => StorageError::Adapter(kind),
    }
}

fn recovery_crypto_error(error: CryptoError) -> StorageError {
    match error {
        CryptoError::UnsupportedProfile => StorageError::UnsupportedProfile,
        CryptoError::ResourceLimit => StorageError::ResourceLimit,
        _ => StorageError::IntegrityFailure,
    }
}

fn committed_crypto_error(_error: CryptoError) -> StorageError {
    StorageError::IntegrityFailure
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, StorageError> {
    let end = offset
        .checked_add(8)
        .ok_or(StorageError::IntegrityFailure)?;
    let value = bytes
        .get(offset..end)
        .ok_or(StorageError::IntegrityFailure)?
        .try_into()
        .map_err(|_| StorageError::IntegrityFailure)?;
    Ok(u64::from_be_bytes(value))
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], StorageError> {
    let end = offset
        .checked_add(N)
        .ok_or(StorageError::IntegrityFailure)?;
    bytes
        .get(offset..end)
        .ok_or(StorageError::IntegrityFailure)?
        .try_into()
        .map_err(|_| StorageError::IntegrityFailure)
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use uste_crypto::{EntropyFailure, SecretKeyMaterial};

    use super::*;
    use crate::{
        fault::{FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation},
        memory::MemoryFileSystem,
    };

    const JOURNAL_VECTORS: &str = include_str!("../../../acceptance/r1/journal-v1.tsv");

    #[derive(Debug)]
    struct TestEnvelope([u8; 32]);

    impl DurableKeyEnvelope for TestEnvelope {
        fn encode_durable(&self) -> Result<Vec<u8>, CryptoError> {
            Ok(self.0.to_vec())
        }

        fn decode_durable(encoded: &[u8]) -> Result<Self, CryptoError> {
            let bytes = encoded
                .try_into()
                .map_err(|_| CryptoError::InvalidEnvelope)?;
            Ok(Self(bytes))
        }
    }

    #[derive(Debug, Default)]
    struct TestKeyAdapter;

    impl KeyAdapter for TestKeyAdapter {
        type Envelope = TestEnvelope;

        fn wrap(
            &mut self,
            _database: DatabaseId,
            key: &SecretKeyMaterial,
            _entropy: &mut dyn EntropySource,
        ) -> Result<Self::Envelope, CryptoError> {
            Ok(TestEnvelope(*key.expose_to_adapter()))
        }

        fn unwrap(
            &mut self,
            _database: DatabaseId,
            envelope: &Self::Envelope,
        ) -> Result<SecretKeyMaterial, CryptoError> {
            Ok(SecretKeyMaterial::from_adapter_bytes(envelope.0))
        }
    }

    #[derive(Debug)]
    struct CounterEntropy(u64);

    impl CounterEntropy {
        fn new(seed: u64) -> Self {
            Self(seed)
        }
    }

    impl EntropySource for CounterEntropy {
        fn fill(&mut self, output: &mut [u8]) -> Result<(), EntropyFailure> {
            self.0 = self.0.checked_add(1).ok_or(EntropyFailure)?;
            let mut block = self.0;
            for chunk in output.chunks_mut(8) {
                let bytes = block.to_be_bytes();
                chunk.copy_from_slice(&bytes[..chunk.len()]);
                block = block.checked_add(1).ok_or(EntropyFailure)?;
            }
            Ok(())
        }
    }

    fn create_vault(database: DatabaseId, seed: u64) -> KeyVault<TestEnvelope, CounterEntropy> {
        KeyVault::create(database, &mut TestKeyAdapter, CounterEntropy::new(seed)).unwrap()
    }

    fn digest_hex(bytes: &[u8]) -> String {
        let mut encoded = String::with_capacity(64);
        for byte in sha256(bytes) {
            write!(&mut encoded, "{byte:02x}").unwrap();
        }
        encoded
    }

    fn options(database: DatabaseId, name: &str) -> CreationOptions {
        CreationOptions {
            database,
            final_name: EntryName::new(name).unwrap(),
        }
    }

    #[test]
    fn encrypted_checkpoint_slots_round_trip_and_corruption_falls_back() {
        let database = DatabaseId::from_bytes([0xc1; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xc2; 16]));
        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "checkpoint-slots"),
            create_vault(database, 81_000),
            CounterEntropy::new(82_000),
        )
        .unwrap();
        let committed = store
            .append_group(
                &mut filesystem,
                CommitInput {
                    encoded_group: b"checkpoint anchor",
                    logical_event_digest: [0xc3; 32],
                },
            )
            .unwrap();
        let first_payload = (0..checkpoint::CHECKPOINT_CHUNK_BYTES + 17)
            .map(|index| u8::try_from(index % 251).unwrap())
            .collect::<Vec<_>>();
        let first = store
            .publish_checkpoint(
                &mut filesystem,
                CheckpointInput {
                    scope,
                    revision: committed.revision,
                    certificate_digest: committed.certificate_digest,
                    reducer_profile: [0xc4; 32],
                    logical_state_digest: [0xc5; 32],
                    payload: &first_payload,
                },
            )
            .unwrap();
        assert_eq!(first.generation, 1);
        let loaded = store.load_checkpoints(&mut filesystem, scope);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].payload(), first_payload);
        assert!(format!("{:?}", loaded[0]).contains("[REDACTED]"));

        let second_commit = store
            .append_group(
                &mut filesystem,
                CommitInput {
                    encoded_group: b"suffix after first checkpoint",
                    logical_event_digest: [0xc8; 32],
                },
            )
            .unwrap();
        let second_payload = b"newest complete cache";
        let second = store
            .publish_checkpoint(
                &mut filesystem,
                CheckpointInput {
                    scope,
                    revision: second_commit.revision,
                    certificate_digest: second_commit.certificate_digest,
                    reducer_profile: [0xc4; 32],
                    logical_state_digest: [0xc6; 32],
                    payload: second_payload,
                },
            )
            .unwrap();
        assert_eq!(second.generation, 2);
        let loaded = store.load_checkpoints(&mut filesystem, scope);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].payload(), second_payload);
        assert_eq!(loaded[1].payload(), first_payload);

        let saved_anchor = store
            .certificate_anchors
            .remove(&second_commit.revision)
            .unwrap();
        let future_rejected = store.load_checkpoints(&mut filesystem, scope);
        assert_eq!(future_rejected.len(), 1);
        assert_eq!(future_rejected[0].generation(), 1);
        let mut divergent_anchor = saved_anchor;
        divergent_anchor[0] ^= 0x80;
        store
            .certificate_anchors
            .insert(second_commit.revision, divergent_anchor);
        let divergent_rejected = store.load_checkpoints(&mut filesystem, scope);
        assert_eq!(divergent_rejected.len(), 1);
        assert_eq!(divergent_rejected[0].generation(), 1);
        store
            .certificate_anchors
            .insert(second_commit.revision, saved_anchor);

        filesystem
            .test_mutate_file(&store.database_directory, &entry("CHECKPOINT-B"), 32)
            .unwrap();
        let fallback = store.load_checkpoints(&mut filesystem, scope);
        assert_eq!(fallback.len(), 1);
        assert_eq!(fallback[0].generation(), 1);
        assert_eq!(fallback[0].payload(), first_payload);
        let other_scope =
            NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xc7; 16]));
        assert!(
            store
                .load_checkpoints(&mut filesystem, other_scope)
                .is_empty()
        );
    }

    #[test]
    fn checkpoint_publication_requires_the_exact_current_certificate_anchor() {
        let database = DatabaseId::from_bytes([0xd1; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xd2; 16]));
        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "checkpoint-anchor"),
            create_vault(database, 83_000),
            CounterEntropy::new(84_000),
        )
        .unwrap();
        let committed = store
            .append_group(
                &mut filesystem,
                CommitInput {
                    encoded_group: b"authoritative group",
                    logical_event_digest: [0xd3; 32],
                },
            )
            .unwrap();
        let mut wrong = committed.certificate_digest;
        wrong[0] ^= 0x80;
        assert_eq!(
            store.publish_checkpoint(
                &mut filesystem,
                CheckpointInput {
                    scope,
                    revision: committed.revision,
                    certificate_digest: wrong,
                    reducer_profile: [0xd4; 32],
                    logical_state_digest: [0xd5; 32],
                    payload: b"must not publish",
                },
            ),
            Err(StorageError::InvalidState)
        );
        assert!(store.load_checkpoints(&mut filesystem, scope).is_empty());
    }

    #[test]
    fn every_checkpoint_publication_boundary_recovers_an_old_or_exact_new_cache() {
        let database = DatabaseId::from_bytes([0xe1; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xe2; 16]));
        let name = "checkpoint-faults";
        let first_payload = b"durable first checkpoint";
        let second_payload = b"durable second checkpoint";
        let third_payload = b"replacement checkpoint after every injected crash";

        let mut base = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut base,
            options(database, name),
            create_vault(database, 85_000),
            CounterEntropy::new(86_000),
        )
        .unwrap();
        let first = store
            .append_group(
                &mut base,
                CommitInput {
                    encoded_group: b"checkpoint fault anchor one",
                    logical_event_digest: [0xe3; 32],
                },
            )
            .unwrap();
        store
            .publish_checkpoint(
                &mut base,
                CheckpointInput {
                    scope,
                    revision: first.revision,
                    certificate_digest: first.certificate_digest,
                    reducer_profile: [0xe4; 32],
                    logical_state_digest: [0xe5; 32],
                    payload: first_payload,
                },
            )
            .unwrap();
        let second = store
            .append_group(
                &mut base,
                CommitInput {
                    encoded_group: b"checkpoint fault anchor two",
                    logical_event_digest: [0xe6; 32],
                },
            )
            .unwrap();
        store
            .publish_checkpoint(
                &mut base,
                CheckpointInput {
                    scope,
                    revision: second.revision,
                    certificate_digest: second.certificate_digest,
                    reducer_profile: [0xe4; 32],
                    logical_state_digest: [0xe7; 32],
                    payload: second_payload,
                },
            )
            .unwrap();
        let third = store
            .append_group(
                &mut base,
                CommitInput {
                    encoded_group: b"checkpoint fault anchor three",
                    logical_event_digest: [0xe8; 32],
                },
            )
            .unwrap();
        drop(store);
        base.restart().unwrap();

        let boundaries = [
            (Operation::RemoveFile, 1_u64, "invalidate manifest"),
            (Operation::SyncDirectory, 1, "persist invalidation"),
            (Operation::RemoveFile, 2, "replace chunk"),
            (Operation::CreateNew, 1, "create chunk"),
            (Operation::WriteAt, 1, "write chunk"),
            (Operation::SetLen, 1, "size chunk"),
            (Operation::SyncAll, 1, "persist chunk"),
            (Operation::SyncDirectory, 2, "publish chunks"),
            (Operation::CreateNew, 2, "create manifest"),
            (Operation::WriteAt, 2, "write manifest object id"),
            (Operation::WriteAt, 3, "write encrypted manifest"),
            (Operation::SetLen, 2, "size manifest"),
            (Operation::SyncAll, 2, "persist manifest"),
            (Operation::SyncDirectory, 3, "publish manifest"),
        ];
        let mut case_seed = 90_000_u64;
        for (operation, occurrence, label) in boundaries {
            for action in [FaultAction::CrashBefore, FaultAction::CrashAfter] {
                case_seed += 10;
                let mut filesystem = FaultFileSystem::new(base.clone(), FaultPlan::default());
                let mut store = reopen_fault_store(&mut filesystem, database, name, case_seed);
                filesystem
                    .arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                assert_eq!(
                    store
                        .publish_checkpoint(
                            &mut filesystem,
                            CheckpointInput {
                                scope,
                                revision: third.revision,
                                certificate_digest: third.certificate_digest,
                                reducer_profile: [0xe4; 32],
                                logical_state_digest: [0xe9; 32],
                                payload: third_payload,
                            },
                        )
                        .unwrap_err(),
                    StorageError::Adapter(AdapterErrorKind::InjectedCrash),
                    "boundary={label}, action={action:?}"
                );
                assert_eq!(filesystem.pending_faults(), 0, "boundary={label}");
                drop(store);
                filesystem.restart().unwrap();

                let mut store = reopen_fault_store(&mut filesystem, database, name, case_seed + 2);
                let recovered = store.load_checkpoints(&mut filesystem, scope);
                assert!(
                    recovered.iter().any(|checkpoint| {
                        (checkpoint.revision() == second.revision
                            && checkpoint.payload() == second_payload)
                            || (checkpoint.revision() == third.revision
                                && checkpoint.payload() == third_payload)
                    }),
                    "boundary={label}, action={action:?}"
                );
                store
                    .publish_checkpoint(
                        &mut filesystem,
                        CheckpointInput {
                            scope,
                            revision: third.revision,
                            certificate_digest: third.certificate_digest,
                            reducer_profile: [0xe4; 32],
                            logical_state_digest: [0xe9; 32],
                            payload: third_payload,
                        },
                    )
                    .unwrap();
                let retried = store.load_checkpoints(&mut filesystem, scope);
                assert_eq!(retried[0].revision(), third.revision, "boundary={label}");
                assert_eq!(retried[0].payload(), third_payload, "boundary={label}");
            }
        }
    }

    #[test]
    fn missing_corrupt_or_swapped_checkpoint_chunks_fall_back_to_the_other_slot() {
        let database = DatabaseId::from_bytes([0xf1; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xf2; 16]));
        let name = "checkpoint-chunk-damage";
        let first_payload = vec![0x31; checkpoint::CHECKPOINT_CHUNK_BYTES + 23];
        let second_payload = vec![0x32; checkpoint::CHECKPOINT_CHUNK_BYTES + 31];
        let mut base = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut base,
            options(database, name),
            create_vault(database, 100_000),
            CounterEntropy::new(101_000),
        )
        .unwrap();
        for (index, payload) in [&first_payload, &second_payload].into_iter().enumerate() {
            let committed = store
                .append_group(
                    &mut base,
                    CommitInput {
                        encoded_group: if index == 0 {
                            b"chunk damage anchor one"
                        } else {
                            b"chunk damage anchor two"
                        },
                        logical_event_digest: [u8::try_from(0xf3 + index).unwrap(); 32],
                    },
                )
                .unwrap();
            store
                .publish_checkpoint(
                    &mut base,
                    CheckpointInput {
                        scope,
                        revision: committed.revision,
                        certificate_digest: committed.certificate_digest,
                        reducer_profile: [0xf5; 32],
                        logical_state_digest: [u8::try_from(0xf6 + index).unwrap(); 32],
                        payload,
                    },
                )
                .unwrap();
        }
        drop(store);
        base.restart().unwrap();

        for damage in 0..3 {
            let mut filesystem = base.clone();
            let (store, _) = JournalStore::open(
                &mut filesystem,
                &entry(name),
                database,
                CounterEntropy::new(102_000 + damage),
                CounterEntropy::new(103_000 + damage),
                &mut TestKeyAdapter,
                |_group| Ok(()),
            )
            .unwrap();
            match damage {
                0 => filesystem
                    .test_remove_entry(&store.database_directory, &entry("CHECKPOINT-B-000"))
                    .unwrap(),
                1 => filesystem
                    .test_mutate_file(&store.database_directory, &entry("CHECKPOINT-B-001"), 32)
                    .unwrap(),
                2 => filesystem
                    .test_swap_file_contents(
                        &store.database_directory,
                        &entry("CHECKPOINT-B-000"),
                        &entry("CHECKPOINT-B-001"),
                    )
                    .unwrap(),
                _ => unreachable!(),
            }
            let recovered = store.load_checkpoints(&mut filesystem, scope);
            assert_eq!(recovered.len(), 1, "damage={damage}");
            assert_eq!(recovered[0].generation(), 1, "damage={damage}");
            assert_eq!(recovered[0].payload(), first_payload, "damage={damage}");
        }
    }

    type FaultStore = JournalStore<
        FaultFileSystem<MemoryFileSystem>,
        TestEnvelope,
        CounterEntropy,
        CounterEntropy,
    >;

    fn reopen_fault_store(
        filesystem: &mut FaultFileSystem<MemoryFileSystem>,
        database: DatabaseId,
        name: &str,
        seed: u64,
    ) -> FaultStore {
        JournalStore::open(
            filesystem,
            &entry(name),
            database,
            CounterEntropy::new(seed),
            CounterEntropy::new(seed + 1),
            &mut TestKeyAdapter,
            |_group| Ok(()),
        )
        .unwrap()
        .0
    }

    fn commit_and_assert_blob(
        store: &mut FaultStore,
        filesystem: &mut FaultFileSystem<MemoryFileSystem>,
        scope: NamespaceRef,
        reference: BlobReference,
        expected: &[u8],
    ) {
        let inventory = BlobInventory::new(scope, [reference]).unwrap();
        store
            .append_group_with_inventory(
                filesystem,
                CommitInput {
                    encoded_group: b"fault-qualified blob",
                    logical_event_digest: [0xac; 32],
                },
                &inventory,
            )
            .unwrap();
        let mut output = Vec::with_capacity(expected.len());
        let mut scratch = vec![0_u8; crate::blob::BLOB_CHUNK_BYTES];
        let mut offset = 0_u64;
        while output.len() < expected.len() {
            let count = store
                .read_blob_range(filesystem, reference, offset, &mut scratch)
                .unwrap();
            assert_ne!(count, 0);
            output.extend_from_slice(&scratch[..count]);
            offset += u64::try_from(count).unwrap();
        }
        assert_eq!(output, expected);
    }

    #[test]
    fn literal_journal_plaintext_goldens_pin_format_one() {
        assert_eq!(ObjectRole::CreationManifest as u8, 9);
        let database = DatabaseId::from_bytes([0x11; 16]);
        let writer = [0x22; 16];
        let log = [0x33; 16];
        let segment = [0x44; 16];
        let manifest = Manifest {
            database,
            writer,
            epoch: KeyEpoch::FIRST,
            certificate_log_id: log,
            initial_segment_id: segment,
        };
        let segment_header = SegmentHeader {
            database,
            segment_id: segment,
            writer,
            previous_segment_id: [0; 16],
            first_revision: 1,
        };
        let certificate = Certificate {
            revision: 1,
            previous_digest: [0; 32],
            segment_id: segment,
            group_sequence: 1,
            group_offset: SMALL_ENVELOPE_BYTES,
            group_length: SMALL_ENVELOPE_BYTES,
            group_digest: [0x55; 32],
            blob_inventory_digest: EMPTY_BLOB_INVENTORY_DIGEST,
            logical_event_digest: [0x66; 32],
        };
        let cases = [
            (
                "manifest_golden",
                digest_hex(&manifest.encode()),
                "4d3b616f0351d702c8853a087ab7deae9d558b1aae19a849cf9a7b0b88cf90ab",
            ),
            (
                "certificate_header_golden",
                digest_hex(&certificate_header(database, log, writer)),
                "841a4ec5ececde9170da70e547a8c88d3df4c3af3223c3bfb627a7875e3d4efb",
            ),
            (
                "segment_header_golden",
                digest_hex(&segment_header.encode()),
                "5c1ad57f9d277db5bad0a0d53891457f43f5114a0b0f865100eb9be383e2c56d",
            ),
            (
                "certificate_golden",
                digest_hex(&certificate.encode()),
                "f64efba83369d15027262e6361b5b9c2d520742e53427a1f6f693f4d416fcaa9",
            ),
        ];
        for (case, actual, expected) in cases {
            assert_eq!(actual, expected);
            assert!(
                JOURNAL_VECTORS
                    .lines()
                    .any(|line| line.starts_with(case) && line.ends_with(expected))
            );
        }
        assert!(
            JOURNAL_VECTORS
                .lines()
                .any(|line| { line == "key_envelope_bound\tKEY\tencoded_length\t1_through_65536" })
        );
    }

    #[test]
    fn exact_groups_replay_in_order_and_ownership_is_exclusive() {
        let database = DatabaseId::from_bytes([0x31; 16]);
        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "world"),
            create_vault(database, 10),
            CounterEntropy::new(100),
        )
        .unwrap();
        let first = store
            .append_group(
                &mut filesystem,
                CommitInput {
                    encoded_group: b"first exact transaction bytes",
                    logical_event_digest: [0x41; 32],
                },
            )
            .unwrap();
        let second = store
            .append_group(
                &mut filesystem,
                CommitInput {
                    encoded_group: &[0, 1, 2, 0xff, 0, 3],
                    logical_event_digest: [0x42; 32],
                },
            )
            .unwrap();
        assert_eq!(first.revision, CommitRevision::FIRST);
        assert_eq!(second.revision.get(), 2);

        let busy = JournalStore::open(
            &mut filesystem,
            &entry("world"),
            database,
            CounterEntropy::new(20),
            CounterEntropy::new(200),
            &mut TestKeyAdapter,
            |_group| Ok(()),
        )
        .unwrap_err();
        assert_eq!(
            busy,
            StorageError::Adapter(AdapterErrorKind::OwnershipConflict)
        );

        drop(store);
        filesystem.restart().unwrap();
        let mut replayed = Vec::new();
        let (store, report) = JournalStore::open(
            &mut filesystem,
            &entry("world"),
            database,
            CounterEntropy::new(30),
            CounterEntropy::new(300),
            &mut TestKeyAdapter,
            |group| {
                replayed.push((group.revision.get(), group.encoded_group.to_vec()));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(report.frontier.map(CommitRevision::get), Some(2));
        assert_eq!(report.repaired_certificate_tail_bytes, 0);
        assert_eq!(report.ignored_uncommitted_journal_bytes, 0);
        assert_eq!(
            replayed,
            vec![
                (1, b"first exact transaction bytes".to_vec()),
                (2, vec![0, 1, 2, 0xff, 0, 3]),
            ]
        );
        assert_eq!(store.frontier().map(CommitRevision::get), Some(2));
    }

    #[test]
    fn crash_after_certificate_sync_recovers_new_frontier() {
        let database = DatabaseId::from_bytes([0x51; 16]);
        let plan = FaultPlan::new([FaultPoint {
            operation: Operation::SyncData,
            occurrence: 2,
            action: FaultAction::CrashAfter,
        }])
        .unwrap();
        let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "crash-after"),
            create_vault(database, 40),
            CounterEntropy::new(400),
        )
        .unwrap();
        assert_eq!(
            store
                .append_group(
                    &mut filesystem,
                    CommitInput {
                        encoded_group: b"response was lost",
                        logical_event_digest: [0x52; 32],
                    },
                )
                .unwrap_err(),
            StorageError::Adapter(AdapterErrorKind::InjectedCrash)
        );
        drop(store);
        filesystem.restart().unwrap();

        let mut replayed = Vec::new();
        let (_store, report) = JournalStore::open(
            &mut filesystem,
            &entry("crash-after"),
            database,
            CounterEntropy::new(50),
            CounterEntropy::new(500),
            &mut TestKeyAdapter,
            |group| {
                replayed.push((group.revision.get(), group.encoded_group.to_vec()));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(report.frontier, Some(CommitRevision::FIRST));
        assert_eq!(replayed, vec![(1, b"response was lost".to_vec())]);
    }

    #[test]
    fn failed_certificate_sync_recovers_previous_frontier_and_discards_group() {
        let database = DatabaseId::from_bytes([0x61; 16]);
        let plan = FaultPlan::new([FaultPoint {
            operation: Operation::SyncData,
            occurrence: 2,
            action: FaultAction::Error(AdapterErrorKind::Io),
        }])
        .unwrap();
        let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "sync-failure"),
            create_vault(database, 60),
            CounterEntropy::new(600),
        )
        .unwrap();
        assert_eq!(
            store
                .append_group(
                    &mut filesystem,
                    CommitInput {
                        encoded_group: b"not certified",
                        logical_event_digest: [0x62; 32],
                    },
                )
                .unwrap_err(),
            StorageError::Adapter(AdapterErrorKind::Io)
        );
        assert_eq!(
            store
                .append_group(
                    &mut filesystem,
                    CommitInput {
                        encoded_group: b"must not continue",
                        logical_event_digest: [0x63; 32],
                    },
                )
                .unwrap_err(),
            StorageError::NeedsRecovery
        );
        drop(store);
        filesystem.restart().unwrap();

        let mut replayed = Vec::new();
        let (_store, report) = JournalStore::open(
            &mut filesystem,
            &entry("sync-failure"),
            database,
            CounterEntropy::new(70),
            CounterEntropy::new(700),
            &mut TestKeyAdapter,
            |group| {
                replayed.push((group.revision.get(), group.encoded_group.to_vec()));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(report.frontier, None);
        assert!(report.ignored_uncommitted_journal_bytes > 0);
        assert!(replayed.is_empty());
    }

    #[test]
    fn incomplete_certificate_tail_is_repaired_but_complete_corruption_is_hard() {
        let database = DatabaseId::from_bytes([0x71; 16]);
        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "certificate-tail"),
            create_vault(database, 80),
            CounterEntropy::new(800),
        )
        .unwrap();
        store
            .append_group(
                &mut filesystem,
                CommitInput {
                    encoded_group: b"certified",
                    logical_event_digest: [0x72; 32],
                },
            )
            .unwrap();
        drop(store);
        let root = filesystem.root();
        let database_directory = filesystem
            .open_directory(&root, &entry("certificate-tail"))
            .unwrap();
        filesystem
            .test_append_file(&database_directory, &entry("CERTIFICATES"), &[0xa5; 37])
            .unwrap();

        let mut replayed = Vec::new();
        let (store, report) = JournalStore::open(
            &mut filesystem,
            &entry("certificate-tail"),
            database,
            CounterEntropy::new(90),
            CounterEntropy::new(900),
            &mut TestKeyAdapter,
            |group| {
                replayed.push(group.encoded_group.to_vec());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(report.repaired_certificate_tail_bytes, 37);
        assert_eq!(replayed, vec![b"certified".to_vec()]);
        drop(store);

        let root = filesystem.root();
        let database_directory = filesystem
            .open_directory(&root, &entry("certificate-tail"))
            .unwrap();
        filesystem
            .test_mutate_file(
                &database_directory,
                &entry("CERTIFICATES"),
                usize::try_from(SMALL_ENVELOPE_BYTES).unwrap() + 100,
            )
            .unwrap();
        assert_eq!(
            JournalStore::open(
                &mut filesystem,
                &entry("certificate-tail"),
                database,
                CounterEntropy::new(91),
                CounterEntropy::new(901),
                &mut TestKeyAdapter,
                |_group| Ok(()),
            )
            .unwrap_err(),
            StorageError::IntegrityFailure
        );
    }

    #[test]
    fn committed_group_corruption_never_rolls_back_to_a_valid_prefix() {
        let database = DatabaseId::from_bytes([0x81; 16]);
        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "group-corruption"),
            create_vault(database, 100),
            CounterEntropy::new(1_000),
        )
        .unwrap();
        store
            .append_group(
                &mut filesystem,
                CommitInput {
                    encoded_group: b"must not disappear",
                    logical_event_digest: [0x82; 32],
                },
            )
            .unwrap();
        drop(store);
        let root = filesystem.root();
        let database_directory = filesystem
            .open_directory(&root, &entry("group-corruption"))
            .unwrap();
        let segment = filesystem
            .test_child_names(&database_directory)
            .unwrap()
            .into_iter()
            .find(|name| name.as_str().starts_with("j-"))
            .unwrap();
        filesystem
            .test_mutate_file(
                &database_directory,
                &segment,
                usize::try_from(SMALL_ENVELOPE_BYTES).unwrap() + 75,
            )
            .unwrap();

        assert_eq!(
            JournalStore::open(
                &mut filesystem,
                &entry("group-corruption"),
                database,
                CounterEntropy::new(101),
                CounterEntropy::new(1_001),
                &mut TestKeyAdapter,
                |_group| Ok(()),
            )
            .unwrap_err(),
            StorageError::IntegrityFailure
        );
    }

    #[test]
    fn every_bootstrap_envelope_byte_is_authenticated() {
        let database = DatabaseId::from_bytes([0x86; 16]);
        let mut base = MemoryFileSystem::default();
        let store = JournalStore::create(
            &mut base,
            options(database, "bootstrap-corruption"),
            create_vault(database, 1_100),
            CounterEntropy::new(1_200),
        )
        .unwrap();
        drop(store);
        base.restart().unwrap();
        let root = base.root();
        let database_directory = base
            .open_directory(&root, &entry("bootstrap-corruption"))
            .unwrap();
        let segment = base
            .test_child_names(&database_directory)
            .unwrap()
            .into_iter()
            .find(|name| name.as_str().starts_with("j-"))
            .unwrap();

        for (name, bytes, allow_unsupported_profile) in [
            (entry("KEY"), 32_usize, false),
            (
                entry("MANIFEST"),
                usize::try_from(SMALL_ENVELOPE_BYTES).unwrap(),
                true,
            ),
            (
                entry("CERTIFICATES"),
                usize::try_from(SMALL_ENVELOPE_BYTES).unwrap(),
                false,
            ),
            (
                segment,
                usize::try_from(SMALL_ENVELOPE_BYTES).unwrap(),
                false,
            ),
        ] {
            for offset in 0..bytes {
                let mut filesystem = base.clone();
                let root = filesystem.root();
                let directory = filesystem
                    .open_directory(&root, &entry("bootstrap-corruption"))
                    .unwrap();
                filesystem
                    .test_mutate_file(&directory, &name, offset)
                    .unwrap();
                let error = JournalStore::open(
                    &mut filesystem,
                    &entry("bootstrap-corruption"),
                    database,
                    CounterEntropy::new(1_300),
                    CounterEntropy::new(1_400),
                    &mut TestKeyAdapter,
                    |_group| Ok(()),
                )
                .unwrap_err();
                assert!(
                    error == StorageError::IntegrityFailure
                        || (allow_unsupported_profile
                            && matches!(
                                error,
                                StorageError::UnsupportedProfile | StorageError::ResourceLimit
                            )),
                    "name={} offset={offset} error={error:?}",
                    name.as_str()
                );
            }
        }
    }

    #[test]
    fn missing_and_truncated_bootstrap_objects_fail_closed() {
        let database = DatabaseId::from_bytes([0x89; 16]);
        let mut base = MemoryFileSystem::default();
        let store = JournalStore::create(
            &mut base,
            options(database, "bootstrap-shape"),
            create_vault(database, 2_300),
            CounterEntropy::new(2_400),
        )
        .unwrap();
        drop(store);
        base.restart().unwrap();
        let root = base.root();
        let directory = base
            .open_directory(&root, &entry("bootstrap-shape"))
            .unwrap();
        let segment = base
            .test_child_names(&directory)
            .unwrap()
            .into_iter()
            .find(|name| name.as_str().starts_with("j-"))
            .unwrap();

        for name in [
            entry("LOCK"),
            entry("KEY"),
            entry("MANIFEST"),
            entry("CERTIFICATES"),
            segment.clone(),
        ] {
            let mut filesystem = base.clone();
            let root = filesystem.root();
            let directory = filesystem
                .open_directory(&root, &entry("bootstrap-shape"))
                .unwrap();
            filesystem.test_remove_entry(&directory, &name).unwrap();
            assert_eq!(
                JournalStore::open(
                    &mut filesystem,
                    &entry("bootstrap-shape"),
                    database,
                    CounterEntropy::new(2_500),
                    CounterEntropy::new(2_600),
                    &mut TestKeyAdapter,
                    |_group| Ok(()),
                )
                .unwrap_err(),
                StorageError::IntegrityFailure,
                "missing {}",
                name.as_str()
            );
        }

        for (name, lengths) in [
            (entry("KEY"), vec![0_u64, 1, 31, 33]),
            (
                entry("MANIFEST"),
                vec![0, 1, SMALL_ENVELOPE_BYTES - 1, SMALL_ENVELOPE_BYTES + 1],
            ),
            (entry("CERTIFICATES"), vec![0, 1, SMALL_ENVELOPE_BYTES - 1]),
            (segment, vec![0, 1, SMALL_ENVELOPE_BYTES - 1]),
        ] {
            for length in lengths {
                let mut filesystem = base.clone();
                let root = filesystem.root();
                let directory = filesystem
                    .open_directory(&root, &entry("bootstrap-shape"))
                    .unwrap();
                let file = filesystem.open_existing(&directory, &name).unwrap();
                filesystem.set_len(&file, length).unwrap();
                filesystem.sync_data(&file).unwrap();
                assert_eq!(
                    JournalStore::open(
                        &mut filesystem,
                        &entry("bootstrap-shape"),
                        database,
                        CounterEntropy::new(2_700),
                        CounterEntropy::new(2_800),
                        &mut TestKeyAdapter,
                        |_group| Ok(()),
                    )
                    .unwrap_err(),
                    StorageError::IntegrityFailure,
                    "name={} length={length}",
                    name.as_str()
                );
            }
        }
    }

    #[test]
    fn late_certificate_corruption_emits_no_callbacks_or_repairs() {
        let database = DatabaseId::from_bytes([0x87; 16]);
        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "late-corruption"),
            create_vault(database, 1_500),
            CounterEntropy::new(1_600),
        )
        .unwrap();
        for (bytes, digest) in [(b"first".as_slice(), [1; 32]), (b"second", [2; 32])] {
            store
                .append_group(
                    &mut filesystem,
                    CommitInput {
                        encoded_group: bytes,
                        logical_event_digest: digest,
                    },
                )
                .unwrap();
        }
        drop(store);
        let root = filesystem.root();
        let directory = filesystem
            .open_directory(&root, &entry("late-corruption"))
            .unwrap();
        filesystem
            .test_append_file(&directory, &entry("CERTIFICATES"), &[0xa5; 23])
            .unwrap();
        filesystem
            .test_mutate_file(
                &directory,
                &entry("CERTIFICATES"),
                usize::try_from(2 * SMALL_ENVELOPE_BYTES).unwrap() + 127,
            )
            .unwrap();
        let certificate_file = filesystem
            .open_existing(&directory, &entry("CERTIFICATES"))
            .unwrap();
        let length_before = filesystem.metadata(&certificate_file).unwrap().len;
        let mut callbacks = 0_u64;

        assert_eq!(
            JournalStore::open(
                &mut filesystem,
                &entry("late-corruption"),
                database,
                CounterEntropy::new(1_700),
                CounterEntropy::new(1_800),
                &mut TestKeyAdapter,
                |_group| {
                    callbacks += 1;
                    Ok(())
                },
            )
            .unwrap_err(),
            StorageError::IntegrityFailure
        );
        assert_eq!(callbacks, 0);
        assert_eq!(
            filesystem.metadata(&certificate_file).unwrap().len,
            length_before,
            "repair must be deferred until the full committed prefix validates"
        );
    }

    #[test]
    fn every_late_committed_byte_is_authenticated_before_replay() {
        let database = DatabaseId::from_bytes([0x88; 16]);
        let mut base = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut base,
            options(database, "committed-byte-corruption"),
            create_vault(database, 1_900),
            CounterEntropy::new(2_000),
        )
        .unwrap();
        for (bytes, digest) in [(b"first".as_slice(), [3; 32]), (b"second", [4; 32])] {
            store
                .append_group(
                    &mut base,
                    CommitInput {
                        encoded_group: bytes,
                        logical_event_digest: digest,
                    },
                )
                .unwrap();
        }
        drop(store);
        base.restart().unwrap();
        let root = base.root();
        let directory = base
            .open_directory(&root, &entry("committed-byte-corruption"))
            .unwrap();
        let segment = base
            .test_child_names(&directory)
            .unwrap()
            .into_iter()
            .find(|name| name.as_str().starts_with("j-"))
            .unwrap();
        let second_record_offset = usize::try_from(2 * SMALL_ENVELOPE_BYTES).unwrap();
        let encoded_record_bytes = usize::try_from(SMALL_ENVELOPE_BYTES).unwrap();

        for (name, start) in [
            (entry("CERTIFICATES"), second_record_offset),
            (segment, second_record_offset),
        ] {
            for relative_offset in 0..encoded_record_bytes {
                let mut filesystem = base.clone();
                let root = filesystem.root();
                let directory = filesystem
                    .open_directory(&root, &entry("committed-byte-corruption"))
                    .unwrap();
                filesystem
                    .test_mutate_file(&directory, &name, start + relative_offset)
                    .unwrap();
                let mut callbacks = 0_u64;
                assert_eq!(
                    JournalStore::open(
                        &mut filesystem,
                        &entry("committed-byte-corruption"),
                        database,
                        CounterEntropy::new(2_100),
                        CounterEntropy::new(2_200),
                        &mut TestKeyAdapter,
                        |_group| {
                            callbacks += 1;
                            Ok(())
                        },
                    )
                    .unwrap_err(),
                    StorageError::IntegrityFailure,
                    "name={} relative_offset={relative_offset}",
                    name.as_str()
                );
                assert_eq!(callbacks, 0);
            }
        }
    }

    #[test]
    fn every_streaming_chunk_boundary_resumes_at_an_exact_durable_offset() {
        let boundaries = [
            (Operation::OpenExisting, 1_u64),
            (Operation::OpenExisting, 2),
            (Operation::OpenExisting, 3),
            (Operation::OpenExisting, 4),
            (Operation::OpenExisting, 5),
            (Operation::OpenExisting, 6),
            (Operation::OpenExisting, 7),
            (Operation::OpenExisting, 8),
            (Operation::OpenExisting, 9),
            (Operation::OpenExisting, 10),
            (Operation::OpenExisting, 11),
            (Operation::CreateNew, 1),
            (Operation::WriteAt, 1),
            (Operation::SetLen, 1),
            (Operation::SyncAll, 1),
            (Operation::SyncDirectory, 1),
            (Operation::RenameNoReplace, 1),
            (Operation::SyncDirectory, 2),
            (Operation::CreateNew, 2),
            (Operation::WriteAt, 2),
            (Operation::SetLen, 2),
            (Operation::SyncAll, 2),
            (Operation::SyncDirectory, 3),
            (Operation::RenameNoReplace, 2),
            (Operation::SyncDirectory, 4),
        ];
        let actions = [
            FaultAction::Error(AdapterErrorKind::Io),
            FaultAction::Error(AdapterErrorKind::NoSpace),
            FaultAction::CrashBefore,
            FaultAction::CrashAfter,
        ];
        for (case, (operation, occurrence)) in boundaries.into_iter().enumerate() {
            for action in actions {
                if operation == Operation::OpenExisting && action == FaultAction::CrashAfter {
                    continue;
                }
                let database = DatabaseId::from_bytes([0xb3; 16]);
                let scope =
                    NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xb4; 16]));
                let mut filesystem =
                    FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
                let mut store = JournalStore::create(
                    &mut filesystem,
                    options(database, "stream-boundary"),
                    create_vault(database, 50_000 + u64::try_from(case).unwrap()),
                    CounterEntropy::new(51_000 + u64::try_from(case).unwrap()),
                )
                .unwrap();
                let bytes = vec![0x5d; crate::blob::BLOB_CHUNK_BYTES];
                let mut upload = store.start_blob_upload(scope).unwrap();
                let token = upload.token();
                filesystem
                    .arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                assert!(
                    store
                        .write_blob_upload(&mut filesystem, &mut upload, &bytes)
                        .is_err(),
                    "operation={operation:?} occurrence={occurrence} action={action:?}"
                );
                assert_eq!(filesystem.pending_faults(), 0);
                drop(upload);
                drop(store);
                if !matches!(action, FaultAction::Error(_)) {
                    filesystem.restart().unwrap();
                }
                let mut store =
                    reopen_fault_store(&mut filesystem, database, "stream-boundary", 52_000);
                let mut resumed = store.resume_blob_upload(&mut filesystem, token).unwrap();
                match resumed.durable_bytes() {
                    0 => store
                        .write_blob_upload(&mut filesystem, &mut resumed, &bytes)
                        .unwrap(),
                    durable if durable == u64::try_from(bytes.len()).unwrap() => {}
                    durable => panic!("non-exact durable offset {durable}"),
                }
                let reference = store
                    .finish_blob_upload(&mut filesystem, &mut resumed)
                    .unwrap();
                commit_and_assert_blob(&mut store, &mut filesystem, scope, reference, &bytes);
            }
        }
    }

    #[test]
    fn blob_chunk_short_write_retries_and_zero_progress_resumes_cleanly() {
        for (case, action) in [
            FaultAction::ShortWrite { maximum: 1 },
            FaultAction::ZeroProgress,
        ]
        .into_iter()
        .enumerate()
        {
            let database = DatabaseId::from_bytes([0xb2; 16]);
            let scope =
                NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xb1; 16]));
            let mut filesystem =
                FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
            let mut store = JournalStore::create(
                &mut filesystem,
                options(database, "chunk-progress"),
                create_vault(database, 49_000 + u64::try_from(case).unwrap()),
                CounterEntropy::new(49_100 + u64::try_from(case).unwrap()),
            )
            .unwrap();
            let bytes = vec![0x4c; crate::blob::BLOB_CHUNK_BYTES];
            let mut upload = store.start_blob_upload(scope).unwrap();
            let token = upload.token();
            filesystem
                .arm(
                    FaultPlan::new([FaultPoint {
                        operation: Operation::WriteAt,
                        occurrence: 1,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
            let result = store.write_blob_upload(&mut filesystem, &mut upload, &bytes);
            if action == FaultAction::ZeroProgress {
                assert_eq!(
                    result.unwrap_err(),
                    StorageError::Adapter(AdapterErrorKind::ZeroProgress)
                );
                drop(upload);
                drop(store);
                let mut reopened =
                    reopen_fault_store(&mut filesystem, database, "chunk-progress", 49_200);
                upload = reopened.resume_blob_upload(&mut filesystem, token).unwrap();
                assert_eq!(upload.durable_bytes(), 0);
                reopened
                    .write_blob_upload(&mut filesystem, &mut upload, &bytes)
                    .unwrap();
                store = reopened;
            } else {
                result.unwrap();
            }
            let reference = store
                .finish_blob_upload(&mut filesystem, &mut upload)
                .unwrap();
            commit_and_assert_blob(&mut store, &mut filesystem, scope, reference, &bytes);
        }
    }

    #[test]
    fn every_single_chunk_finish_boundary_is_exactly_resumable() {
        let mut boundaries = Vec::new();
        boundaries.extend((1..=16).map(|occurrence| (Operation::OpenExisting, occurrence)));
        boundaries.extend((1..=4).map(|occurrence| (Operation::CreateNew, occurrence)));
        boundaries.extend((1..=4).map(|occurrence| (Operation::WriteAt, occurrence)));
        boundaries.extend((1..=4).map(|occurrence| (Operation::SetLen, occurrence)));
        boundaries.extend((1..=4).map(|occurrence| (Operation::SyncAll, occurrence)));
        boundaries.extend((1..=9).map(|occurrence| (Operation::SyncDirectory, occurrence)));
        boundaries.extend((1..=5).map(|occurrence| (Operation::RenameNoReplace, occurrence)));
        let actions = [
            FaultAction::Error(AdapterErrorKind::Io),
            FaultAction::Error(AdapterErrorKind::NoSpace),
            FaultAction::CrashBefore,
            FaultAction::CrashAfter,
        ];
        for (case, (operation, occurrence)) in boundaries.into_iter().enumerate() {
            for action in actions {
                if operation == Operation::OpenExisting && action == FaultAction::CrashAfter {
                    continue;
                }
                let database = DatabaseId::from_bytes([0xb5; 16]);
                let scope =
                    NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xb6; 16]));
                let mut filesystem =
                    FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
                let mut store = JournalStore::create(
                    &mut filesystem,
                    options(database, "finish-boundary"),
                    create_vault(database, 53_000 + u64::try_from(case).unwrap()),
                    CounterEntropy::new(54_000 + u64::try_from(case).unwrap()),
                )
                .unwrap();
                let bytes = b"terminal chunk fault matrix";
                let mut upload = store.start_blob_upload(scope).unwrap();
                let token = upload.token();
                store
                    .write_blob_upload(&mut filesystem, &mut upload, bytes)
                    .unwrap();
                filesystem
                    .arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                assert!(
                    store
                        .finish_blob_upload(&mut filesystem, &mut upload)
                        .is_err(),
                    "operation={operation:?} occurrence={occurrence} action={action:?}"
                );
                assert_eq!(filesystem.pending_faults(), 0);
                drop(upload);
                drop(store);
                if !matches!(action, FaultAction::Error(_)) {
                    filesystem.restart().unwrap();
                }
                let mut store =
                    reopen_fault_store(&mut filesystem, database, "finish-boundary", 55_000);
                let mut resumed = store.resume_blob_upload(&mut filesystem, token).unwrap();
                match resumed.durable_bytes() {
                    0 => store
                        .write_blob_upload(&mut filesystem, &mut resumed, bytes)
                        .unwrap(),
                    durable if durable == u64::try_from(bytes.len()).unwrap() => {}
                    durable => panic!("non-exact terminal offset {durable}"),
                }
                let reference = store
                    .finish_blob_upload(&mut filesystem, &mut resumed)
                    .unwrap();
                commit_and_assert_blob(&mut store, &mut filesystem, scope, reference, bytes);
            }
        }
    }

    #[test]
    fn interrupted_multi_chunk_finish_recovers_mixed_final_and_staging_state() {
        for (case, action) in [
            FaultAction::Error(AdapterErrorKind::Io),
            FaultAction::Error(AdapterErrorKind::NoSpace),
            FaultAction::CrashBefore,
            FaultAction::CrashAfter,
        ]
        .into_iter()
        .enumerate()
        {
            let database = DatabaseId::from_bytes([0xcc; 16]);
            let scope =
                NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xcd; 16]));
            let mut filesystem =
                FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
            let mut store = JournalStore::create(
                &mut filesystem,
                options(database, "multi-finish"),
                create_vault(database, 55_100 + u64::try_from(case).unwrap()),
                CounterEntropy::new(55_200 + u64::try_from(case).unwrap()),
            )
            .unwrap();
            let bytes = vec![0x6d; crate::blob::BLOB_CHUNK_BYTES * 3];
            let mut upload = store.start_blob_upload(scope).unwrap();
            let token = upload.token();
            store
                .write_blob_upload(&mut filesystem, &mut upload, &bytes)
                .unwrap();
            filesystem
                .arm(
                    FaultPlan::new([FaultPoint {
                        operation: Operation::RenameNoReplace,
                        occurrence: 2,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
            assert!(
                store
                    .finish_blob_upload(&mut filesystem, &mut upload)
                    .is_err()
            );
            drop(upload);
            drop(store);
            if !matches!(action, FaultAction::Error(_)) {
                filesystem.restart().unwrap();
            }
            let mut store = reopen_fault_store(&mut filesystem, database, "multi-finish", 55_300);
            let mut resumed = store.resume_blob_upload(&mut filesystem, token).unwrap();
            assert_eq!(resumed.durable_bytes(), u64::try_from(bytes.len()).unwrap());
            let reference = store
                .finish_blob_upload(&mut filesystem, &mut resumed)
                .unwrap();
            commit_and_assert_blob(&mut store, &mut filesystem, scope, reference, &bytes);
        }
    }

    #[test]
    fn equal_bytes_remain_isolated_by_upload_and_namespace() {
        let database = DatabaseId::from_bytes([0xce; 16]);
        let first_scope =
            NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xcf; 16]));
        let second_scope =
            NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xd0; 16]));
        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "isolation"),
            create_vault(database, 55_400),
            CounterEntropy::new(55_500),
        )
        .unwrap();
        let bytes = b"identical source bytes remain namespace isolated";
        let mut references = Vec::new();
        for scope in [first_scope, first_scope, second_scope] {
            let mut upload = store.start_blob_upload(scope).unwrap();
            store
                .write_blob_upload(&mut filesystem, &mut upload, bytes)
                .unwrap();
            references.push(
                store
                    .finish_blob_upload(&mut filesystem, &mut upload)
                    .unwrap(),
            );
        }
        assert_ne!(references[0].id(), references[1].id());
        assert_ne!(references[0].id(), references[2].id());
        assert_eq!(
            references[0].content_digest(),
            references[1].content_digest()
        );
        assert_eq!(
            references[0].content_digest(),
            references[2].content_digest()
        );
        assert_eq!(references[0].scope(), first_scope);
        assert_eq!(references[2].scope(), second_scope);
    }

    #[test]
    fn database_upload_cap_survives_reopen_and_releases_on_terminal_state() {
        let database = DatabaseId::from_bytes([0xb7; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xb8; 16]));
        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "upload-cap"),
            create_vault(database, 56_000),
            CounterEntropy::new(57_000),
        )
        .unwrap();
        let mut uploads = Vec::new();
        for _ in 0..crate::blob::MAX_CONCURRENT_UPLOADS {
            uploads.push(store.start_blob_upload(scope).unwrap());
        }
        assert_eq!(
            store.start_blob_upload(scope).unwrap_err(),
            StorageError::ResourceLimit
        );
        drop(store);
        let (mut store, _) = JournalStore::open(
            &mut filesystem,
            &entry("upload-cap"),
            database,
            CounterEntropy::new(58_000),
            CounterEntropy::new(59_000),
            &mut TestKeyAdapter,
            |_group| Ok(()),
        )
        .unwrap();
        assert_eq!(
            store.start_blob_upload(scope).unwrap_err(),
            StorageError::ResourceLimit
        );
        uploads.pop();
        let mut replacement = store.start_blob_upload(scope).unwrap();
        store
            .finish_blob_upload(&mut filesystem, &mut replacement)
            .unwrap();
        let after_finish = store.start_blob_upload(scope).unwrap();
        drop(after_finish);
        drop(uploads);

        let mut finalized = store
            .resume_blob_upload(&mut filesystem, replacement.token())
            .unwrap();
        let mut uploads = Vec::new();
        for _ in 0..crate::blob::MAX_CONCURRENT_UPLOADS {
            uploads.push(store.start_blob_upload(scope).unwrap());
        }
        assert_eq!(
            store.start_blob_upload(scope).unwrap_err(),
            StorageError::ResourceLimit
        );
        assert_eq!(
            store
                .finish_blob_upload(&mut filesystem, &mut finalized)
                .unwrap()
                .byte_len(),
            0
        );
        // Resumed-final handles do not retain a live-upload lease.
        uploads.pop();
        let extra = store.start_blob_upload(scope).unwrap();
        drop(extra);
    }

    #[test]
    fn every_abort_boundary_preserves_terminal_intent_or_exact_resumability() {
        let mut boundaries = Vec::new();
        boundaries.extend((1..=12).map(|occurrence| (Operation::OpenExisting, occurrence)));
        boundaries.extend((1..=2).map(|occurrence| (Operation::CreateNew, occurrence)));
        boundaries.extend((1..=2).map(|occurrence| (Operation::WriteAt, occurrence)));
        boundaries.extend((1..=2).map(|occurrence| (Operation::SetLen, occurrence)));
        boundaries.extend((1..=2).map(|occurrence| (Operation::SyncAll, occurrence)));
        boundaries.extend((1..=5).map(|occurrence| (Operation::SyncDirectory, occurrence)));
        boundaries.extend((1..=2).map(|occurrence| (Operation::RenameNoReplace, occurrence)));
        boundaries.extend((1..=2).map(|occurrence| (Operation::RemoveFile, occurrence)));
        let actions = [
            FaultAction::Error(AdapterErrorKind::Io),
            FaultAction::Error(AdapterErrorKind::NoSpace),
            FaultAction::CrashBefore,
            FaultAction::CrashAfter,
        ];
        for (case, (operation, occurrence)) in boundaries.into_iter().enumerate() {
            for action in actions {
                if operation == Operation::OpenExisting && action == FaultAction::CrashAfter {
                    continue;
                }
                let database = DatabaseId::from_bytes([0xb9; 16]);
                let scope =
                    NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xba; 16]));
                let mut filesystem =
                    FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
                let mut store = JournalStore::create(
                    &mut filesystem,
                    options(database, "abort-boundary"),
                    create_vault(database, 60_000 + u64::try_from(case).unwrap()),
                    CounterEntropy::new(61_000 + u64::try_from(case).unwrap()),
                )
                .unwrap();
                let mut upload = store.start_blob_upload(scope).unwrap();
                let token = upload.token();
                store
                    .write_blob_upload(
                        &mut filesystem,
                        &mut upload,
                        &vec![0x6a; crate::blob::BLOB_CHUNK_BYTES],
                    )
                    .unwrap();
                filesystem
                    .arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                assert!(
                    store
                        .abort_blob_upload(&mut filesystem, &mut upload)
                        .is_err(),
                    "operation={operation:?} occurrence={occurrence} action={action:?}"
                );
                assert_eq!(filesystem.pending_faults(), 0);
                drop(upload);
                drop(store);
                if !matches!(action, FaultAction::Error(_)) {
                    filesystem.restart().unwrap();
                }
                let mut store =
                    reopen_fault_store(&mut filesystem, database, "abort-boundary", 62_000);
                match store.resume_blob_upload(&mut filesystem, token) {
                    Ok(mut resumed) => {
                        assert_eq!(
                            resumed.durable_bytes(),
                            u64::try_from(crate::blob::BLOB_CHUNK_BYTES).unwrap()
                        );
                        store
                            .abort_blob_upload(&mut filesystem, &mut resumed)
                            .unwrap();
                    }
                    Err(StorageError::InvalidState) => {}
                    Err(error) => panic!("unexpected abort recovery error {error:?}"),
                }
                drop(store);
                filesystem.restart().unwrap();
                let mut store =
                    reopen_fault_store(&mut filesystem, database, "abort-boundary", 63_000);
                assert_eq!(
                    store
                        .resume_blob_upload(&mut filesystem, token)
                        .unwrap_err(),
                    StorageError::InvalidState
                );
            }
        }
    }

    #[test]
    fn abort_cleanup_error_leaves_terminal_handle_and_is_idempotently_retryable() {
        let database = DatabaseId::from_bytes([0xaa; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xab; 16]));
        let mut filesystem =
            FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "abort-retry"),
            create_vault(database, 63_100),
            CounterEntropy::new(63_200),
        )
        .unwrap();
        let mut upload = store.start_blob_upload(scope).unwrap();
        let token = upload.token();
        store
            .write_blob_upload(
                &mut filesystem,
                &mut upload,
                &vec![0x3a; crate::blob::BLOB_CHUNK_BYTES],
            )
            .unwrap();
        filesystem
            .arm(
                FaultPlan::new([FaultPoint {
                    operation: Operation::RemoveFile,
                    occurrence: 1,
                    action: FaultAction::Error(AdapterErrorKind::Io),
                }])
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            store
                .abort_blob_upload(&mut filesystem, &mut upload)
                .unwrap_err(),
            StorageError::Adapter(AdapterErrorKind::Io)
        );
        assert_eq!(
            store
                .write_blob_upload(&mut filesystem, &mut upload, b"resurrection")
                .unwrap_err(),
            StorageError::InvalidState
        );
        store
            .abort_blob_upload(&mut filesystem, &mut upload)
            .unwrap();
        assert_eq!(
            store
                .resume_blob_upload(&mut filesystem, token)
                .unwrap_err(),
            StorageError::InvalidState
        );
    }

    #[test]
    fn every_inventory_and_commit_boundary_has_no_dangling_committed_reference() {
        let boundaries = [
            (Operation::CreateNew, 1_u64),
            (Operation::WriteAt, 1),
            (Operation::SetLen, 1),
            (Operation::SyncAll, 1),
            (Operation::SyncDirectory, 1),
            (Operation::WriteAt, 2),
            (Operation::SyncData, 1),
            (Operation::WriteAt, 3),
            (Operation::SyncData, 2),
        ];
        let actions = [
            FaultAction::Error(AdapterErrorKind::Io),
            FaultAction::Error(AdapterErrorKind::NoSpace),
            FaultAction::CrashBefore,
            FaultAction::CrashAfter,
        ];
        for (case, (operation, occurrence)) in boundaries.into_iter().enumerate() {
            for action in actions {
                let database = DatabaseId::from_bytes([0xbb; 16]);
                let scope =
                    NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xbc; 16]));
                let mut filesystem =
                    FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
                let mut store = JournalStore::create(
                    &mut filesystem,
                    options(database, "inventory-boundary"),
                    create_vault(database, 64_000 + u64::try_from(case).unwrap()),
                    CounterEntropy::new(65_000 + u64::try_from(case).unwrap()),
                )
                .unwrap();
                let bytes = b"inventory publication bytes";
                let mut upload = store.start_blob_upload(scope).unwrap();
                store
                    .write_blob_upload(&mut filesystem, &mut upload, bytes)
                    .unwrap();
                let reference = store
                    .finish_blob_upload(&mut filesystem, &mut upload)
                    .unwrap();
                let inventory = BlobInventory::new(scope, [reference]).unwrap();
                filesystem
                    .arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                assert!(
                    store
                        .append_group_with_inventory(
                            &mut filesystem,
                            CommitInput {
                                encoded_group: b"inventory fault matrix",
                                logical_event_digest: [0xbd; 32],
                            },
                            &inventory,
                        )
                        .is_err(),
                    "operation={operation:?} occurrence={occurrence} action={action:?}"
                );
                assert_eq!(filesystem.pending_faults(), 0);
                drop(store);
                if !matches!(action, FaultAction::Error(_)) {
                    filesystem.restart().unwrap();
                }
                let mut store =
                    reopen_fault_store(&mut filesystem, database, "inventory-boundary", 66_000);
                let expected_commit = operation == Operation::SyncData
                    && occurrence == 2
                    && matches!(action, FaultAction::Error(_) | FaultAction::CrashAfter);
                assert_eq!(
                    store.frontier().is_some(),
                    expected_commit,
                    "operation={operation:?} occurrence={occurrence} action={action:?}"
                );
                let mut output = [0_u8; 64];
                if expected_commit {
                    let count = store
                        .read_blob_range(&mut filesystem, reference, 0, &mut output)
                        .unwrap();
                    assert_eq!(&output[..count], bytes);
                } else {
                    assert_eq!(
                        store
                            .read_blob_range(&mut filesystem, reference, 0, &mut output)
                            .unwrap_err(),
                        StorageError::InvalidState
                    );
                    store
                        .append_group_with_inventory(
                            &mut filesystem,
                            CommitInput {
                                encoded_group: b"inventory retry after publication fault",
                                logical_event_digest: [0xbe; 32],
                            },
                            &inventory,
                        )
                        .unwrap();
                    let count = store
                        .read_blob_range(&mut filesystem, reference, 0, &mut output)
                        .unwrap();
                    assert_eq!(&output[..count], bytes);
                }
            }
        }
    }

    #[test]
    fn existing_inventory_retry_resynchronizes_before_new_certificate() {
        for (case, operation) in [Operation::SyncAll, Operation::SyncDirectory]
            .into_iter()
            .enumerate()
        {
            let database = DatabaseId::from_bytes([0xad; 16]);
            let scope =
                NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xae; 16]));
            let mut filesystem =
                FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
            let mut store = JournalStore::create(
                &mut filesystem,
                options(database, "inventory-resync"),
                create_vault(database, 66_100 + u64::try_from(case).unwrap()),
                CounterEntropy::new(66_200 + u64::try_from(case).unwrap()),
            )
            .unwrap();
            let mut upload = store.start_blob_upload(scope).unwrap();
            store
                .write_blob_upload(&mut filesystem, &mut upload, b"shared inventory")
                .unwrap();
            let reference = store
                .finish_blob_upload(&mut filesystem, &mut upload)
                .unwrap();
            let inventory = BlobInventory::new(scope, [reference]).unwrap();
            store
                .append_group_with_inventory(
                    &mut filesystem,
                    CommitInput {
                        encoded_group: b"first binding",
                        logical_event_digest: [0xaf; 32],
                    },
                    &inventory,
                )
                .unwrap();
            filesystem
                .arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence: 1,
                        action: FaultAction::Error(AdapterErrorKind::Io),
                    }])
                    .unwrap(),
                )
                .unwrap();
            assert_eq!(
                store
                    .append_group_with_inventory(
                        &mut filesystem,
                        CommitInput {
                            encoded_group: b"second binding",
                            logical_event_digest: [0xb0; 32],
                        },
                        &inventory,
                    )
                    .unwrap_err(),
                StorageError::Adapter(AdapterErrorKind::Io)
            );
            assert_eq!(store.frontier().map(CommitRevision::get), Some(1));
            store
                .append_group_with_inventory(
                    &mut filesystem,
                    CommitInput {
                        encoded_group: b"second binding",
                        logical_event_digest: [0xb0; 32],
                    },
                    &inventory,
                )
                .unwrap();
            assert_eq!(store.frontier().map(CommitRevision::get), Some(2));
        }
    }

    #[test]
    fn corrupt_acknowledged_staging_is_never_discarded_or_replaced() {
        let database = DatabaseId::from_bytes([0xbe; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xbf; 16]));
        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "staging-corruption"),
            create_vault(database, 67_000),
            CounterEntropy::new(68_000),
        )
        .unwrap();
        let mut upload = store.start_blob_upload(scope).unwrap();
        let token = upload.token();
        store
            .write_blob_upload(
                &mut filesystem,
                &mut upload,
                &vec![0x7a; crate::blob::BLOB_CHUNK_BYTES],
            )
            .unwrap();
        drop(upload);
        let directory = store.database_directory;
        let staging = filesystem
            .test_child_names(&directory)
            .unwrap()
            .into_iter()
            .find(|name| name.as_str().starts_with("u-"))
            .unwrap();
        filesystem
            .test_mutate_file(&directory, &staging, 100)
            .unwrap();
        assert_eq!(
            store
                .resume_blob_upload(&mut filesystem, token)
                .unwrap_err(),
            StorageError::IntegrityFailure
        );
        assert!(
            filesystem
                .test_child_names(&directory)
                .unwrap()
                .contains(&staging)
        );
    }

    #[test]
    fn missing_acknowledged_chunk_or_progress_cannot_shorten_an_upload() {
        for missing_chunk in 0..3_u32 {
            let database = DatabaseId::from_bytes([0xc3; 16]);
            let scope =
                NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xc4; 16]));
            let mut filesystem = MemoryFileSystem::default();
            let mut store = JournalStore::create(
                &mut filesystem,
                options(database, &format!("missing-chunk-{missing_chunk}")),
                create_vault(database, 68_100 + u64::from(missing_chunk)),
                CounterEntropy::new(68_200 + u64::from(missing_chunk)),
            )
            .unwrap();
            let mut upload = store.start_blob_upload(scope).unwrap();
            let token = upload.token();
            store
                .write_blob_upload(
                    &mut filesystem,
                    &mut upload,
                    &vec![0x7b; crate::blob::BLOB_CHUNK_BYTES * 3],
                )
                .unwrap();
            drop(upload);
            let directory = store.database_directory;
            filesystem
                .remove_file(
                    &directory,
                    &crate::blob::staging_chunk_name(token.upload_id(), missing_chunk).unwrap(),
                )
                .unwrap();
            filesystem.sync_directory(&directory).unwrap();
            assert_eq!(
                store
                    .resume_blob_upload(&mut filesystem, token)
                    .unwrap_err(),
                StorageError::IntegrityFailure
            );
        }

        let database = DatabaseId::from_bytes([0xc5; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xc6; 16]));
        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "missing-progress"),
            create_vault(database, 68_300),
            CounterEntropy::new(68_400),
        )
        .unwrap();
        let mut upload = store.start_blob_upload(scope).unwrap();
        let token = upload.token();
        store
            .write_blob_upload(
                &mut filesystem,
                &mut upload,
                &vec![0x7c; crate::blob::BLOB_CHUNK_BYTES],
            )
            .unwrap();
        drop(upload);
        let directory = store.database_directory;
        let progress = crate::blob::progress_name(token, 0).unwrap();
        filesystem.remove_file(&directory, &progress).unwrap();
        filesystem.sync_directory(&directory).unwrap();
        let resumed = store.resume_blob_upload(&mut filesystem, token).unwrap();
        assert_eq!(
            resumed.durable_bytes(),
            u64::try_from(crate::blob::BLOB_CHUNK_BYTES).unwrap()
        );
        assert!(
            filesystem
                .test_child_names(&directory)
                .unwrap()
                .contains(&progress)
        );
    }

    #[test]
    fn terminal_manifest_witness_prevents_single_object_resurrection() {
        let database = DatabaseId::from_bytes([0xc7; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xc8; 16]));

        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "terminal-final-witness"),
            create_vault(database, 68_500),
            CounterEntropy::new(68_600),
        )
        .unwrap();
        let mut upload = store.start_blob_upload(scope).unwrap();
        let token = upload.token();
        let expected = store
            .finish_blob_upload(&mut filesystem, &mut upload)
            .unwrap();
        let directory = store.database_directory;
        let primary = crate::blob::manifest_name(token, crate::blob::MANIFEST_FINAL).unwrap();
        let witness =
            crate::blob::manifest_witness_name(token, crate::blob::MANIFEST_FINAL).unwrap();
        filesystem.remove_file(&directory, &primary).unwrap();
        filesystem.sync_directory(&directory).unwrap();
        let mut resumed = store.resume_blob_upload(&mut filesystem, token).unwrap();
        assert_eq!(
            store
                .finish_blob_upload(&mut filesystem, &mut resumed)
                .unwrap(),
            expected
        );
        filesystem.remove_file(&directory, &witness).unwrap();
        filesystem.sync_directory(&directory).unwrap();
        assert!(store.resume_blob_upload(&mut filesystem, token).is_ok());
        let primary_file = filesystem.open_existing(&directory, &primary).unwrap();
        filesystem.set_len(&primary_file, 1).unwrap();
        filesystem.sync_all(&primary_file).unwrap();
        assert_eq!(
            store
                .resume_blob_upload(&mut filesystem, token)
                .unwrap_err(),
            StorageError::IntegrityFailure
        );

        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "terminal-abort-witness"),
            create_vault(database, 68_700),
            CounterEntropy::new(68_800),
        )
        .unwrap();
        let mut upload = store.start_blob_upload(scope).unwrap();
        let token = upload.token();
        store
            .abort_blob_upload(&mut filesystem, &mut upload)
            .unwrap();
        let directory = store.database_directory;
        let primary = crate::blob::manifest_name(token, crate::blob::MANIFEST_ABORTED).unwrap();
        let witness =
            crate::blob::manifest_witness_name(token, crate::blob::MANIFEST_ABORTED).unwrap();
        filesystem.remove_file(&directory, &primary).unwrap();
        filesystem.sync_directory(&directory).unwrap();
        assert_eq!(
            store
                .resume_blob_upload(&mut filesystem, token)
                .unwrap_err(),
            StorageError::InvalidState
        );
        filesystem.remove_file(&directory, &witness).unwrap();
        filesystem.sync_directory(&directory).unwrap();
        assert_eq!(
            store
                .resume_blob_upload(&mut filesystem, token)
                .unwrap_err(),
            StorageError::InvalidState
        );
        let primary_file = filesystem.open_existing(&directory, &primary).unwrap();
        filesystem.set_len(&primary_file, 1).unwrap();
        filesystem.sync_all(&primary_file).unwrap();
        assert_eq!(
            store
                .resume_blob_upload(&mut filesystem, token)
                .unwrap_err(),
            StorageError::IntegrityFailure
        );
    }

    #[test]
    fn foreign_upload_handles_are_rejected_before_io() {
        let database_a = DatabaseId::from_bytes([0xc9; 16]);
        let database_b = DatabaseId::from_bytes([0xca; 16]);
        let mut filesystem = MemoryFileSystem::default();
        let mut store_a = JournalStore::create(
            &mut filesystem,
            options(database_a, "owner-a"),
            create_vault(database_a, 68_900),
            CounterEntropy::new(69_000),
        )
        .unwrap();
        let mut store_b = JournalStore::create(
            &mut filesystem,
            options(database_b, "owner-b"),
            create_vault(database_b, 69_100),
            CounterEntropy::new(69_200),
        )
        .unwrap();
        let scope_b =
            NamespaceRef::new(database_b, uste_types::NamespaceId::from_bytes([0xcb; 16]));
        let mut upload = store_b.start_blob_upload(scope_b).unwrap();
        let before = filesystem
            .test_child_names(&store_a.database_directory)
            .unwrap();
        assert_eq!(
            store_a
                .write_blob_upload(&mut filesystem, &mut upload, b"foreign")
                .unwrap_err(),
            StorageError::InvalidState
        );
        assert_eq!(
            store_a
                .finish_blob_upload(&mut filesystem, &mut upload)
                .unwrap_err(),
            StorageError::InvalidState
        );
        assert_eq!(
            store_a
                .abort_blob_upload(&mut filesystem, &mut upload)
                .unwrap_err(),
            StorageError::InvalidState
        );
        assert_eq!(
            filesystem
                .test_child_names(&store_a.database_directory)
                .unwrap(),
            before
        );
    }

    #[test]
    fn profile_limits_are_inclusive_and_reject_one_more() {
        assert!(check_unique_blob_limit(MAX_COMMITTED_BLOBS_PER_JOURNAL).is_ok());
        assert_eq!(
            check_unique_blob_limit(MAX_COMMITTED_BLOBS_PER_JOURNAL + 1).unwrap_err(),
            StorageError::ResourceLimit
        );
        assert!(
            check_blob_reference_binding_limit(MAX_BLOB_REFERENCE_BINDINGS_PER_JOURNAL).is_ok()
        );
        assert_eq!(
            check_blob_reference_binding_limit(MAX_BLOB_REFERENCE_BINDINGS_PER_JOURNAL + 1)
                .unwrap_err(),
            StorageError::ResourceLimit
        );
        assert!(check_namespace_blob_byte_limit(MAX_NAMESPACE_BLOB_BYTES).is_ok());
        assert_eq!(
            check_namespace_blob_byte_limit(MAX_NAMESPACE_BLOB_BYTES + 1).unwrap_err(),
            StorageError::ResourceLimit
        );
    }

    #[test]
    fn authenticated_malformed_manifest_and_inventory_fail_closed() {
        let database = DatabaseId::from_bytes([0xc0; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xc1; 16]));

        let mut marker_filesystem = MemoryFileSystem::default();
        let mut marker_store = JournalStore::create(
            &mut marker_filesystem,
            options(database, "malformed-marker"),
            create_vault(database, 69_000),
            CounterEntropy::new(70_000),
        )
        .unwrap();
        let mut upload = marker_store.start_blob_upload(scope).unwrap();
        let token = upload.token();
        let reference = marker_store
            .finish_blob_upload(&mut marker_filesystem, &mut upload)
            .unwrap();
        let mut malformed = crate::blob::encode_manifest(token, Some(reference));
        malformed[104] = 1;
        let encoded = marker_store
            .vault
            .encrypt(
                crate::blob::manifest_context(
                    database,
                    marker_store.epoch,
                    marker_store.writer,
                    token,
                    crate::blob::MANIFEST_FINAL,
                ),
                &malformed,
            )
            .unwrap()
            .encode()
            .unwrap();
        let marker_file = marker_filesystem
            .open_existing(
                &marker_store.database_directory,
                &crate::blob::manifest_name(token, crate::blob::MANIFEST_FINAL).unwrap(),
            )
            .unwrap();
        marker_filesystem.set_len(&marker_file, 0).unwrap();
        write_all_at(&mut marker_filesystem, &marker_file, 0, &encoded).unwrap();
        marker_filesystem
            .set_len(&marker_file, u64::try_from(encoded.len()).unwrap())
            .unwrap();
        marker_filesystem.sync_all(&marker_file).unwrap();
        assert_eq!(
            marker_store
                .resume_blob_upload(&mut marker_filesystem, token)
                .unwrap_err(),
            StorageError::IntegrityFailure
        );

        let mut inventory_filesystem = MemoryFileSystem::default();
        let mut inventory_store = JournalStore::create(
            &mut inventory_filesystem,
            options(database, "malformed-inventory"),
            create_vault(database, 71_000),
            CounterEntropy::new(72_000),
        )
        .unwrap();
        let mut upload = inventory_store.start_blob_upload(scope).unwrap();
        inventory_store
            .write_blob_upload(&mut inventory_filesystem, &mut upload, b"inventory bytes")
            .unwrap();
        let reference = inventory_store
            .finish_blob_upload(&mut inventory_filesystem, &mut upload)
            .unwrap();
        let inventory = BlobInventory::new(scope, [reference]).unwrap();
        inventory_store
            .append_group_with_inventory(
                &mut inventory_filesystem,
                CommitInput {
                    encoded_group: b"committed malformed inventory target",
                    logical_event_digest: [0xc2; 32],
                },
                &inventory,
            )
            .unwrap();
        let mut malformed = inventory.encoded().to_vec();
        malformed[28] = 1;
        let encoded = inventory_store
            .vault
            .encrypt(
                inventory_context(
                    database,
                    inventory_store.epoch,
                    inventory_store.writer,
                    inventory.digest(),
                ),
                &malformed,
            )
            .unwrap()
            .encode()
            .unwrap();
        let inventory_file = inventory_filesystem
            .open_existing(
                &inventory_store.database_directory,
                &inventory_name(
                    &inventory_store.vault,
                    database,
                    inventory_store.epoch,
                    inventory_store.writer,
                    inventory.digest(),
                )
                .unwrap(),
            )
            .unwrap();
        inventory_filesystem.set_len(&inventory_file, 0).unwrap();
        write_all_at(&mut inventory_filesystem, &inventory_file, 0, &encoded).unwrap();
        inventory_filesystem
            .set_len(&inventory_file, u64::try_from(encoded.len()).unwrap())
            .unwrap();
        inventory_filesystem.sync_all(&inventory_file).unwrap();
        drop(inventory_store);
        let mut callbacks = 0_u64;
        assert_eq!(
            JournalStore::open(
                &mut inventory_filesystem,
                &entry("malformed-inventory"),
                database,
                CounterEntropy::new(73_000),
                CounterEntropy::new(74_000),
                &mut TestKeyAdapter,
                |_group| {
                    callbacks += 1;
                    Ok(())
                },
            )
            .unwrap_err(),
            StorageError::IntegrityFailure
        );
        assert_eq!(callbacks, 0);
    }

    #[test]
    fn authenticated_progress_digest_mismatch_fails_closed() {
        let database = DatabaseId::from_bytes([0xd7; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xd8; 16]));
        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "malformed-progress"),
            create_vault(database, 69_300),
            CounterEntropy::new(69_400),
        )
        .unwrap();
        let bytes = vec![0x7d; crate::blob::BLOB_CHUNK_BYTES];
        let mut upload = store.start_blob_upload(scope).unwrap();
        let token = upload.token();
        store
            .write_blob_upload(&mut filesystem, &mut upload, &bytes)
            .unwrap();
        drop(upload);
        let mut malformed = crate::blob::encode_progress_plaintext(token, 0, &bytes).unwrap();
        malformed[64] ^= 1;
        let encoded = store
            .vault
            .encrypt(
                crate::blob::progress_context(database, store.epoch, store.writer, token, 0)
                    .unwrap(),
                &malformed,
            )
            .unwrap()
            .encode()
            .unwrap();
        let progress_file = filesystem
            .open_existing(
                &store.database_directory,
                &crate::blob::progress_name(token, 0).unwrap(),
            )
            .unwrap();
        filesystem.set_len(&progress_file, 0).unwrap();
        write_all_at(&mut filesystem, &progress_file, 0, &encoded).unwrap();
        filesystem
            .set_len(&progress_file, u64::try_from(encoded.len()).unwrap())
            .unwrap();
        filesystem.sync_all(&progress_file).unwrap();
        assert_eq!(
            store
                .resume_blob_upload(&mut filesystem, token)
                .unwrap_err(),
            StorageError::IntegrityFailure
        );
    }

    #[test]
    fn committed_chunk_and_inventory_mutation_truncation_and_append_fail_before_replay() {
        let database = DatabaseId::from_bytes([0xc3; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xc4; 16]));
        let mut base = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut base,
            options(database, "blob-corruption"),
            create_vault(database, 75_000),
            CounterEntropy::new(76_000),
        )
        .unwrap();
        let mut upload = store.start_blob_upload(scope).unwrap();
        store
            .write_blob_upload(&mut base, &mut upload, b"authenticated committed bytes")
            .unwrap();
        let reference = store.finish_blob_upload(&mut base, &mut upload).unwrap();
        let inventory = BlobInventory::new(scope, [reference]).unwrap();
        store
            .append_group_with_inventory(
                &mut base,
                CommitInput {
                    encoded_group: b"corruption target",
                    logical_event_digest: [0xc5; 32],
                },
                &inventory,
            )
            .unwrap();
        drop(store);
        base.restart().unwrap();

        for case in 0..7 {
            let mut filesystem = base.clone();
            let root = filesystem.root();
            let directory = filesystem
                .open_directory(&root, &entry("blob-corruption"))
                .unwrap();
            let names = filesystem.test_child_names(&directory).unwrap();
            let chunk = names
                .iter()
                .find(|name| name.as_str().starts_with("b-"))
                .unwrap()
                .clone();
            let inventory = names
                .iter()
                .find(|name| name.as_str().starts_with("i-"))
                .unwrap()
                .clone();
            let target = if case < 4 { &chunk } else { &inventory };
            match case {
                0 | 4 => filesystem.test_mutate_file(&directory, target, 0).unwrap(),
                1 => {
                    let file = filesystem.open_existing(&directory, target).unwrap();
                    let len = filesystem.metadata(&file).unwrap().len;
                    filesystem
                        .test_mutate_file(&directory, target, usize::try_from(len - 1).unwrap())
                        .unwrap();
                }
                2 | 5 => {
                    let file = filesystem.open_existing(&directory, target).unwrap();
                    let len = filesystem.metadata(&file).unwrap().len;
                    filesystem.set_len(&file, len - 1).unwrap();
                    filesystem.sync_data(&file).unwrap();
                }
                3 | 6 => filesystem
                    .test_append_file(&directory, target, &[0])
                    .unwrap(),
                _ => unreachable!(),
            }
            let mut callbacks = 0_u64;
            assert_eq!(
                JournalStore::open(
                    &mut filesystem,
                    &entry("blob-corruption"),
                    database,
                    CounterEntropy::new(77_000 + case),
                    CounterEntropy::new(78_000 + case),
                    &mut TestKeyAdapter,
                    |_group| {
                        callbacks += 1;
                        Ok(())
                    },
                )
                .unwrap_err(),
                StorageError::IntegrityFailure,
                "case={case}"
            );
            assert_eq!(callbacks, 0);
        }
    }

    #[test]
    fn valid_ciphertext_replayed_under_another_blob_context_fails_closed() {
        let database = DatabaseId::from_bytes([0xc6; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xc7; 16]));
        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "blob-context-replay"),
            create_vault(database, 79_000),
            CounterEntropy::new(80_000),
        )
        .unwrap();
        let mut references = Vec::new();
        for byte in [0x11, 0x22] {
            let mut upload = store.start_blob_upload(scope).unwrap();
            store
                .write_blob_upload(&mut filesystem, &mut upload, &[byte; 32])
                .unwrap();
            references.push(
                store
                    .finish_blob_upload(&mut filesystem, &mut upload)
                    .unwrap(),
            );
        }
        let inventory = BlobInventory::new(scope, references.iter().copied()).unwrap();
        store
            .append_group_with_inventory(
                &mut filesystem,
                CommitInput {
                    encoded_group: b"context replay",
                    logical_event_digest: [0xc8; 32],
                },
                &inventory,
            )
            .unwrap();
        let source_name = crate::blob::final_chunk_name(references[1].id(), 0).unwrap();
        let target_name = crate::blob::final_chunk_name(references[0].id(), 0).unwrap();
        let source = filesystem
            .open_existing(&store.database_directory, &source_name)
            .unwrap();
        let source_len = filesystem.metadata(&source).unwrap().len;
        let mut encoded = vec![0_u8; usize::try_from(source_len).unwrap()];
        read_exact_at(&mut filesystem, &source, 0, &mut encoded).unwrap();
        let target = filesystem
            .open_existing(&store.database_directory, &target_name)
            .unwrap();
        filesystem.set_len(&target, 0).unwrap();
        write_all_at(&mut filesystem, &target, 0, &encoded).unwrap();
        filesystem.set_len(&target, source_len).unwrap();
        filesystem.sync_all(&target).unwrap();
        drop(store);
        let mut callbacks = 0_u64;
        assert_eq!(
            JournalStore::open(
                &mut filesystem,
                &entry("blob-context-replay"),
                database,
                CounterEntropy::new(81_000),
                CounterEntropy::new(82_000),
                &mut TestKeyAdapter,
                |_group| {
                    callbacks += 1;
                    Ok(())
                },
            )
            .unwrap_err(),
            StorageError::IntegrityFailure
        );
        assert_eq!(callbacks, 0);
    }

    #[test]
    fn duplicate_upload_handles_cannot_replace_a_staged_chunk() {
        let database = DatabaseId::from_bytes([0x90; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0x91; 16]));
        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "immutable-staging"),
            create_vault(database, 40_000),
            CounterEntropy::new(41_000),
        )
        .unwrap();
        let mut first = store.start_blob_upload(scope).unwrap();
        let token = first.token();
        let mut duplicate = store.resume_blob_upload(&mut filesystem, token).unwrap();
        let original = vec![0x31; crate::blob::BLOB_CHUNK_BYTES];
        let alternate = vec![0x32; crate::blob::BLOB_CHUNK_BYTES];
        store
            .write_blob_upload(&mut filesystem, &mut first, &original)
            .unwrap();
        assert_eq!(
            store
                .write_blob_upload(&mut filesystem, &mut duplicate, &alternate)
                .unwrap_err(),
            StorageError::IntegrityFailure
        );
        assert_eq!(
            store
                .write_blob_upload(&mut filesystem, &mut duplicate, &[])
                .unwrap_err(),
            StorageError::NeedsRecovery
        );
        let mut resumed = store.resume_blob_upload(&mut filesystem, token).unwrap();
        let reference = store
            .finish_blob_upload(&mut filesystem, &mut resumed)
            .unwrap();
        let inventory = BlobInventory::new(scope, [reference]).unwrap();
        store
            .append_group_with_inventory(
                &mut filesystem,
                CommitInput {
                    encoded_group: b"immutable chunk",
                    logical_event_digest: [0x92; 32],
                },
                &inventory,
            )
            .unwrap();
        let mut output = vec![0_u8; crate::blob::BLOB_CHUNK_BYTES];
        assert_eq!(
            store
                .read_blob_range(&mut filesystem, reference, 0, &mut output)
                .unwrap(),
            original.len()
        );
        assert_eq!(output, original);
    }

    #[test]
    fn duplicate_handles_cannot_publish_conflicting_terminal_states() {
        for finish_first in [true, false] {
            let database = DatabaseId::from_bytes([0xd4; 16]);
            let scope =
                NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0xd5; 16]));
            let name = if finish_first {
                "terminal-finish-first"
            } else {
                "terminal-abort-first"
            };
            let mut filesystem = MemoryFileSystem::default();
            let mut store = JournalStore::create(
                &mut filesystem,
                options(database, name),
                create_vault(database, if finish_first { 75_000 } else { 76_000 }),
                CounterEntropy::new(if finish_first { 75_100 } else { 76_100 }),
            )
            .unwrap();
            let mut first = store.start_blob_upload(scope).unwrap();
            let token = first.token();
            let mut duplicate = store.resume_blob_upload(&mut filesystem, token).unwrap();
            if finish_first {
                let reference = store
                    .finish_blob_upload(&mut filesystem, &mut first)
                    .unwrap();
                assert_eq!(first.buffered_capacity(), 0);
                assert_eq!(
                    store
                        .write_blob_upload(&mut filesystem, &mut duplicate, b"stale write")
                        .unwrap_err(),
                    StorageError::InvalidState
                );
                assert_eq!(
                    store
                        .abort_blob_upload(&mut filesystem, &mut duplicate)
                        .unwrap_err(),
                    StorageError::InvalidState
                );
                let inventory = BlobInventory::new(scope, [reference]).unwrap();
                store
                    .append_group_with_inventory(
                        &mut filesystem,
                        CommitInput {
                            encoded_group: b"duplicate terminal exclusion",
                            logical_event_digest: [0xd6; 32],
                        },
                        &inventory,
                    )
                    .unwrap();
                assert_eq!(
                    store
                        .read_blob_range(&mut filesystem, reference, 0, &mut [])
                        .unwrap(),
                    0
                );
            } else {
                store
                    .abort_blob_upload(&mut filesystem, &mut first)
                    .unwrap();
                assert_eq!(first.buffered_capacity(), 0);
                assert_eq!(
                    store
                        .write_blob_upload(&mut filesystem, &mut duplicate, b"stale write")
                        .unwrap_err(),
                    StorageError::InvalidState
                );
                assert_eq!(
                    store
                        .finish_blob_upload(&mut filesystem, &mut duplicate)
                        .unwrap_err(),
                    StorageError::InvalidState
                );
            }
            let names = filesystem
                .test_child_names(&store.database_directory)
                .unwrap();
            let final_primary =
                crate::blob::manifest_name(token, crate::blob::MANIFEST_FINAL).unwrap();
            let final_witness =
                crate::blob::manifest_witness_name(token, crate::blob::MANIFEST_FINAL).unwrap();
            let abort_primary =
                crate::blob::manifest_name(token, crate::blob::MANIFEST_ABORTED).unwrap();
            let abort_witness =
                crate::blob::manifest_witness_name(token, crate::blob::MANIFEST_ABORTED).unwrap();
            assert_eq!(names.contains(&final_primary), finish_first);
            assert_eq!(names.contains(&final_witness), finish_first);
            assert_eq!(names.contains(&abort_primary), !finish_first);
            assert_eq!(names.contains(&abort_witness), !finish_first);
        }
    }

    #[test]
    fn failed_chunk_flush_requires_resume_without_duplicate_input() {
        let database = DatabaseId::from_bytes([0x93; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0x94; 16]));
        let plan = FaultPlan::new([FaultPoint {
            operation: Operation::SyncAll,
            occurrence: 6,
            action: FaultAction::Error(AdapterErrorKind::Io),
        }])
        .unwrap();
        let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "flush-resume"),
            create_vault(database, 42_000),
            CounterEntropy::new(43_000),
        )
        .unwrap();
        let bytes = vec![0x41; crate::blob::BLOB_CHUNK_BYTES];
        let mut upload = store.start_blob_upload(scope).unwrap();
        let token = upload.token();
        assert_eq!(
            store
                .write_blob_upload(&mut filesystem, &mut upload, &bytes)
                .unwrap_err(),
            StorageError::Adapter(AdapterErrorKind::Io)
        );
        assert_eq!(
            store
                .write_blob_upload(&mut filesystem, &mut upload, &bytes)
                .unwrap_err(),
            StorageError::NeedsRecovery
        );
        let mut resumed = store.resume_blob_upload(&mut filesystem, token).unwrap();
        assert_eq!(resumed.durable_bytes(), 0);
        store
            .write_blob_upload(&mut filesystem, &mut resumed, &bytes)
            .unwrap();
        let reference = store
            .finish_blob_upload(&mut filesystem, &mut resumed)
            .unwrap();
        let inventory = BlobInventory::new(scope, [reference]).unwrap();
        store
            .append_group_with_inventory(
                &mut filesystem,
                CommitInput {
                    encoded_group: b"resumed exactly once",
                    logical_event_digest: [0x95; 32],
                },
                &inventory,
            )
            .unwrap();
        let mut output = vec![0_u8; crate::blob::BLOB_CHUNK_BYTES];
        store
            .read_blob_range(&mut filesystem, reference, 0, &mut output)
            .unwrap();
        assert_eq!(output, bytes);
    }

    #[test]
    fn partial_terminal_chunk_survives_repeated_resume_before_finish() {
        let database = DatabaseId::from_bytes([0x96; 16]);
        let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0x97; 16]));
        let mut filesystem =
            FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "partial-resume"),
            create_vault(database, 44_000),
            CounterEntropy::new(45_000),
        )
        .unwrap();
        let bytes = b"durable terminal bytes";
        let mut upload = store.start_blob_upload(scope).unwrap();
        let token = upload.token();
        store
            .write_blob_upload(&mut filesystem, &mut upload, bytes)
            .unwrap();
        filesystem
            .arm(
                FaultPlan::new([FaultPoint {
                    operation: Operation::RenameNoReplace,
                    occurrence: 2,
                    action: FaultAction::CrashBefore,
                }])
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            store
                .finish_blob_upload(&mut filesystem, &mut upload)
                .unwrap_err(),
            StorageError::Adapter(AdapterErrorKind::InjectedCrash)
        );
        drop(store);
        filesystem.restart().unwrap();

        let (mut store, _) = JournalStore::open(
            &mut filesystem,
            &entry("partial-resume"),
            database,
            CounterEntropy::new(46_000),
            CounterEntropy::new(47_000),
            &mut TestKeyAdapter,
            |_group| Ok(()),
        )
        .unwrap();
        let resumed = store.resume_blob_upload(&mut filesystem, token).unwrap();
        assert_eq!(resumed.durable_bytes(), u64::try_from(bytes.len()).unwrap());
        drop(resumed);
        drop(store);
        filesystem.restart().unwrap();

        let (mut store, _) = JournalStore::open(
            &mut filesystem,
            &entry("partial-resume"),
            database,
            CounterEntropy::new(48_000),
            CounterEntropy::new(49_000),
            &mut TestKeyAdapter,
            |_group| Ok(()),
        )
        .unwrap();
        let mut resumed = store.resume_blob_upload(&mut filesystem, token).unwrap();
        assert_eq!(resumed.durable_bytes(), u64::try_from(bytes.len()).unwrap());
        assert_eq!(
            store
                .write_blob_upload(&mut filesystem, &mut resumed, b"extra")
                .unwrap_err(),
            StorageError::InvalidState
        );
        let reference = store
            .finish_blob_upload(&mut filesystem, &mut resumed)
            .unwrap();
        let inventory = BlobInventory::new(scope, [reference]).unwrap();
        store
            .append_group_with_inventory(
                &mut filesystem,
                CommitInput {
                    encoded_group: b"partial terminal",
                    logical_event_digest: [0x98; 32],
                },
                &inventory,
            )
            .unwrap();
        let mut output = [0_u8; 64];
        let count = store
            .read_blob_range(&mut filesystem, reference, 0, &mut output)
            .unwrap();
        assert_eq!(&output[..count], bytes);
    }

    #[test]
    fn every_initial_creation_crash_boundary_has_one_permitted_name_outcome() {
        let boundaries = [
            (Operation::CreateDirectory, 1_u64),
            (Operation::CreateNew, 1),
            (Operation::CreateNew, 2),
            (Operation::CreateNew, 3),
            (Operation::CreateNew, 4),
            (Operation::CreateNew, 5),
            (Operation::SyncAll, 1),
            (Operation::SyncAll, 2),
            (Operation::SyncAll, 3),
            (Operation::SyncAll, 4),
            (Operation::SyncAll, 5),
            (Operation::TryLockExclusive, 1),
            (Operation::WriteAt, 1),
            (Operation::WriteAt, 2),
            (Operation::WriteAt, 3),
            (Operation::WriteAt, 4),
            (Operation::SyncDirectory, 1),
            (Operation::RenameNoReplace, 1),
            (Operation::SyncDirectory, 2),
        ];
        for (case, (operation, occurrence)) in boundaries.into_iter().enumerate() {
            for action in [FaultAction::CrashBefore, FaultAction::CrashAfter] {
                let database = DatabaseId::from_bytes([0x91; 16]);
                let plan = FaultPlan::new([FaultPoint {
                    operation,
                    occurrence,
                    action,
                }])
                .unwrap();
                let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
                let result = JournalStore::create(
                    &mut filesystem,
                    options(database, "creation-boundary"),
                    create_vault(database, 2_000 + u64::try_from(case).unwrap()),
                    CounterEntropy::new(3_000 + u64::try_from(case).unwrap()),
                );
                assert_eq!(
                    result.unwrap_err(),
                    StorageError::Adapter(AdapterErrorKind::InjectedCrash),
                    "operation={operation:?} occurrence={occurrence} action={action:?}"
                );
                filesystem.restart().unwrap();
                let root = filesystem.root();
                let published = filesystem
                    .open_directory(&root, &entry("creation-boundary"))
                    .is_ok();
                let expected_published = operation == Operation::SyncDirectory
                    && occurrence == 2
                    && action == FaultAction::CrashAfter;
                assert_eq!(
                    published, expected_published,
                    "operation={operation:?} occurrence={occurrence} action={action:?}"
                );
                if published {
                    let (_store, report) = JournalStore::open(
                        &mut filesystem,
                        &entry("creation-boundary"),
                        database,
                        CounterEntropy::new(4_000),
                        CounterEntropy::new(5_000),
                        &mut TestKeyAdapter,
                        |_group| Ok(()),
                    )
                    .unwrap();
                    assert_eq!(report.frontier, None);
                }
            }
        }
    }

    #[test]
    fn every_initial_commit_crash_boundary_recovers_previous_or_exact_new_frontier() {
        let boundaries = [
            (Operation::WriteAt, 5_u64),
            (Operation::SyncData, 1),
            (Operation::WriteAt, 6),
            (Operation::SyncData, 2),
        ];
        for (case, (operation, occurrence)) in boundaries.into_iter().enumerate() {
            for action in [FaultAction::CrashBefore, FaultAction::CrashAfter] {
                let database = DatabaseId::from_bytes([0xa1; 16]);
                let plan = FaultPlan::new([FaultPoint {
                    operation,
                    occurrence,
                    action,
                }])
                .unwrap();
                let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
                let mut store = JournalStore::create(
                    &mut filesystem,
                    options(database, "commit-boundary"),
                    create_vault(database, 6_000 + u64::try_from(case).unwrap()),
                    CounterEntropy::new(7_000 + u64::try_from(case).unwrap()),
                )
                .unwrap();
                assert_eq!(
                    store
                        .append_group(
                            &mut filesystem,
                            CommitInput {
                                encoded_group: b"one atomic group",
                                logical_event_digest: [0xa2; 32],
                            },
                        )
                        .unwrap_err(),
                    StorageError::Adapter(AdapterErrorKind::InjectedCrash),
                    "operation={operation:?} occurrence={occurrence} action={action:?}"
                );
                drop(store);
                filesystem.restart().unwrap();
                let mut replayed = Vec::new();
                let (_store, report) = JournalStore::open(
                    &mut filesystem,
                    &entry("commit-boundary"),
                    database,
                    CounterEntropy::new(8_000),
                    CounterEntropy::new(9_000),
                    &mut TestKeyAdapter,
                    |group| {
                        replayed.push((group.revision, group.encoded_group.to_vec()));
                        Ok(())
                    },
                )
                .unwrap();
                let expected_new = operation == Operation::SyncData
                    && occurrence == 2
                    && action == FaultAction::CrashAfter;
                assert_eq!(
                    report.frontier.is_some(),
                    expected_new,
                    "operation={operation:?} occurrence={occurrence} action={action:?}"
                );
                if expected_new {
                    assert_eq!(
                        replayed,
                        vec![(CommitRevision::FIRST, b"one atomic group".to_vec())]
                    );
                } else {
                    assert!(replayed.is_empty());
                }
            }
        }
    }

    #[test]
    fn initial_publication_errors_never_create_a_durable_database() {
        let boundaries = [
            (Operation::CreateDirectory, 1_u64),
            (Operation::CreateNew, 1),
            (Operation::CreateNew, 2),
            (Operation::CreateNew, 3),
            (Operation::CreateNew, 4),
            (Operation::CreateNew, 5),
            (Operation::SyncAll, 1),
            (Operation::SyncAll, 2),
            (Operation::SyncAll, 3),
            (Operation::SyncAll, 4),
            (Operation::SyncAll, 5),
            (Operation::TryLockExclusive, 1),
            (Operation::WriteAt, 1),
            (Operation::WriteAt, 2),
            (Operation::WriteAt, 3),
            (Operation::WriteAt, 4),
            (Operation::SyncDirectory, 1),
            (Operation::RenameNoReplace, 1),
            (Operation::SyncDirectory, 2),
        ];
        for (case, (operation, occurrence)) in boundaries.into_iter().enumerate() {
            let database = DatabaseId::from_bytes([0xa3; 16]);
            let plan = FaultPlan::new([FaultPoint {
                operation,
                occurrence,
                action: FaultAction::Error(AdapterErrorKind::Io),
            }])
            .unwrap();
            let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
            assert_eq!(
                JournalStore::create(
                    &mut filesystem,
                    options(database, "creation-error"),
                    create_vault(database, 9_100 + u64::try_from(case).unwrap()),
                    CounterEntropy::new(9_200 + u64::try_from(case).unwrap()),
                )
                .unwrap_err(),
                StorageError::Adapter(AdapterErrorKind::Io),
                "operation={operation:?} occurrence={occurrence}"
            );
            assert_eq!(filesystem.pending_faults(), 0);
            filesystem.restart().unwrap();
            let root = filesystem.root();
            assert!(
                filesystem
                    .open_directory(&root, &entry("creation-error"))
                    .is_err(),
                "operation={operation:?} occurrence={occurrence}"
            );
        }
    }

    #[test]
    fn initial_commit_errors_recover_only_the_previous_frontier() {
        for (case, (operation, occurrence)) in [
            (Operation::WriteAt, 5_u64),
            (Operation::SyncData, 1),
            (Operation::WriteAt, 6),
            (Operation::SyncData, 2),
        ]
        .into_iter()
        .enumerate()
        {
            let database = DatabaseId::from_bytes([0xa4; 16]);
            let plan = FaultPlan::new([FaultPoint {
                operation,
                occurrence,
                action: FaultAction::Error(AdapterErrorKind::Io),
            }])
            .unwrap();
            let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
            let mut store = JournalStore::create(
                &mut filesystem,
                options(database, "commit-error"),
                create_vault(database, 9_300 + u64::try_from(case).unwrap()),
                CounterEntropy::new(9_400 + u64::try_from(case).unwrap()),
            )
            .unwrap();
            assert_eq!(
                store
                    .append_group(
                        &mut filesystem,
                        CommitInput {
                            encoded_group: b"must remain uncommitted",
                            logical_event_digest: [0xa5; 32],
                        },
                    )
                    .unwrap_err(),
                StorageError::Adapter(AdapterErrorKind::Io)
            );
            assert_eq!(
                store
                    .append_group(
                        &mut filesystem,
                        CommitInput {
                            encoded_group: b"poisoned writer",
                            logical_event_digest: [0xa6; 32],
                        },
                    )
                    .unwrap_err(),
                StorageError::NeedsRecovery
            );
            drop(store);
            filesystem.restart().unwrap();
            let mut callbacks = 0_u64;
            let (_store, report) = JournalStore::open(
                &mut filesystem,
                &entry("commit-error"),
                database,
                CounterEntropy::new(9_500),
                CounterEntropy::new(9_600),
                &mut TestKeyAdapter,
                |_group| {
                    callbacks += 1;
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(report.frontier, None);
            assert_eq!(callbacks, 0);
        }
    }

    #[test]
    fn journal_exact_io_retries_every_short_progress_position() {
        let database = DatabaseId::from_bytes([0xa7; 16]);
        for occurrence in 1..=6 {
            let plan = FaultPlan::new([FaultPoint {
                operation: Operation::WriteAt,
                occurrence,
                action: FaultAction::ShortWrite { maximum: 1 },
            }])
            .unwrap();
            let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
            let mut store = JournalStore::create(
                &mut filesystem,
                options(database, "short-write"),
                create_vault(database, 9_700 + occurrence),
                CounterEntropy::new(9_800 + occurrence),
            )
            .unwrap();
            store
                .append_group(
                    &mut filesystem,
                    CommitInput {
                        encoded_group: b"exact despite a short write",
                        logical_event_digest: [0xa8; 32],
                    },
                )
                .unwrap();
            assert_eq!(filesystem.pending_faults(), 0);
            drop(store);
            filesystem.restart().unwrap();
            let mut replayed = Vec::new();
            let (_store, report) = JournalStore::open(
                &mut filesystem,
                &entry("short-write"),
                database,
                CounterEntropy::new(9_900),
                CounterEntropy::new(10_000),
                &mut TestKeyAdapter,
                |group| {
                    replayed.push(group.encoded_group.to_vec());
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(report.frontier, Some(CommitRevision::FIRST));
            assert_eq!(replayed, vec![b"exact despite a short write".to_vec()]);
        }

        let mut base = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut base,
            options(database, "short-read"),
            create_vault(database, 10_100),
            CounterEntropy::new(10_200),
        )
        .unwrap();
        for group in [b"first".as_slice(), b"second"] {
            store
                .append_group(
                    &mut base,
                    CommitInput {
                        encoded_group: group,
                        logical_event_digest: [0xa9; 32],
                    },
                )
                .unwrap();
        }
        drop(store);
        base.restart().unwrap();
        for occurrence in 1..=12 {
            let plan = FaultPlan::new([FaultPoint {
                operation: Operation::ReadAt,
                occurrence,
                action: FaultAction::ShortRead { maximum: 1 },
            }])
            .unwrap();
            let mut filesystem = FaultFileSystem::new(base.clone(), plan);
            let mut replayed = Vec::new();
            let (store, report) = JournalStore::open(
                &mut filesystem,
                &entry("short-read"),
                database,
                CounterEntropy::new(10_300 + occurrence),
                CounterEntropy::new(10_400 + occurrence),
                &mut TestKeyAdapter,
                |group| {
                    replayed.push(group.encoded_group.to_vec());
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(report.frontier.map(CommitRevision::get), Some(2));
            assert_eq!(replayed, vec![b"first".to_vec(), b"second".to_vec()]);
            drop(store);
            assert_eq!(filesystem.pending_faults(), 0, "occurrence={occurrence}");
        }
    }

    #[test]
    fn rollover_header_is_durable_before_use_and_replays_through_segment_chain() {
        let database = DatabaseId::from_bytes([0xb1; 16]);
        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            options(database, "rollover"),
            create_vault(database, 10_000),
            CounterEntropy::new(11_000),
        )
        .unwrap();
        store.current_segment_offset = JOURNAL_SEGMENT_LIMIT;
        store
            .append_group(
                &mut filesystem,
                CommitInput {
                    encoded_group: b"first group in second segment",
                    logical_event_digest: [0xb2; 32],
                },
            )
            .unwrap();
        drop(store);
        filesystem.restart().unwrap();

        let mut replayed = Vec::new();
        let (_store, report) = JournalStore::open(
            &mut filesystem,
            &entry("rollover"),
            database,
            CounterEntropy::new(12_000),
            CounterEntropy::new(13_000),
            &mut TestKeyAdapter,
            |group| {
                replayed.push(group.encoded_group.to_vec());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(report.frontier, Some(CommitRevision::FIRST));
        assert_eq!(replayed, vec![b"first group in second segment".to_vec()]);
    }

    #[test]
    fn every_rollover_commit_crash_recovers_the_previous_or_exact_new_frontier() {
        let boundaries = [
            (Operation::CreateNew, 6_u64),
            (Operation::WriteAt, 5),
            (Operation::SyncAll, 6),
            (Operation::SyncDirectory, 3),
            (Operation::WriteAt, 6),
            (Operation::SyncData, 1),
            (Operation::WriteAt, 7),
            (Operation::SyncData, 2),
        ];
        for (case, (operation, occurrence)) in boundaries.into_iter().enumerate() {
            for action in [FaultAction::CrashBefore, FaultAction::CrashAfter] {
                let database = DatabaseId::from_bytes([0xc1; 16]);
                let plan = FaultPlan::new([FaultPoint {
                    operation,
                    occurrence,
                    action,
                }])
                .unwrap();
                let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
                let mut store = JournalStore::create(
                    &mut filesystem,
                    options(database, "rollover-boundary"),
                    create_vault(database, 14_000 + u64::try_from(case).unwrap()),
                    CounterEntropy::new(15_000 + u64::try_from(case).unwrap()),
                )
                .unwrap();
                store.current_segment_offset = JOURNAL_SEGMENT_LIMIT;
                assert_eq!(
                    store
                        .append_group(
                            &mut filesystem,
                            CommitInput {
                                encoded_group: b"not yet certified",
                                logical_event_digest: [0xc2; 32],
                            },
                        )
                        .unwrap_err(),
                    StorageError::Adapter(AdapterErrorKind::InjectedCrash),
                    "operation={operation:?} occurrence={occurrence} action={action:?}"
                );
                drop(store);
                filesystem.restart().unwrap();
                let mut replayed = Vec::new();
                let (_store, report) = JournalStore::open(
                    &mut filesystem,
                    &entry("rollover-boundary"),
                    database,
                    CounterEntropy::new(16_000),
                    CounterEntropy::new(17_000),
                    &mut TestKeyAdapter,
                    |group| {
                        replayed.push(group.encoded_group.to_vec());
                        Ok(())
                    },
                )
                .unwrap();
                let expected_new = operation == Operation::SyncData
                    && occurrence == 2
                    && action == FaultAction::CrashAfter;
                assert_eq!(report.frontier.is_some(), expected_new);
                if expected_new {
                    assert_eq!(replayed, vec![b"not yet certified".to_vec()]);
                } else {
                    assert!(replayed.is_empty());
                }
            }
        }
    }
}
