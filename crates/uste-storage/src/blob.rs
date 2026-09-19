//! Bounded encrypted blob uploads and canonical committed inventories.

use std::{
    collections::BTreeMap,
    sync::{LazyLock, Mutex},
};

use sha2::{Digest, Sha256};
use uste_crypto::{
    CryptoContext, CryptoObjectId, EncryptedEnvelope, EntropySource, FrameClass, KeyEpoch,
    KeyVault, OBJECT_ENVELOPE_HEADER_BYTES, ObjectRole, Scope, WriterIncarnationId,
};
use uste_types::{DatabaseId, NamespaceRef};
use zeroize::Zeroize;

use crate::{
    AdapterErrorKind, EntryName, FileSystem, journal::StorageError, read_exact_at, write_all_at,
};

pub const BLOB_CHUNK_BYTES: usize = 1024 * 1024;
pub const MAX_BLOB_BYTES: u64 = 16 * 1024 * 1024 * 1024;
pub const MAX_BLOBS_PER_INVENTORY: usize = 100_000;
pub const MAX_COMMITTED_BLOBS_PER_JOURNAL: usize = 1_000_000;
pub const MAX_BLOB_REFERENCE_BINDINGS_PER_JOURNAL: u64 = 10_000_000;
pub const MAX_NAMESPACE_BLOB_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
pub const MAX_CONCURRENT_UPLOADS: usize = 32;
static ACTIVE_UPLOADS: LazyLock<Mutex<BTreeMap<DatabaseId, usize>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
const INVENTORY_HEADER_BYTES: usize = 32;
const INVENTORY_ENTRY_BYTES: usize = 64;
const BLOB_FORMAT_MAJOR: u8 = 1;
const BLOB_FORMAT_MINOR: u8 = 0;
const BLOB_MANIFEST_BYTES: usize = 128;
const BLOB_PROGRESS_BYTES: usize = 128;
const SMALL_ENCRYPTED_OBJECT_BYTES: u64 = 4_161;
pub(crate) const MANIFEST_FINAL: u8 = 1;
pub(crate) const MANIFEST_ABORTED: u8 = 2;
const PROGRESS_SEQUENCE_BASE: u64 = 3;
const MAX_ENCODED_CHUNK_BYTES: u64 =
    (BLOB_CHUNK_BYTES + 64 * 1024 + OBJECT_ENVELOPE_HEADER_BYTES + 16) as u64;

pub const EMPTY_BLOB_INVENTORY_DIGEST: [u8; 32] = [
    0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
    0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
];

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BlobId([u8; 16]);

impl BlobId {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BlobReference {
    scope: NamespaceRef,
    id: BlobId,
    byte_len: u64,
    chunk_count: u32,
    content_digest: [u8; 32],
}

impl BlobReference {
    pub fn new(
        scope: NamespaceRef,
        id: BlobId,
        byte_len: u64,
        chunk_count: u32,
        content_digest: [u8; 32],
    ) -> Result<Self, StorageError> {
        let reference = Self {
            scope,
            id,
            byte_len,
            chunk_count,
            content_digest,
        };
        if valid_reference_shape(reference) {
            Ok(reference)
        } else {
            Err(StorageError::InvalidState)
        }
    }

    #[must_use]
    pub const fn scope(self) -> NamespaceRef {
        self.scope
    }

    #[must_use]
    pub const fn id(self) -> BlobId {
        self.id
    }

    #[must_use]
    pub const fn byte_len(self) -> u64 {
        self.byte_len
    }

    #[must_use]
    pub const fn chunk_count(self) -> u32 {
        self.chunk_count
    }

    #[must_use]
    pub const fn content_digest(self) -> [u8; 32] {
        self.content_digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlobInventory {
    scope: NamespaceRef,
    references: Vec<BlobReference>,
    encoded: Vec<u8>,
    digest: [u8; 32],
}

impl BlobInventory {
    pub fn new(
        scope: NamespaceRef,
        references: impl IntoIterator<Item = BlobReference>,
    ) -> Result<Self, StorageError> {
        let mut bounded = Vec::new();
        for reference in references {
            if bounded.len() == MAX_BLOBS_PER_INVENTORY {
                return Err(StorageError::ResourceLimit);
            }
            bounded
                .try_reserve(1)
                .map_err(|_| StorageError::ResourceLimit)?;
            bounded.push(reference);
        }
        let mut references = bounded;
        if references
            .iter()
            .any(|reference| reference.scope != scope || !valid_reference_shape(*reference))
        {
            return Err(StorageError::InvalidState);
        }
        references.sort_unstable_by_key(|reference| reference.id);
        if references.windows(2).any(|pair| pair[0].id == pair[1].id) {
            return Err(StorageError::InvalidState);
        }
        let encoded = if references.is_empty() {
            Vec::new()
        } else {
            encode_inventory(scope, &references)?
        };
        let digest = Sha256::digest(&encoded).into();
        Ok(Self {
            scope,
            references,
            encoded,
            digest,
        })
    }

    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.scope
    }

    #[must_use]
    pub fn references(&self) -> &[BlobReference] {
        &self.references
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.references.is_empty()
    }

    #[must_use]
    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }

    pub(crate) fn decode(database: DatabaseId, bytes: &[u8]) -> Result<Self, StorageError> {
        Self::decode_bounded(database, bytes, MAX_BLOBS_PER_INVENTORY)
    }

    pub(crate) fn decode_bounded(
        database: DatabaseId,
        bytes: &[u8],
        maximum_references: usize,
    ) -> Result<Self, StorageError> {
        if bytes.len() < INVENTORY_HEADER_BYTES
            || &bytes[..4] != b"UBIN"
            || bytes[4] != BLOB_FORMAT_MAJOR
            || bytes[5] != BLOB_FORMAT_MINOR
            || bytes[6..8] != [0, 0]
            || bytes[28..32].iter().any(|byte| *byte != 0)
        {
            return Err(StorageError::IntegrityFailure);
        }
        let count = usize::try_from(u32::from_be_bytes(read_array(bytes, 24)?))
            .map_err(|_| StorageError::ResourceLimit)?;
        let expected = INVENTORY_HEADER_BYTES
            .checked_add(
                count
                    .checked_mul(INVENTORY_ENTRY_BYTES)
                    .ok_or(StorageError::ResourceLimit)?,
            )
            .ok_or(StorageError::ResourceLimit)?;
        if count == 0 || count > MAX_BLOBS_PER_INVENTORY || bytes.len() != expected {
            return Err(StorageError::IntegrityFailure);
        }
        if count > maximum_references {
            return Err(StorageError::ResourceLimit);
        }
        let scope = NamespaceRef::new(
            database,
            uste_types::NamespaceId::from_bytes(read_array(bytes, 8)?),
        );
        let mut references = Vec::new();
        references
            .try_reserve_exact(count)
            .map_err(|_| StorageError::ResourceLimit)?;
        for index in 0..count {
            let offset = INVENTORY_HEADER_BYTES + index * INVENTORY_ENTRY_BYTES;
            if bytes[offset + 28..offset + 32]
                .iter()
                .any(|byte| *byte != 0)
            {
                return Err(StorageError::IntegrityFailure);
            }
            let reference = BlobReference {
                scope,
                id: BlobId(read_array(bytes, offset)?),
                byte_len: u64::from_be_bytes(read_array(bytes, offset + 16)?),
                chunk_count: u32::from_be_bytes(read_array(bytes, offset + 24)?),
                content_digest: read_array(bytes, offset + 32)?,
            };
            if !valid_reference_shape(reference)
                || references
                    .last()
                    .is_some_and(|previous: &BlobReference| previous.id >= reference.id)
            {
                return Err(StorageError::IntegrityFailure);
            }
            references.push(reference);
        }
        let digest = Sha256::digest(bytes).into();
        Ok(Self {
            scope,
            references,
            encoded: bytes.to_vec(),
            digest,
        })
    }

