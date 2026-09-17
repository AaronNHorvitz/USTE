//! Encrypted commit-certificate journal and streaming recovery.
//!
//! This layer publishes opaque transaction groups. Transaction coordination, conflicts and
//! idempotency are deliberately owned by T-14 rather than inferred here.

use core::fmt;

use sha2::{Digest, Sha256};
use uste_crypto::{
    CryptoContext, CryptoError, CryptoObjectId, EncryptedEnvelope, EntropySource, FrameClass,
    KeyAdapter, KeyEpoch, KeyVault, MAX_PLAINTEXT_BYTES, ObjectRole, RecoveryEnvelope, Scope,
    WriterIncarnationId,
};
use uste_types::{CommitRevision, DatabaseId};

use crate::{
    AdapterError, AdapterErrorKind, EntryName, FileSystem, OwnershipFileSystem, read_exact_at,
    write_all_at,
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

/// SHA-256 of the canonical empty blob inventory. T-15 adds nonempty inventory verification.
pub const EMPTY_BLOB_INVENTORY_DIGEST: [u8; 32] = [
    0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
    0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
];

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
    pub encoded_group: &'a [u8],
    pub blob_inventory_digest: [u8; 32],
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
            &mut |_group| Ok(()),
        )?;

        if repaired_certificate_tail_bytes != 0 {
            filesystem.set_len(
                &certificate_file,
                SMALL_ENVELOPE_BYTES + complete_certificate_bytes,
            )?;
            filesystem.sync_data(&certificate_file)?;
        }

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
            &mut visitor,
        )?;
        if replayed.current_segment_id != validated.current_segment_id
            || replayed.current_segment_offset != validated.current_segment_offset
            || replayed.frontier != validated.frontier
            || replayed.previous_certificate_digest != validated.previous_certificate_digest
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
                poisoned: false,
                vault,
                identity_entropy,
                _ownership: ownership,
            },
            report,
        ))
    }

    /// Publish one encrypted group followed by its authenticated commit certificate.
    pub fn append_group(
        &mut self,
        filesystem: &mut F,
        input: CommitInput<'_>,
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
            blob_inventory_digest: EMPTY_BLOB_INVENTORY_DIGEST,
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
        self.poisoned = false;
        Ok(DurableCommit {
            revision,
            certificate_digest,
        })
    }

    #[must_use]
    pub const fn frontier(&self) -> Option<CommitRevision> {
        self.frontier
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
    uncommitted_tails: Vec<SegmentTail>,
}

struct SegmentTail {
    segment_id: [u8; 16],
    committed_end: u64,
    extra_bytes: u64,
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
    let mut frontier = None;
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
            || certificate.blob_inventory_digest != EMPTY_BLOB_INVENTORY_DIGEST
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
        visitor(RecoveredGroup {
            revision,
            encoded_group: plaintext.as_slice(),
            blob_inventory_digest: certificate.blob_inventory_digest,
            logical_event_digest: certificate.logical_event_digest,
        })?;
        current_segment_offset = group_end;
        previous_certificate_digest = sha256(&encoded_certificate);
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
    let envelope = EncryptedEnvelope::decode(&encoded).map_err(recovery_crypto_error)?;
    let plaintext = vault
        .decrypt(
            segment_context(database, epoch, segment_id, writer, 0),
            &envelope,
        )
        .map_err(recovery_crypto_error)?;
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