    pub(crate) fn encoded(&self) -> &[u8] {
        &self.encoded
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlobUploadToken {
    scope: NamespaceRef,
    upload: [u8; 16],
    blob: BlobId,
}

impl BlobUploadToken {
    /// Reconstruct an opaque upload token from its persisted random upload identity.
    ///
    /// The blob identity is domain-derived so a second upload identity cannot mint alternate
    /// valid ciphertext for the same final chunk names.
    #[must_use]
    pub fn from_upload_id(scope: NamespaceRef, upload: [u8; 16]) -> Self {
        Self {
            scope,
            upload,
            blob: derive_blob_id(scope, upload),
        }
    }

    #[must_use]
    pub const fn scope(self) -> NamespaceRef {
        self.scope
    }

    #[must_use]
    pub const fn upload_id(self) -> [u8; 16] {
        self.upload
    }

    #[must_use]
    pub const fn blob_id(self) -> BlobId {
        self.blob
    }
}

pub struct BlobUpload {
    token: BlobUploadToken,
    buffer: Vec<u8>,
    durable_chunks: u32,
    durable_bytes: u64,
    hasher: Sha256,
    sealed: bool,
    aborted: bool,
    requires_resume: bool,
    finalized: Option<BlobReference>,
    durable_resume_evidence: bool,
    lease: Option<UploadLease>,
    owner_epoch: KeyEpoch,
    owner_writer: WriterIncarnationId,
}

struct UploadLease(DatabaseId);

impl UploadLease {
    fn acquire(database: DatabaseId) -> Result<Self, StorageError> {
        let mut active = ACTIVE_UPLOADS
            .lock()
            .map_err(|_| StorageError::InvalidState)?;
        let count = active.entry(database).or_default();
        if *count >= MAX_CONCURRENT_UPLOADS {
            return Err(StorageError::ResourceLimit);
        }
        *count += 1;
        Ok(Self(database))
    }
}

impl Drop for UploadLease {
    fn drop(&mut self) {
        let mut active = ACTIVE_UPLOADS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let remove = if let Some(count) = active.get_mut(&self.0) {
            debug_assert!(*count > 0);
            *count -= 1;
            *count == 0
        } else {
            debug_assert!(false, "upload lease database is registered");
            false
        };
        if remove {
            active.remove(&self.0);
        }
    }
}

impl core::fmt::Debug for BlobUpload {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("BlobUpload")
            .field("durable_chunks", &self.durable_chunks)
            .field("durable_bytes", &self.durable_bytes)
            .field("buffered_bytes", &self.buffer.len())
            .field("sealed", &self.sealed)
            .field("aborted", &self.aborted)
            .field("requires_resume", &self.requires_resume)
            .finish_non_exhaustive()
    }
}

impl Drop for BlobUpload {
    fn drop(&mut self) {
        self.buffer.zeroize();
    }
}

impl BlobUpload {
    pub(crate) fn new(
        token: BlobUploadToken,
        owner_epoch: KeyEpoch,
        owner_writer: WriterIncarnationId,
    ) -> Result<Self, StorageError> {
        let lease = UploadLease::acquire(token.scope.database())?;
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(BLOB_CHUNK_BYTES)
            .map_err(|_| StorageError::ResourceLimit)?;
        Ok(Self {
            token,
            buffer,
            durable_chunks: 0,
            durable_bytes: 0,
            hasher: Sha256::new(),
            sealed: false,
            aborted: false,
            requires_resume: false,
            finalized: None,
            durable_resume_evidence: false,
            lease: Some(lease),
            owner_epoch,
            owner_writer,
        })
    }

    #[must_use]
    pub const fn token(&self) -> BlobUploadToken {
        self.token
    }

    #[must_use]
    pub fn accepted_bytes(&self) -> u64 {
        self.durable_bytes + u64::try_from(self.buffer.len()).unwrap_or(u64::MAX)
    }

    #[must_use]
    pub const fn durable_bytes(&self) -> u64 {
        self.durable_bytes
    }

    /// Whether resume authenticated a durable chunk or terminal marker for this token.
    #[must_use]
    pub const fn has_durable_resume_evidence(&self) -> bool {
        self.durable_resume_evidence
    }

    fn release_lease(&mut self) {
        let mut buffer = core::mem::take(&mut self.buffer);
        buffer.zeroize();
        drop(buffer);
        self.lease = None;
    }

    #[cfg(test)]
    pub(crate) fn buffered_capacity(&self) -> usize {
        self.buffer.capacity()
    }

    fn validate_owner(
        &self,
        database: DatabaseId,
        epoch: KeyEpoch,
        writer: WriterIncarnationId,
    ) -> Result<(), StorageError> {
        if self.token.scope.database() != database
            || self.owner_epoch != epoch
            || self.owner_writer != writer
        {
            return Err(StorageError::InvalidState);
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_upload<F, W, E>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &mut KeyVault<W, E>,
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    upload: &mut BlobUpload,
    input: &[u8],
) -> Result<(), StorageError>
where
    F: FileSystem,
    E: EntropySource,
{
    upload.validate_owner(database, epoch, writer)?;
    if upload.requires_resume {
        return Err(StorageError::NeedsRecovery);
    }
    match recover_terminal_state(filesystem, directory, vault, epoch, writer, upload.token)? {
        TerminalState::Aborted => {
            upload.aborted = true;
            upload.release_lease();
            return Err(StorageError::InvalidState);
        }
        TerminalState::Final(reference) => {
            upload.durable_chunks = reference.chunk_count;
            upload.durable_bytes = reference.byte_len;
            upload.sealed = true;
            upload.finalized = Some(reference);
            upload.durable_resume_evidence = true;
            upload.release_lease();
            return Err(StorageError::InvalidState);
        }
        TerminalState::None => {}
    }
    if upload.sealed || upload.aborted {
        return Err(StorageError::InvalidState);
    }
    let new_total = upload
        .accepted_bytes()
        .checked_add(u64::try_from(input.len()).map_err(|_| StorageError::ResourceLimit)?)
        .ok_or(StorageError::ResourceLimit)?;
    if new_total > MAX_BLOB_BYTES {
        return Err(StorageError::ResourceLimit);
    }
    let mut remaining = input;
    while !remaining.is_empty() {
        let available = BLOB_CHUNK_BYTES - upload.buffer.len();
        let take = available.min(remaining.len());
        upload.buffer.extend_from_slice(&remaining[..take]);
        remaining = &remaining[take..];
        if upload.buffer.len() == BLOB_CHUNK_BYTES
            && let Err(error) = flush_buffer(
                filesystem, directory, vault, database, epoch, writer, upload,
            )
        {
            upload.requires_resume = true;
            return Err(error);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn finish_upload<F, W, E>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &mut KeyVault<W, E>,
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    upload: &mut BlobUpload,
) -> Result<BlobReference, StorageError>
where
    F: FileSystem,
    E: EntropySource,
{
    upload.validate_owner(database, epoch, writer)?;
    if upload.requires_resume {
        return Err(StorageError::NeedsRecovery);
    }
    match recover_terminal_state(filesystem, directory, vault, epoch, writer, upload.token)? {
        TerminalState::Aborted => {
            upload.aborted = true;
            upload.release_lease();
            return Err(StorageError::InvalidState);
        }
        TerminalState::Final(reference) => {
            verify_reference(
                filesystem, directory, vault, database, epoch, writer, reference,
            )?;
            upload.durable_chunks = reference.chunk_count;
            upload.durable_bytes = reference.byte_len;
            upload.sealed = true;
            upload.finalized = Some(reference);
            upload.durable_resume_evidence = true;
            upload.release_lease();
            return Ok(reference);
        }
        TerminalState::None => {}
    }
    if upload.aborted {
        return Err(StorageError::InvalidState);
    }
    if let Some(reference) = upload.finalized {
        return Ok(reference);
    }
    if !upload.sealed
        && !upload.buffer.is_empty()
        && let Err(error) = flush_buffer(
            filesystem, directory, vault, database, epoch, writer, upload,
        )
    {
        upload.requires_resume = true;
        return Err(error);
    }
    upload.sealed = true;
    for chunk in 0..upload.durable_chunks {
        let final_name = final_chunk_name(upload.token.blob, chunk)?;
        match filesystem.open_existing(directory, &final_name) {
            Ok(file) => {
                let _ = decrypt_chunk(
                    filesystem,
                    &file,
                    vault,
                    upload.token.scope,
                    epoch,
                    writer,
                    upload.token.blob,
                    chunk,
                )?;
                let staging_name = staging_chunk_name(upload.token.upload, chunk)?;
                match filesystem.open_existing(directory, &staging_name) {
                    Ok(_) => return Err(StorageError::IntegrityFailure),
                    Err(error) if error.kind() == AdapterErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
            Err(error) if error.kind() == AdapterErrorKind::NotFound => {
                filesystem.rename_no_replace(
                    directory,
                    &staging_chunk_name(upload.token.upload, chunk)?,
                    directory,
                    &final_name,
                )?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    filesystem.sync_directory(directory)?;
    let reference = BlobReference {
        scope: upload.token.scope,
        id: upload.token.blob,
        byte_len: upload.durable_bytes,
        chunk_count: upload.durable_chunks,
        content_digest: upload.hasher.clone().finalize().into(),
    };
    publish_manifest(
        filesystem,
        directory,
        vault,
        database,
        epoch,
        writer,
        upload.token,
        Some(reference),
    )?;
    upload.finalized = Some(reference);
    upload.release_lease();
    Ok(reference)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resume_upload<F, W, E>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &mut KeyVault<W, E>,
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    token: BlobUploadToken,
) -> Result<BlobUpload, StorageError>
where
    F: FileSystem,
    E: EntropySource,
{
    if token.scope.database() != database {
        return Err(StorageError::InvalidState);
    }
    match recover_terminal_state(filesystem, directory, vault, epoch, writer, token)? {
        TerminalState::Aborted => return Err(StorageError::InvalidState),
        TerminalState::Final(reference) => {
            verify_reference(
                filesystem, directory, vault, database, epoch, writer, reference,
            )?;
            let mut upload = BlobUpload::new(token, epoch, writer)?;
            upload.durable_chunks = reference.chunk_count;
            upload.durable_bytes = reference.byte_len;
            upload.sealed = true;
            upload.finalized = Some(reference);
            upload.durable_resume_evidence = true;
            upload.release_lease();
            return Ok(upload);
        }
        TerminalState::None => {}
    }
    let mut upload = BlobUpload::new(token, epoch, writer)?;
    loop {
        let chunk = upload.durable_chunks;
        let final_file =
            open_optional(filesystem, directory, &final_chunk_name(token.blob, chunk)?)?;
        let staging_name = staging_chunk_name(token.upload, chunk)?;
        let staging_file = open_optional(filesystem, directory, &staging_name)?;
        let temporary_name = temporary_chunk_name(token.upload, chunk)?;
        if open_optional(filesystem, directory, &temporary_name)?.is_some() {
            filesystem.remove_file(directory, &temporary_name)?;
            filesystem.sync_directory(directory)?;
        }
        let temporary_progress = temporary_progress_name(token, chunk)?;
        if open_optional(filesystem, directory, &temporary_progress)?.is_some() {
            filesystem.remove_file(directory, &temporary_progress)?;
            filesystem.sync_directory(directory)?;
        }
        let progress = load_progress(filesystem, directory, vault, epoch, writer, token, chunk)?;
        if final_file.is_some() && staging_file.is_some() {
            return Err(StorageError::IntegrityFailure);
        }
        let (file, is_final) = match (final_file, staging_file) {
            (Some(file), None) => (file, true),
            (None, Some(file)) => (file, false),
            (None, None) if progress.is_some() => return Err(StorageError::IntegrityFailure),
            (None, None) => break,
            (Some(_), Some(_)) => unreachable!(),
        };
        let plaintext = decrypt_chunk(
            filesystem,
            &file,
            vault,
            token.scope,
            epoch,
            writer,
            token.blob,
            chunk,
        )?;
        if !is_final {
            filesystem.sync_all(&file)?;
            filesystem.sync_directory(directory)?;
        }
        if plaintext.len() > BLOB_CHUNK_BYTES
            || upload
                .durable_bytes
                .checked_add(
                    u64::try_from(plaintext.len()).map_err(|_| StorageError::ResourceLimit)?,
                )
                .is_none_or(|value| value > MAX_BLOB_BYTES)
        {
            return Err(StorageError::IntegrityFailure);
        }
        publish_progress(
            filesystem, directory, vault, database, epoch, writer, token, chunk, &plaintext,
        )?;
        upload.hasher.update(&plaintext);
        upload.durable_bytes +=
            u64::try_from(plaintext.len()).map_err(|_| StorageError::ResourceLimit)?;
        upload.durable_chunks = upload
            .durable_chunks
            .checked_add(1)
            .ok_or(StorageError::ResourceLimit)?;
        upload.sealed |= is_final;
        if plaintext.len() < BLOB_CHUNK_BYTES {
            // Streaming writes publish only full chunks. A durable partial staging chunk can
            // therefore only be the terminal chunk from an interrupted finish operation.
            upload.sealed = true;
            break;
        }
    }
    upload.durable_resume_evidence = upload.durable_chunks != 0;
    Ok(upload)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn abort_upload<F, W, E>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &mut KeyVault<W, E>,
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    upload: &mut BlobUpload,
) -> Result<(), StorageError>
where
    F: FileSystem,
    E: EntropySource,
{
    upload.validate_owner(database, epoch, writer)?;
    if upload.requires_resume {
        return Err(StorageError::NeedsRecovery);
    }
    match recover_terminal_state(filesystem, directory, vault, epoch, writer, upload.token)? {
        TerminalState::Final(reference) => {
            upload.durable_chunks = reference.chunk_count;
            upload.durable_bytes = reference.byte_len;
            upload.sealed = true;
            upload.finalized = Some(reference);
            upload.release_lease();
            return Err(StorageError::InvalidState);
        }
        TerminalState::Aborted => {
            upload.aborted = true;
            upload.hasher = Sha256::new();
            upload.release_lease();
        }
        TerminalState::None => {}
    }
    if upload.sealed {
        return Err(StorageError::InvalidState);
    }
    if !upload.aborted {
        if let Err(error) = publish_manifest(
            filesystem,
            directory,
            vault,
            database,
            epoch,
            writer,
            upload.token,
            None,
        ) {
            upload.requires_resume = true;
            return Err(error);
        }
        upload.aborted = true;
        upload.hasher = Sha256::new();
        upload.release_lease();
    }
    let durable_chunks = upload.durable_chunks;
    for chunk in 0..durable_chunks {
        let name = staging_chunk_name(upload.token.upload, chunk)?;
        match filesystem.remove_file(directory, &name) {
            Ok(()) => {}
            Err(error) if error.kind() == AdapterErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let name = progress_name(upload.token, chunk)?;
        match filesystem.remove_file(directory, &name) {
            Ok(()) => {}
            Err(error) if error.kind() == AdapterErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    filesystem.sync_directory(directory)?;
    upload.durable_chunks = 0;
    upload.durable_bytes = 0;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn verify_reference<F, W, E>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &KeyVault<W, E>,
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    reference: BlobReference,
) -> Result<(), StorageError>
where
    F: FileSystem,
    E: EntropySource,
{
    if reference.scope.database() != database || !valid_reference_shape(reference) {
        return Err(StorageError::IntegrityFailure);
    }
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    for chunk in 0..reference.chunk_count {
        let file = filesystem
            .open_existing(directory, &final_chunk_name(reference.id, chunk)?)
            .map_err(|_| StorageError::IntegrityFailure)?;
        let plaintext = decrypt_chunk(
            filesystem,
            &file,
            vault,
            reference.scope,
            epoch,
            writer,
            reference.id,
            chunk,
        )?;
        let expected = expected_chunk_len(reference, chunk)?;
        if plaintext.len() != expected {
            return Err(StorageError::IntegrityFailure);
        }
        total = total
            .checked_add(u64::try_from(plaintext.len()).map_err(|_| StorageError::ResourceLimit)?)
            .ok_or(StorageError::ResourceLimit)?;
        hasher.update(&plaintext);
    }
    if total != reference.byte_len
        || <[u8; 32]>::from(hasher.finalize()) != reference.content_digest
    {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn read_range<F, W, E>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &KeyVault<W, E>,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    reference: BlobReference,
    offset: u64,
    output: &mut [u8],
) -> Result<usize, StorageError>
where
    F: FileSystem,
    E: EntropySource,
{
    if output.len() > BLOB_CHUNK_BYTES || offset > reference.byte_len {
        return Err(StorageError::ResourceLimit);
    }
    let remaining = reference.byte_len - offset;
    let wanted = usize::try_from(
        remaining.min(u64::try_from(output.len()).map_err(|_| StorageError::ResourceLimit)?),
    )
    .map_err(|_| StorageError::ResourceLimit)?;
    let mut copied = 0_usize;
    let mut position = offset;
    while copied < wanted {
        let chunk = u32::try_from(position / u64::try_from(BLOB_CHUNK_BYTES).unwrap())
            .map_err(|_| StorageError::ResourceLimit)?;
        let within = usize::try_from(position % u64::try_from(BLOB_CHUNK_BYTES).unwrap())
            .map_err(|_| StorageError::ResourceLimit)?;
        let file = filesystem
            .open_existing(directory, &final_chunk_name(reference.id, chunk)?)
            .map_err(|_| StorageError::IntegrityFailure)?;
        let plaintext = decrypt_chunk(
            filesystem,
            &file,
            vault,
            reference.scope,
            epoch,
            writer,
            reference.id,
            chunk,
        )?;
        if plaintext.len() != expected_chunk_len(reference, chunk)? || within >= plaintext.len() {
            return Err(StorageError::IntegrityFailure);
        }
        let take = (wanted - copied).min(plaintext.len() - within);
        output[copied..copied + take].copy_from_slice(&plaintext[within..within + take]);
        copied += take;
        position = position
            .checked_add(u64::try_from(take).map_err(|_| StorageError::ResourceLimit)?)
            .ok_or(StorageError::ResourceLimit)?;
    }
    Ok(copied)
}

pub(crate) fn inventory_context(
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    digest: [u8; 32],
) -> CryptoContext {
    let mut object = [0_u8; 16];
    object.copy_from_slice(&digest[..16]);
    CryptoContext::new(
        database,
        Scope::Database,
        epoch,
        ObjectRole::BlobInventory,
        CryptoObjectId::from_bytes(object),
        0,
        writer,
        BLOB_FORMAT_MAJOR,
        BLOB_FORMAT_MINOR,
        FrameClass::Small4KiB,
    )
}

pub(crate) fn inventory_name<W, E: EntropySource>(
    vault: &KeyVault<W, E>,
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    digest: [u8; 32],
) -> Result<EntryName, StorageError> {
    let context = CryptoContext::new(
        database,
        Scope::Database,
        epoch,
        ObjectRole::BlobInventoryName,
        CryptoObjectId::from_bytes([0; 16]),
        0,
        writer,
        BLOB_FORMAT_MAJOR,
        BLOB_FORMAT_MINOR,
        FrameClass::Small4KiB,
    );
    let opaque = vault.derive_opaque_identifier(context, &digest)?;
    EntryName::new(format!("i-{}", hex(&opaque))).map_err(|_| StorageError::IntegrityFailure)
}

#[allow(clippy::too_many_arguments)]
fn publish_manifest<F, W, E>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &mut KeyVault<W, E>,
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    token: BlobUploadToken,
    reference: Option<BlobReference>,
) -> Result<(), StorageError>
where
    F: FileSystem,
    E: EntropySource,
{
    let state = if reference.is_some() {
        MANIFEST_FINAL
    } else {
        MANIFEST_ABORTED
    };
    publish_manifest_copy(
        filesystem,
        directory,
        vault,
        database,
        epoch,
        writer,
        token,
        state,
        reference,
        &manifest_name(token, state)?,
        &temporary_manifest_name(token, state)?,
    )?;
    publish_manifest_copy(
        filesystem,
        directory,
        vault,
        database,
        epoch,
        writer,
        token,
        state,
        reference,
        &manifest_witness_name(token, state)?,
        &temporary_manifest_witness_name(token, state)?,
    )
}

#[allow(clippy::too_many_arguments)]
fn publish_manifest_copy<F, W, E>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &mut KeyVault<W, E>,
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    token: BlobUploadToken,
    state: u8,
    reference: Option<BlobReference>,
    name: &EntryName,
    temporary_name: &EntryName,
) -> Result<(), StorageError>
where
    F: FileSystem,
    E: EntropySource,
{
    match load_manifest(
        filesystem, directory, vault, epoch, writer, token, state, name,
    ) {
        Ok(Some(existing)) => {
            if state != MANIFEST_ABORTED && reference != Some(existing) {
                return Err(StorageError::IntegrityFailure);
            }
            let file = filesystem.open_existing(directory, name)?;
            filesystem.sync_all(&file)?;
            filesystem.sync_directory(directory)?;
            return Ok(());
        }
        Ok(None) => {}
        Err(error) => return Err(error),
    }
    if open_optional(filesystem, directory, temporary_name)?.is_some() {
        filesystem.remove_file(directory, temporary_name)?;
        filesystem.sync_directory(directory)?;
    }
    let plaintext = encode_manifest(token, reference);
    let encoded = vault
        .encrypt(
            manifest_context(database, epoch, writer, token, state),
            &plaintext,
        )?
        .encode()?;
    let file = filesystem.create_new(directory, temporary_name)?;
    write_all_at(filesystem, &file, 0, &encoded)?;
    filesystem.set_len(
        &file,
        u64::try_from(encoded.len()).map_err(|_| StorageError::ResourceLimit)?,
    )?;
    filesystem.sync_all(&file)?;
    filesystem.sync_directory(directory)?;
    filesystem.rename_no_replace(directory, temporary_name, directory, name)?;
    filesystem.sync_directory(directory)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn load_manifest<F, W, E>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &KeyVault<W, E>,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    token: BlobUploadToken,
    state: u8,
    name: &EntryName,
) -> Result<Option<BlobReference>, StorageError>
where
    F: FileSystem,
    E: EntropySource,
{
    let file = match filesystem.open_existing(directory, name) {
        Ok(file) => file,
        Err(error) if error.kind() == AdapterErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let len = filesystem.metadata(&file)?.len;
    if len != SMALL_ENCRYPTED_OBJECT_BYTES {
        return Err(StorageError::IntegrityFailure);
    }
    let mut encoded = vec![0_u8; usize::try_from(len).map_err(|_| StorageError::ResourceLimit)?];
    read_exact_at(filesystem, &file, 0, &mut encoded)?;
    let envelope =
        EncryptedEnvelope::decode(&encoded).map_err(|_| StorageError::IntegrityFailure)?;
    let plaintext = vault
        .decrypt(
            manifest_context(token.scope.database(), epoch, writer, token, state),
            &envelope,
        )
        .map_err(|_| StorageError::IntegrityFailure)?;
    decode_manifest(token, state, plaintext.as_slice()).map(Some)
}

fn recover_manifest<F, W, E>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &mut KeyVault<W, E>,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    token: BlobUploadToken,
    state: u8,
) -> Result<Option<BlobReference>, StorageError>
where
    F: FileSystem,
    E: EntropySource,
{
    let primary_name = manifest_name(token, state)?;
    let witness_name = manifest_witness_name(token, state)?;
    let primary_temporary = temporary_manifest_name(token, state)?;
    let witness_temporary = temporary_manifest_witness_name(token, state)?;
    for temporary_name in [&primary_temporary, &witness_temporary] {
        if open_optional(filesystem, directory, temporary_name)?.is_some() {
            filesystem.remove_file(directory, temporary_name)?;
            filesystem.sync_directory(directory)?;
        }
    }
    let primary = load_manifest(
        filesystem,
        directory,
        vault,
        epoch,
        writer,
        token,
        state,
        &primary_name,
    )?;
    let witness = load_manifest(
        filesystem,
        directory,
        vault,
        epoch,
        writer,
        token,
        state,
        &witness_name,
    )?;
    let reference = match (primary, witness) {
        (None, None) => return Ok(None),
        (Some(primary), Some(witness)) if primary == witness => Some(primary),
        (Some(reference), None) | (None, Some(reference)) => Some(reference),
        (Some(_), Some(_)) => return Err(StorageError::IntegrityFailure),
    };
    publish_manifest(
        filesystem,
        directory,
        vault,
        token.scope.database(),
        epoch,
        writer,
        token,
        if state == MANIFEST_FINAL {
            reference
        } else {
            None
        },
    )?;
    for name in [&primary_name, &witness_name] {
        let file = filesystem.open_existing(directory, name)?;
        filesystem.sync_all(&file)?;
        filesystem.sync_directory(directory)?;
    }
    Ok(reference)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalState {
    None,
    Final(BlobReference),
    Aborted,
}

#[allow(clippy::too_many_arguments)]
fn recover_terminal_state<F, W, E>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &mut KeyVault<W, E>,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    token: BlobUploadToken,
) -> Result<TerminalState, StorageError>
where
    F: FileSystem,
    E: EntropySource,
{
    let aborted = recover_manifest(
        filesystem,
        directory,
        vault,
        epoch,
        writer,
        token,
        MANIFEST_ABORTED,
    )?
    .is_some();
    let finalized = recover_manifest(
        filesystem,
        directory,
        vault,
        epoch,
        writer,
        token,
        MANIFEST_FINAL,
    )?;
    match (aborted, finalized) {
        (false, None) => Ok(TerminalState::None),
        (true, None) => Ok(TerminalState::Aborted),
        (false, Some(reference)) => Ok(TerminalState::Final(reference)),
        (true, Some(_)) => Err(StorageError::IntegrityFailure),
    }
}

pub(crate) fn encode_manifest(
    token: BlobUploadToken,
    reference: Option<BlobReference>,
) -> [u8; BLOB_MANIFEST_BYTES] {
    let mut bytes = [0_u8; BLOB_MANIFEST_BYTES];
    bytes[..4].copy_from_slice(b"UBMF");
    bytes[4] = BLOB_FORMAT_MAJOR;
    bytes[5] = BLOB_FORMAT_MINOR;
    bytes[6] = if reference.is_some() {
        MANIFEST_FINAL
    } else {
        MANIFEST_ABORTED
    };
    bytes[8..24].copy_from_slice(token.scope.namespace().as_bytes());
    bytes[24..40].copy_from_slice(&token.upload);
    bytes[40..56].copy_from_slice(&token.blob.0);
    if let Some(reference) = reference {
        bytes[56..64].copy_from_slice(&reference.byte_len.to_be_bytes());
        bytes[64..68].copy_from_slice(&reference.chunk_count.to_be_bytes());
        bytes[72..104].copy_from_slice(&reference.content_digest);
    }
    bytes
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BlobProgress {
    chunk: u32,
    plaintext_len: u32,
    plaintext_digest: [u8; 32],
}

fn progress_for(chunk: u32, plaintext: &[u8]) -> Result<BlobProgress, StorageError> {
    Ok(BlobProgress {
        chunk,
        plaintext_len: u32::try_from(plaintext.len()).map_err(|_| StorageError::ResourceLimit)?,
        plaintext_digest: Sha256::digest(plaintext).into(),
    })
}

pub(crate) fn encode_progress_plaintext(
    token: BlobUploadToken,
    chunk: u32,
    plaintext: &[u8],
) -> Result<[u8; BLOB_PROGRESS_BYTES], StorageError> {
    Ok(encode_progress(token, progress_for(chunk, plaintext)?))
}

fn encode_progress(token: BlobUploadToken, progress: BlobProgress) -> [u8; BLOB_PROGRESS_BYTES] {
    let mut bytes = [0_u8; BLOB_PROGRESS_BYTES];
    bytes[..4].copy_from_slice(b"UBPG");
    bytes[4] = BLOB_FORMAT_MAJOR;
    bytes[5] = BLOB_FORMAT_MINOR;
    bytes[8..24].copy_from_slice(token.scope.namespace().as_bytes());
    bytes[24..40].copy_from_slice(&token.upload);
    bytes[40..56].copy_from_slice(&token.blob.0);
    bytes[56..60].copy_from_slice(&progress.chunk.to_be_bytes());
    bytes[60..64].copy_from_slice(&progress.plaintext_len.to_be_bytes());
    bytes[64..96].copy_from_slice(&progress.plaintext_digest);
    bytes
}

fn decode_progress(
    token: BlobUploadToken,
    chunk: u32,
    bytes: &[u8],
) -> Result<BlobProgress, StorageError> {
    if bytes.len() != BLOB_PROGRESS_BYTES
        || &bytes[..4] != b"UBPG"
        || bytes[4] != BLOB_FORMAT_MAJOR
        || bytes[5] != BLOB_FORMAT_MINOR
        || bytes[6..8] != [0, 0]
        || bytes[8..24] != *token.scope.namespace().as_bytes()
        || bytes[24..40] != token.upload
        || bytes[40..56] != token.blob.0
        || bytes[56..60] != chunk.to_be_bytes()
        || bytes[96..].iter().any(|byte| *byte != 0)
    {
        return Err(StorageError::IntegrityFailure);
    }
    let progress = BlobProgress {
        chunk,
        plaintext_len: u32::from_be_bytes(read_array(bytes, 60)?),
        plaintext_digest: read_array(bytes, 64)?,
    };
    if progress.plaintext_len == 0
        || usize::try_from(progress.plaintext_len)
            .ok()
            .is_none_or(|len| len > BLOB_CHUNK_BYTES)
    {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(progress)
}

#[allow(clippy::too_many_arguments)]
fn publish_progress<F, W, E>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &mut KeyVault<W, E>,
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    token: BlobUploadToken,
    chunk: u32,
    plaintext: &[u8],
) -> Result<(), StorageError>
where
    F: FileSystem,
    E: EntropySource,
{
    let expected = progress_for(chunk, plaintext)?;
    if let Some(existing) =
        load_progress(filesystem, directory, vault, epoch, writer, token, chunk)?
    {
        if existing != expected {
            return Err(StorageError::IntegrityFailure);
        }
        let file = filesystem.open_existing(directory, &progress_name(token, chunk)?)?;
        filesystem.sync_all(&file)?;
        filesystem.sync_directory(directory)?;
        return Ok(());
    }
    let temporary_name = temporary_progress_name(token, chunk)?;
    if open_optional(filesystem, directory, &temporary_name)?.is_some() {
        filesystem.remove_file(directory, &temporary_name)?;
        filesystem.sync_directory(directory)?;
    }
    let encoded = vault
        .encrypt(
            progress_context(database, epoch, writer, token, chunk)?,
            &encode_progress_plaintext(token, chunk, plaintext)?,
        )?
        .encode()?;
    let file = filesystem.create_new(directory, &temporary_name)?;
    write_all_at(filesystem, &file, 0, &encoded)?;
    filesystem.set_len(
        &file,
        u64::try_from(encoded.len()).map_err(|_| StorageError::ResourceLimit)?,
    )?;
    filesystem.sync_all(&file)?;
    filesystem.sync_directory(directory)?;
    filesystem.rename_no_replace(
        directory,
        &temporary_name,
        directory,
        &progress_name(token, chunk)?,
    )?;
    filesystem.sync_directory(directory)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn load_progress<F, W, E>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &KeyVault<W, E>,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    token: BlobUploadToken,
    chunk: u32,
) -> Result<Option<BlobProgress>, StorageError>
where
    F: FileSystem,
    E: EntropySource,
{
    let file = match filesystem.open_existing(directory, &progress_name(token, chunk)?) {
        Ok(file) => file,
        Err(error) if error.kind() == AdapterErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let len = filesystem.metadata(&file)?.len;
    if len != SMALL_ENCRYPTED_OBJECT_BYTES {
        return Err(StorageError::IntegrityFailure);
    }
    let mut encoded = vec![0_u8; usize::try_from(len).map_err(|_| StorageError::ResourceLimit)?];
    read_exact_at(filesystem, &file, 0, &mut encoded)?;
    let envelope =
        EncryptedEnvelope::decode(&encoded).map_err(|_| StorageError::IntegrityFailure)?;
    let plaintext = vault
        .decrypt(
            progress_context(token.scope.database(), epoch, writer, token, chunk)?,
            &envelope,
        )
        .map_err(|_| StorageError::IntegrityFailure)?;
    decode_progress(token, chunk, plaintext.as_slice()).map(Some)
}

pub(crate) fn progress_context(
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    token: BlobUploadToken,
    chunk: u32,
) -> Result<CryptoContext, StorageError> {
    let sequence = PROGRESS_SEQUENCE_BASE
        .checked_add(u64::from(chunk))
        .ok_or(StorageError::ResourceLimit)?;
    Ok(CryptoContext::new(
        database,
        Scope::Namespace(token.scope.namespace()),
        epoch,
        ObjectRole::BlobManifest,
        CryptoObjectId::from_bytes(token.upload),
        sequence,
        writer,
        BLOB_FORMAT_MAJOR,
        BLOB_FORMAT_MINOR,
        FrameClass::Small4KiB,
    ))
}

fn decode_manifest(
    token: BlobUploadToken,
    state: u8,
    bytes: &[u8],
) -> Result<BlobReference, StorageError> {
    if bytes.len() != BLOB_MANIFEST_BYTES
        || &bytes[..4] != b"UBMF"
        || bytes[4] != BLOB_FORMAT_MAJOR
        || bytes[5] != BLOB_FORMAT_MINOR
        || bytes[6] != state
        || bytes[7] != 0
        || bytes[8..24] != *token.scope.namespace().as_bytes()
        || bytes[24..40] != token.upload
        || bytes[40..56] != token.blob.0
        || bytes[68..72].iter().any(|byte| *byte != 0)
        || bytes[104..].iter().any(|byte| *byte != 0)
    {
        return Err(StorageError::IntegrityFailure);
    }
    let reference = BlobReference {
        scope: token.scope,
        id: token.blob,
        byte_len: u64::from_be_bytes(read_array(bytes, 56)?),
        chunk_count: u32::from_be_bytes(read_array(bytes, 64)?),
        content_digest: read_array(bytes, 72)?,
    };
    if state == MANIFEST_ABORTED {
        if reference.byte_len != 0
            || reference.chunk_count != 0
            || reference.content_digest != [0; 32]
        {
            return Err(StorageError::IntegrityFailure);
        }
    } else if state != MANIFEST_FINAL || !valid_reference_shape(reference) {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(reference)
}

pub(crate) fn manifest_context(
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    token: BlobUploadToken,
    state: u8,
) -> CryptoContext {
    let object = if state == MANIFEST_FINAL {
        token.blob.0
    } else {
        token.upload
    };
    CryptoContext::new(
        database,
        Scope::Namespace(token.scope.namespace()),
        epoch,
        ObjectRole::BlobManifest,
        CryptoObjectId::from_bytes(object),
        u64::from(state),
        writer,
        BLOB_FORMAT_MAJOR,
        BLOB_FORMAT_MINOR,
        FrameClass::Small4KiB,
    )
}

pub(crate) fn manifest_name(token: BlobUploadToken, state: u8) -> Result<EntryName, StorageError> {
    let (prefix, id) = if state == MANIFEST_FINAL {
        ("m", token.blob.0)
    } else if state == MANIFEST_ABORTED {
        ("a", token.upload)
    } else {
        return Err(StorageError::IntegrityFailure);
    };
    EntryName::new(format!("{prefix}-{}", hex(&id))).map_err(|_| StorageError::IntegrityFailure)
}

fn temporary_manifest_name(token: BlobUploadToken, state: u8) -> Result<EntryName, StorageError> {
    let (prefix, id) = if state == MANIFEST_FINAL {
        ("tm", token.blob.0)
    } else if state == MANIFEST_ABORTED {
        ("ta", token.upload)
    } else {
        return Err(StorageError::IntegrityFailure);
    };
    EntryName::new(format!("{prefix}-{}", hex(&id))).map_err(|_| StorageError::IntegrityFailure)
}

pub(crate) fn manifest_witness_name(
    token: BlobUploadToken,
    state: u8,
) -> Result<EntryName, StorageError> {
    let (prefix, id) = if state == MANIFEST_FINAL {
        ("wm", token.blob.0)
    } else if state == MANIFEST_ABORTED {
        ("wa", token.upload)
    } else {
        return Err(StorageError::IntegrityFailure);
    };
    EntryName::new(format!("{prefix}-{}", hex(&id))).map_err(|_| StorageError::IntegrityFailure)
}

fn temporary_manifest_witness_name(
    token: BlobUploadToken,
    state: u8,
) -> Result<EntryName, StorageError> {
    let (prefix, id) = if state == MANIFEST_FINAL {
        ("twm", token.blob.0)
    } else if state == MANIFEST_ABORTED {
        ("twa", token.upload)
    } else {
        return Err(StorageError::IntegrityFailure);
    };
    EntryName::new(format!("{prefix}-{}", hex(&id))).map_err(|_| StorageError::IntegrityFailure)
}

#[allow(clippy::too_many_arguments)]
fn flush_buffer<F, W, E>(
    filesystem: &mut F,
    directory: &F::Directory,
    vault: &mut KeyVault<W, E>,
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    upload: &mut BlobUpload,
) -> Result<(), StorageError>
where
    F: FileSystem,
    E: EntropySource,
{
    let chunk = upload.durable_chunks;
    let staging_name = staging_chunk_name(upload.token.upload, chunk)?;
    match filesystem.open_existing(directory, &staging_name) {
        Ok(file) => {
            let existing = decrypt_chunk(
                filesystem,
                &file,
                vault,
                upload.token.scope,
                epoch,
                writer,
                upload.token.blob,
                chunk,
            )?;
            if existing != upload.buffer {
                return Err(StorageError::IntegrityFailure);
            }
            filesystem.sync_all(&file)?;
            filesystem.sync_directory(directory)?;
        }
        Err(error) if error.kind() == AdapterErrorKind::NotFound => {
            let temporary_name = temporary_chunk_name(upload.token.upload, chunk)?;
            match filesystem.remove_file(directory, &temporary_name) {
                Ok(()) => filesystem.sync_directory(directory)?,
                Err(error) if error.kind() == AdapterErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            let encoded = vault
                .encrypt(
                    chunk_context(
                        database,
                        upload.token.scope,
                        epoch,
                        writer,
                        upload.token.blob,
                        chunk,
                    ),
                    &upload.buffer,
                )?
                .encode()?;
            let file = filesystem.create_new(directory, &temporary_name)?;
            write_all_at(filesystem, &file, 0, &encoded)?;
            filesystem.set_len(
                &file,
                u64::try_from(encoded.len()).map_err(|_| StorageError::ResourceLimit)?,
            )?;
            filesystem.sync_all(&file)?;
            filesystem.sync_directory(directory)?;
            filesystem.rename_no_replace(directory, &temporary_name, directory, &staging_name)?;
            filesystem.sync_directory(directory)?;
        }
        Err(error) => return Err(error.into()),
    }
    publish_progress(
        filesystem,
        directory,
        vault,
        database,
        epoch,
        writer,
        upload.token,
        chunk,
        &upload.buffer,
    )?;
    update_upload_after_flush(upload)
}

fn update_upload_after_flush(upload: &mut BlobUpload) -> Result<(), StorageError> {
    upload.hasher.update(&upload.buffer);
    upload.durable_bytes = upload
        .durable_bytes
        .checked_add(u64::try_from(upload.buffer.len()).map_err(|_| StorageError::ResourceLimit)?)
        .ok_or(StorageError::ResourceLimit)?;
    upload.durable_chunks = upload
        .durable_chunks
        .checked_add(1)
        .ok_or(StorageError::ResourceLimit)?;
    upload.buffer.clear();
    Ok(())
}

fn derive_blob_id(scope: NamespaceRef, upload: [u8; 16]) -> BlobId {
    let mut hasher = Sha256::new();
    hasher.update(b"USTE-BLOB-ID-V1\0");
    hasher.update(scope.database().as_bytes());
    hasher.update(scope.namespace().as_bytes());
    hasher.update(upload);
    let digest: [u8; 32] = hasher.finalize().into();
    let mut id = [0_u8; 16];
    id.copy_from_slice(&digest[..16]);
    BlobId(id)
}

#[allow(clippy::too_many_arguments)]
fn decrypt_chunk<F, W, E>(
    filesystem: &mut F,
    file: &F::File,
    vault: &KeyVault<W, E>,
    scope: NamespaceRef,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    blob: BlobId,
    chunk: u32,
) -> Result<Vec<u8>, StorageError>
where
    F: FileSystem,
    E: EntropySource,
{
    let len = filesystem.metadata(file)?.len;
    if len == 0 || len > MAX_ENCODED_CHUNK_BYTES {
        return Err(StorageError::IntegrityFailure);
    }
    let len_usize = usize::try_from(len).map_err(|_| StorageError::ResourceLimit)?;
    let mut encoded = vec![0_u8; len_usize];
    read_exact_at(filesystem, file, 0, &mut encoded)?;
    let envelope =
        EncryptedEnvelope::decode(&encoded).map_err(|_| StorageError::IntegrityFailure)?;
    let plaintext = vault
        .decrypt(
            chunk_context(scope.database(), scope, epoch, writer, blob, chunk),
            &envelope,
        )
        .map_err(|_| StorageError::IntegrityFailure)?;
    Ok(plaintext.as_slice().to_vec())
}

fn chunk_context(
    database: DatabaseId,
    scope: NamespaceRef,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    blob: BlobId,
    chunk: u32,
) -> CryptoContext {
    CryptoContext::new(
        database,
        Scope::Namespace(scope.namespace()),
        epoch,
        ObjectRole::BlobChunk,
        CryptoObjectId::from_bytes(blob.0),
        u64::from(chunk),
        writer,
        BLOB_FORMAT_MAJOR,
        BLOB_FORMAT_MINOR,
        FrameClass::Blob64KiB,
    )
}

fn valid_reference_shape(reference: BlobReference) -> bool {
    if reference.byte_len > MAX_BLOB_BYTES || reference.id.0 == [0; 16] {
        return false;
    }
    let expected = if reference.byte_len == 0 {
        0
    } else {
        ((reference.byte_len - 1) / u64::try_from(BLOB_CHUNK_BYTES).unwrap() + 1) as u32
    };
    reference.chunk_count == expected
}

fn expected_chunk_len(reference: BlobReference, chunk: u32) -> Result<usize, StorageError> {
    if chunk >= reference.chunk_count {
        return Err(StorageError::IntegrityFailure);
    }
    let start = u64::from(chunk)
        .checked_mul(u64::try_from(BLOB_CHUNK_BYTES).unwrap())
        .ok_or(StorageError::ResourceLimit)?;
    usize::try_from((reference.byte_len - start).min(u64::try_from(BLOB_CHUNK_BYTES).unwrap()))
        .map_err(|_| StorageError::ResourceLimit)
}

fn encode_inventory(
    scope: NamespaceRef,
    references: &[BlobReference],
) -> Result<Vec<u8>, StorageError> {
    let length = INVENTORY_HEADER_BYTES
        .checked_add(
            references
                .len()
                .checked_mul(INVENTORY_ENTRY_BYTES)
                .ok_or(StorageError::ResourceLimit)?,
        )
        .ok_or(StorageError::ResourceLimit)?;
    let mut bytes = vec![0_u8; length];
    bytes[..4].copy_from_slice(b"UBIN");
    bytes[4] = BLOB_FORMAT_MAJOR;
    bytes[5] = BLOB_FORMAT_MINOR;
    bytes[8..24].copy_from_slice(scope.namespace().as_bytes());
    bytes[24..28].copy_from_slice(
        &u32::try_from(references.len())
            .map_err(|_| StorageError::ResourceLimit)?
            .to_be_bytes(),
    );
    for (index, reference) in references.iter().enumerate() {
        let offset = INVENTORY_HEADER_BYTES + index * INVENTORY_ENTRY_BYTES;
        bytes[offset..offset + 16].copy_from_slice(&reference.id.0);
        bytes[offset + 16..offset + 24].copy_from_slice(&reference.byte_len.to_be_bytes());
        bytes[offset + 24..offset + 28].copy_from_slice(&reference.chunk_count.to_be_bytes());
        bytes[offset + 32..offset + 64].copy_from_slice(&reference.content_digest);
    }
    Ok(bytes)
}

pub(crate) fn staging_chunk_name(upload: [u8; 16], chunk: u32) -> Result<EntryName, StorageError> {
    EntryName::new(format!("u-{}-{chunk:08x}", hex(&upload)))
        .map_err(|_| StorageError::IntegrityFailure)
}

fn temporary_chunk_name(upload: [u8; 16], chunk: u32) -> Result<EntryName, StorageError> {
    EntryName::new(format!("t-{}-{chunk:08x}", hex(&upload)))
        .map_err(|_| StorageError::IntegrityFailure)
}

pub(crate) fn progress_name(token: BlobUploadToken, chunk: u32) -> Result<EntryName, StorageError> {
    EntryName::new(format!("p-{}-{chunk:08x}", hex(&token.upload)))
        .map_err(|_| StorageError::IntegrityFailure)
}

fn temporary_progress_name(token: BlobUploadToken, chunk: u32) -> Result<EntryName, StorageError> {
    EntryName::new(format!("q-{}-{chunk:08x}", hex(&token.upload)))
        .map_err(|_| StorageError::IntegrityFailure)
}

pub(crate) fn final_chunk_name(blob: BlobId, chunk: u32) -> Result<EntryName, StorageError> {
    EntryName::new(format!("b-{}-{chunk:08x}", hex(&blob.0)))
        .map_err(|_| StorageError::IntegrityFailure)
}

fn open_optional<F: FileSystem>(
    filesystem: &mut F,
    directory: &F::Directory,
    name: &EntryName,
) -> Result<Option<F::File>, StorageError> {
    match filesystem.open_existing(directory, name) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == AdapterErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], StorageError> {
    bytes
        .get(offset..offset.checked_add(N).ok_or(StorageError::ResourceLimit)?)
        .ok_or(StorageError::IntegrityFailure)?
        .try_into()
        .map_err(|_| StorageError::IntegrityFailure)
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::memory::MemoryFileSystem;
    use uste_types::{DatabaseId, NamespaceId};

    #[derive(Debug)]
    struct NoEntropy;

    impl EntropySource for NoEntropy {
        fn fill(&mut self, _output: &mut [u8]) -> Result<(), uste_crypto::EntropyFailure> {
            Err(uste_crypto::EntropyFailure)
        }
    }

    fn scope() -> NamespaceRef {
        NamespaceRef::new(
            DatabaseId::from_bytes([1; 16]),
            NamespaceId::from_bytes([2; 16]),
        )
    }

    fn reference() -> BlobReference {
        BlobReference {
            scope: scope(),
            id: BlobId::from_bytes([3; 16]),
            byte_len: u64::try_from(BLOB_CHUNK_BYTES).unwrap() + 7,
            chunk_count: 2,
            content_digest: [4; 32],
        }
    }

    fn golden() -> Vec<u8> {
        include_str!("../../../acceptance/r1/blob-inventory-v1.hex")
            .trim()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    fn manifest_golden() -> Vec<u8> {
        include_str!("../../../acceptance/r1/blob-manifest-v1.hex")
            .trim()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    fn progress_golden() -> Vec<u8> {
        include_str!("../../../acceptance/r1/blob-progress-v1.hex")
            .trim()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let text = core::str::from_utf8(pair).unwrap();
                u8::from_str_radix(text, 16).unwrap()
            })
            .collect()
    }

    #[test]
    fn literal_manifest_golden_and_assigned_roles_are_exact() {
        assert_eq!(ObjectRole::BlobInventory as u8, 0x0a);
        assert_eq!(ObjectRole::BlobManifest as u8, 0x0b);
        assert_eq!(ObjectRole::BlobInventoryName as u8, 0x0c);
        let scope = NamespaceRef::new(
            DatabaseId::from_bytes([0x11; 16]),
            NamespaceId::from_bytes([0x22; 16]),
        );
        let token = BlobUploadToken::from_upload_id(scope, [0x33; 16]);
        assert_eq!(
            token.blob_id().as_bytes(),
            [
                0x58, 0xf9, 0x0d, 0x4c, 0x94, 0xeb, 0xad, 0x66, 0x10, 0x4a, 0xd1, 0xec, 0xdc, 0xbe,
                0x59, 0xda,
            ]
        );
        let reference = BlobReference {
            scope,
            id: token.blob_id(),
            byte_len: 5,
            chunk_count: 1,
            content_digest: [0x44; 32],
        };
        assert_eq!(
            encode_manifest(token, Some(reference)).as_slice(),
            manifest_golden()
        );
        assert_eq!(
            decode_manifest(token, MANIFEST_FINAL, &manifest_golden()).unwrap(),
            reference
        );
        let progress = progress_for(0, b"hello").unwrap();
        assert_eq!(
            encode_progress(token, progress).as_slice(),
            progress_golden()
        );
        assert_eq!(
            decode_progress(token, 0, &progress_golden()).unwrap(),
            progress
        );
        let other_scope = NamespaceRef::new(
            DatabaseId::from_bytes([0x11; 16]),
            NamespaceId::from_bytes([0x23; 16]),
        );
        assert_ne!(
            token.blob_id(),
            BlobUploadToken::from_upload_id(other_scope, [0x33; 16]).blob_id()
        );
    }

    #[test]
    fn inventory_iterator_stops_at_cap_plus_one_without_unbounded_collection() {
        let pulls = Cell::new(0_usize);
        let iterator = core::iter::from_fn(|| {
            pulls.set(pulls.get() + 1);
            Some(reference())
        });
        assert_eq!(
            BlobInventory::new(scope(), iterator).unwrap_err(),
            StorageError::ResourceLimit
        );
        assert_eq!(pulls.get(), MAX_BLOBS_PER_INVENTORY + 1);
    }

    #[test]
    fn single_blob_cap_is_inclusive_and_rejects_one_more_byte_before_io() {
        let token = BlobUploadToken::from_upload_id(scope(), [0x55; 16]);
        let mut upload = BlobUpload::new(
            token,
            KeyEpoch::FIRST,
            WriterIncarnationId::from_bytes([0x56; 16]),
        )
        .unwrap();
        upload.durable_bytes = MAX_BLOB_BYTES - 1;
        let mut filesystem = MemoryFileSystem::default();
        let directory = filesystem.root();
        let mut vault = KeyVault::from_locked(scope().database(), (), NoEntropy);
        write_upload(
            &mut filesystem,
            &directory,
            &mut vault,
            scope().database(),
            KeyEpoch::FIRST,
            WriterIncarnationId::from_bytes([0x56; 16]),
            &mut upload,
            b"x",
        )
        .unwrap();
        assert_eq!(upload.accepted_bytes(), MAX_BLOB_BYTES);
        assert_eq!(
            write_upload(
                &mut filesystem,
                &directory,
                &mut vault,
                scope().database(),
                KeyEpoch::FIRST,
                WriterIncarnationId::from_bytes([0x56; 16]),
                &mut upload,
                b"y",
            )
            .unwrap_err(),
            StorageError::ResourceLimit
        );
    }

    #[test]
    fn literal_inventory_golden_and_empty_digest_are_exact() {
        let inventory = BlobInventory::new(scope(), [reference()]).unwrap();
        assert_eq!(inventory.encoded(), golden());
        assert_eq!(
            inventory.digest(),
            [
                0x3c, 0xc1, 0xe1, 0xca, 0x03, 0x6b, 0x71, 0x55, 0x15, 0x20, 0xce, 0xf6, 0xd8, 0x70,
                0x8e, 0xd3, 0xf2, 0xe8, 0xf9, 0x4b, 0x03, 0x2e, 0x9a, 0x00, 0xbf, 0xf3, 0x07, 0xda,
                0xbf, 0x6f, 0x6e, 0x87,
            ]
        );
        assert_eq!(
            BlobInventory::decode(scope().database(), inventory.encoded()).unwrap(),
            inventory
        );
        assert_eq!(
            BlobInventory::new(scope(), []).unwrap().digest(),
            EMPTY_BLOB_INVENTORY_DIGEST
        );
    }

    #[test]
    fn malformed_and_noncanonical_inventories_fail_closed() {
        let valid = golden();
        for offset in [0, 4, 5, 6, 24, 28, 31, 48, 56, 60] {
            let mut bytes = valid.clone();
            bytes[offset] ^= 0x80;
            assert!(BlobInventory::decode(scope().database(), &bytes).is_err());
        }
        assert!(BlobInventory::decode(scope().database(), &valid[..valid.len() - 1]).is_err());
        assert_eq!(
            BlobInventory::new(scope(), [reference(), reference()]).unwrap_err(),
            StorageError::InvalidState
        );
    }
}
