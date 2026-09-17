//! Optional encrypted two-slot checkpoint cache owned by the journal writer.

use sha2::{Digest, Sha256};
use uste_crypto::{
    CryptoContext, CryptoError, CryptoObjectId, EncryptedEnvelope, EntropySource, FrameClass,
    KeyEpoch, KeyVault, ObjectRole, Scope, WriterIncarnationId,
};
use uste_types::{CommitRevision, DatabaseId, NamespaceId, NamespaceRef};

use crate::{
    AdapterErrorKind, EntryName, FileSystem,
    journal::{DurableKeyEnvelope, StorageError},
    read_exact_at, write_all_at,
};

pub const CHECKPOINT_CHUNK_BYTES: usize = 1024 * 1024;
pub const MAX_CHECKPOINT_BYTES: usize = 256 * 1024 * 1024;
const MAX_CHECKPOINT_CHUNKS: usize = MAX_CHECKPOINT_BYTES / CHECKPOINT_CHUNK_BYTES;
const MAX_ENCODED_CHUNK_BYTES: u64 = 2 * 1024 * 1024;
const MANIFEST_BYTES: usize = 208;
const SMALL_ENVELOPE_BYTES: usize = 4_161;
const MANIFEST_FILE_BYTES: u64 = 16 + SMALL_ENVELOPE_BYTES as u64;
const MAGIC: &[u8; 4] = b"UCKP";
const MAJOR: u8 = 1;
const MINOR: u8 = 0;

#[derive(Clone, Copy)]
pub struct CheckpointInput<'a> {
    pub scope: NamespaceRef,
    pub revision: CommitRevision,
    pub certificate_digest: [u8; 32],
    pub reducer_profile: [u8; 32],
    pub logical_state_digest: [u8; 32],
    pub payload: &'a [u8],
}

/// Metadata for a bounded checkpoint payload supplied incrementally.
#[derive(Clone, Copy)]
pub struct CheckpointStreamInput {
    pub scope: NamespaceRef,
    pub revision: CommitRevision,
    pub certificate_digest: [u8; 32],
    pub reducer_profile: [u8; 32],
    pub logical_state_digest: [u8; 32],
    pub payload_len: u64,
}

impl core::fmt::Debug for CheckpointStreamInput {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("CheckpointStreamInput")
            .field("scope", &"[REDACTED]")
            .field("revision", &self.revision)
            .field("certificate_digest", &"[REDACTED]")
            .field("reducer_profile", &"[REDACTED]")
            .field("logical_state_digest", &"[REDACTED]")
            .field("payload_len", &self.payload_len)
            .finish()
    }
}

impl core::fmt::Debug for CheckpointInput<'_> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("CheckpointInput")
            .field("scope", &"[REDACTED]")
            .field("revision", &self.revision)
            .field("certificate_digest", &"[REDACTED]")
            .field("reducer_profile", &"[REDACTED]")
            .field("logical_state_digest", &"[REDACTED]")
            .field("payload_len", &self.payload.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DurableCheckpoint {
    pub revision: CommitRevision,
    pub generation: u64,
}

#[derive(Eq, PartialEq)]
pub struct RecoveredCheckpoint {
    scope: NamespaceRef,
    revision: CommitRevision,
    generation: u64,
    certificate_digest: [u8; 32],
    reducer_profile: [u8; 32],
    logical_state_digest: [u8; 32],
    payload: Vec<u8>,
}

impl core::fmt::Debug for RecoveredCheckpoint {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("RecoveredCheckpoint")
            .field("scope", &"[REDACTED]")
            .field("revision", &self.revision)
            .field("generation", &self.generation)
            .field("certificate_digest", &"[REDACTED]")
            .field("reducer_profile", &"[REDACTED]")
            .field("logical_state_digest", &"[REDACTED]")
            .field("payload_len", &self.payload.len())
            .finish()
    }
}

impl RecoveredCheckpoint {
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
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

#[derive(Clone, Copy)]
struct Manifest {
    scope: NamespaceRef,
    revision: CommitRevision,
    generation: u64,
    certificate_digest: [u8; 32],
    reducer_profile: [u8; 32],
    logical_state_digest: [u8; 32],
    payload_digest: [u8; 32],
    payload_len: u64,
    chunk_count: u32,
    object_id: [u8; 16],
}

pub(crate) struct CheckpointContext<'a, D> {
    pub database: DatabaseId,
    pub epoch: KeyEpoch,
    pub writer: WriterIncarnationId,
    pub directory: &'a D,
}

pub(crate) fn publish<F, W, E, I>(
    filesystem: &mut F,
    context: CheckpointContext<'_, F::Directory>,
    vault: &mut KeyVault<W, E>,
    identity_entropy: &mut I,
    input: CheckpointInput<'_>,
) -> Result<DurableCheckpoint, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let payload_len =
        u64::try_from(input.payload.len()).map_err(|_| StorageError::ResourceLimit)?;
    publish_stream(
        filesystem,
        context,
        vault,
        identity_entropy,
        CheckpointStreamInput {
            scope: input.scope,
            revision: input.revision,
            certificate_digest: input.certificate_digest,
            reducer_profile: input.reducer_profile,
            logical_state_digest: input.logical_state_digest,
            payload_len,
        },
        |sink| sink(input.payload),
    )
}

pub(crate) fn publish_stream<F, W, E, I, P>(
    filesystem: &mut F,
    context: CheckpointContext<'_, F::Directory>,
    vault: &mut KeyVault<W, E>,
    identity_entropy: &mut I,
    input: CheckpointStreamInput,
    producer: P,
) -> Result<DurableCheckpoint, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
    P: FnOnce(&mut dyn FnMut(&[u8]) -> Result<(), StorageError>) -> Result<(), StorageError>,
{
    let payload_len =
        usize::try_from(input.payload_len).map_err(|_| StorageError::ResourceLimit)?;
    if input.scope.database() != context.database
        || payload_len == 0
        || payload_len > MAX_CHECKPOINT_BYTES
    {
        return Err(StorageError::InvalidState);
    }
    let candidates = load_candidates(filesystem, &context, vault, input.scope)?;
    let generation = candidates
        .iter()
        .map(|(_, checkpoint)| checkpoint.generation)
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
    invalidate_slot(filesystem, context.directory, slot)?;

    let object_id = random_nonzero_id(identity_entropy)?;
    let chunk_count = payload_len.div_ceil(CHECKPOINT_CHUNK_BYTES);
    let chunk_count_u32 = u32::try_from(chunk_count).map_err(|_| StorageError::ResourceLimit)?;
    if chunk_count == 0 || chunk_count > MAX_CHECKPOINT_CHUNKS {
        return Err(StorageError::ResourceLimit);
    }
    let mut pending = Vec::new();
    pending
        .try_reserve_exact(CHECKPOINT_CHUNK_BYTES)
        .map_err(|_| StorageError::ResourceLimit)?;
    let mut payload_digest = Sha256::new();
    let mut actual_len = 0_usize;
    let mut written_chunks = 0_usize;
    let mut sink_error = None;
    let producer_result = {
        let mut sink = |mut bytes: &[u8]| -> Result<(), StorageError> {
            if let Some(error) = sink_error {
                return Err(error);
            }
            let next = actual_len
                .checked_add(bytes.len())
                .ok_or(StorageError::ResourceLimit);
            let next = match next {
                Ok(next) => next,
                Err(error) => {
                    sink_error = Some(error);
                    return Err(error);
                }
            };
            if next > payload_len {
                sink_error = Some(StorageError::InvalidState);
                return Err(StorageError::InvalidState);
            }
            actual_len = next;
            payload_digest.update(bytes);
            while !bytes.is_empty() {
                let count = (CHECKPOINT_CHUNK_BYTES - pending.len()).min(bytes.len());
                pending.extend_from_slice(&bytes[..count]);
                bytes = &bytes[count..];
                if pending.len() == CHECKPOINT_CHUNK_BYTES {
                    if let Err(error) = publish_chunk(
                        filesystem,
                        &context,
                        vault,
                        input.scope,
                        slot,
                        object_id,
                        written_chunks,
                        &pending,
                    ) {
                        sink_error = Some(error);
                        return Err(error);
                    }
                    written_chunks = match written_chunks
                        .checked_add(1)
                        .ok_or(StorageError::ResourceLimit)
                    {
                        Ok(written_chunks) => written_chunks,
                        Err(error) => {
                            sink_error = Some(error);
                            return Err(error);
                        }
                    };
                    pending.clear();
                }
            }
            Ok(())
        };
        producer(&mut sink)
    };
    if let Some(error) = sink_error {
        return Err(error);
    }
    producer_result?;
    if actual_len != payload_len {
        return Err(StorageError::InvalidState);
    }
    if !pending.is_empty() {
        publish_chunk(
            filesystem,
            &context,
            vault,
            input.scope,
            slot,
            object_id,
            written_chunks,
            &pending,
        )?;
        written_chunks = written_chunks
            .checked_add(1)
            .ok_or(StorageError::ResourceLimit)?;
    }
    if written_chunks != chunk_count {
        return Err(StorageError::IntegrityFailure);
    }
    // Chunk names and bytes precede the terminal manifest in the durable directory order.
    filesystem.sync_directory(context.directory)?;

    let manifest = Manifest {
        scope: input.scope,
        revision: input.revision,
        generation,
        certificate_digest: input.certificate_digest,
        reducer_profile: input.reducer_profile,
        logical_state_digest: input.logical_state_digest,
        payload_digest: payload_digest.finalize().into(),
        payload_len: input.payload_len,
        chunk_count: chunk_count_u32,
        object_id,
    };
    let encoded_manifest = vault
        .encrypt(
            checkpoint_context(
                &context,
                input.scope.namespace(),
                object_id,
                0,
                FrameClass::Small4KiB,
            ),
            &manifest.encode(),
        )?
        .encode()?;
    if encoded_manifest.len() != SMALL_ENVELOPE_BYTES {
        return Err(StorageError::IntegrityFailure);
    }
    let manifest_name = manifest_name(slot);
    let file = filesystem.create_new(context.directory, &manifest_name)?;
    write_all_at(filesystem, &file, 0, &object_id)?;
    write_all_at(filesystem, &file, 16, &encoded_manifest)?;
    filesystem.set_len(&file, MANIFEST_FILE_BYTES)?;
    filesystem.sync_all(&file)?;
    filesystem.sync_directory(context.directory)?;
    Ok(DurableCheckpoint {
        revision: input.revision,
        generation,
    })
}

pub(crate) fn load<F, W, E>(
    filesystem: &mut F,
    context: CheckpointContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    scope: NamespaceRef,
) -> Vec<RecoveredCheckpoint>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    load_candidates(filesystem, &context, vault, scope)
        .unwrap_or_default()
        .into_iter()
        .map(|(_, checkpoint)| checkpoint)
        .collect()
}

fn load_candidates<F, W, E>(
    filesystem: &mut F,
    context: &CheckpointContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    scope: NamespaceRef,
) -> Result<Vec<(Slot, RecoveredCheckpoint)>, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    let mut candidates = [Slot::A, Slot::B]
        .into_iter()
        .filter_map(|slot| {
            load_slot(filesystem, context, vault, scope, slot)
                .ok()
                .flatten()
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| core::cmp::Reverse(candidate.1.generation));
    if candidates.len() == 2
        && candidates[0].1.generation == candidates[1].1.generation
        && candidates[0].1 != candidates[1].1
    {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(candidates)
}

fn load_slot<F, W, E>(
    filesystem: &mut F,
    context: &CheckpointContext<'_, F::Directory>,
    vault: &KeyVault<W, E>,
    expected_scope: NamespaceRef,
    slot: Slot,
) -> Result<Option<(Slot, RecoveredCheckpoint)>, StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    let file = match filesystem.open_existing(context.directory, &manifest_name(slot)) {
        Ok(file) => file,
        Err(error) if error.kind() == AdapterErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if filesystem.metadata(&file)?.len != MANIFEST_FILE_BYTES {
        return Err(StorageError::IntegrityFailure);
    }
    let mut object_id = [0_u8; 16];
    read_exact_at(filesystem, &file, 0, &mut object_id)?;
    if object_id == [0; 16] {
        return Err(StorageError::IntegrityFailure);
    }
    let mut encoded = vec![0_u8; SMALL_ENVELOPE_BYTES];
    read_exact_at(filesystem, &file, 16, &mut encoded)?;
    let envelope = EncryptedEnvelope::decode(&encoded).map_err(cache_crypto_error)?;
    let plaintext = vault
        .decrypt(
            checkpoint_context(
                context,
                expected_scope.namespace(),
                object_id,
                0,
                FrameClass::Small4KiB,
            ),
            &envelope,
        )
        .map_err(cache_crypto_error)?;
    let manifest = Manifest::decode(plaintext.as_slice(), context.database)?;
    if manifest.scope != expected_scope || manifest.object_id != object_id {
        return Err(StorageError::IntegrityFailure);
    }
    let (payload_len, chunk_count) = validate_manifest_shape(manifest)?;
    let mut payload = Vec::new();
    payload
        .try_reserve_exact(payload_len)
        .map_err(|_| StorageError::ResourceLimit)?;
    for index in 0..chunk_count {
        let chunk_file = filesystem.open_existing(context.directory, &chunk_name(slot, index)?)?;
        let length = filesystem.metadata(&chunk_file)?.len;
        if length == 0 || length > MAX_ENCODED_CHUNK_BYTES {
            return Err(StorageError::IntegrityFailure);
        }
        let length_usize = usize::try_from(length).map_err(|_| StorageError::ResourceLimit)?;
        let mut chunk_encoded = vec![0_u8; length_usize];
        read_exact_at(filesystem, &chunk_file, 0, &mut chunk_encoded)?;
        let chunk_envelope =
            EncryptedEnvelope::decode(&chunk_encoded).map_err(cache_crypto_error)?;
        let sequence = u64::try_from(index)
            .map_err(|_| StorageError::ResourceLimit)?
            .checked_add(1)
            .ok_or(StorageError::ResourceLimit)?;
        let chunk = vault
            .decrypt(
                checkpoint_context(
                    context,
                    expected_scope.namespace(),
                    object_id,
                    sequence,
                    FrameClass::Blob64KiB,
                ),
                &chunk_envelope,
            )
            .map_err(cache_crypto_error)?;
        let expected = if index + 1 == chunk_count {
            payload_len - index * CHECKPOINT_CHUNK_BYTES
        } else {
            CHECKPOINT_CHUNK_BYTES
        };
        if chunk.as_slice().len() != expected {
            return Err(StorageError::IntegrityFailure);
        }
        payload.extend_from_slice(chunk.as_slice());
    }
    let actual_payload_digest: [u8; 32] = Sha256::digest(&payload).into();
    if payload.len() != payload_len || actual_payload_digest != manifest.payload_digest {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(Some((
        slot,
        RecoveredCheckpoint {
            scope: manifest.scope,
            revision: manifest.revision,
            generation: manifest.generation,
            certificate_digest: manifest.certificate_digest,
            reducer_profile: manifest.reducer_profile,
            logical_state_digest: manifest.logical_state_digest,
            payload,
        },
    )))
}

fn validate_manifest_shape(manifest: Manifest) -> Result<(usize, usize), StorageError> {
    let payload_len =
        usize::try_from(manifest.payload_len).map_err(|_| StorageError::ResourceLimit)?;
    let chunk_count =
        usize::try_from(manifest.chunk_count).map_err(|_| StorageError::ResourceLimit)?;
    if payload_len == 0
        || payload_len > MAX_CHECKPOINT_BYTES
        || chunk_count == 0
        || chunk_count > MAX_CHECKPOINT_CHUNKS
        || payload_len.div_ceil(CHECKPOINT_CHUNK_BYTES) != chunk_count
    {
        return Err(StorageError::IntegrityFailure);
    }
    Ok((payload_len, chunk_count))
}

impl Manifest {
    fn encode(self) -> [u8; MANIFEST_BYTES] {
        let mut bytes = [0_u8; MANIFEST_BYTES];
        bytes[..4].copy_from_slice(MAGIC);
        bytes[4] = MAJOR;
        bytes[5] = MINOR;
        bytes[8..24].copy_from_slice(self.scope.namespace().as_bytes());
        bytes[24..32].copy_from_slice(&self.revision.get().to_be_bytes());
        bytes[32..40].copy_from_slice(&self.generation.to_be_bytes());
        bytes[40..72].copy_from_slice(&self.certificate_digest);
        bytes[72..104].copy_from_slice(&self.reducer_profile);
        bytes[104..136].copy_from_slice(&self.logical_state_digest);
        bytes[136..168].copy_from_slice(&self.payload_digest);
        bytes[168..176].copy_from_slice(&self.payload_len.to_be_bytes());
        bytes[176..180].copy_from_slice(&self.chunk_count.to_be_bytes());
        bytes[184..200].copy_from_slice(&self.object_id);
        bytes
    }

    fn decode(bytes: &[u8], database: DatabaseId) -> Result<Self, StorageError> {
        if bytes.len() != MANIFEST_BYTES
            || &bytes[..4] != MAGIC
            || bytes[4] != MAJOR
            || bytes[5] != MINOR
            || bytes[6..8] != [0; 2]
            || bytes[180..184] != [0; 4]
            || bytes[200..].iter().any(|byte| *byte != 0)
        {
            return Err(StorageError::IntegrityFailure);
        }
        let revision = CommitRevision::new(read_u64(bytes, 24)?)
            .map_err(|_| StorageError::IntegrityFailure)?;
        let generation = read_u64(bytes, 32)?;
        if generation == 0 {
            return Err(StorageError::IntegrityFailure);
        }
        let object_id = read_array(bytes, 184)?;
        if object_id == [0; 16] {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(Self {
            scope: NamespaceRef::new(database, NamespaceId::from_bytes(read_array(bytes, 8)?)),
            revision,
            generation,
            certificate_digest: read_array(bytes, 40)?,
            reducer_profile: read_array(bytes, 72)?,
            logical_state_digest: read_array(bytes, 104)?,
            payload_digest: read_array(bytes, 136)?,
            payload_len: read_u64(bytes, 168)?,
            chunk_count: u32::from_be_bytes(read_array(bytes, 176)?),
            object_id,
        })
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

    const fn label(self) -> char {
        match self {
            Self::A => 'A',
            Self::B => 'B',
        }
    }
}

fn manifest_name(slot: Slot) -> EntryName {
    EntryName::new(format!("CHECKPOINT-{}", slot.label())).expect("fixed checkpoint name")
}

fn chunk_name(slot: Slot, index: usize) -> Result<EntryName, StorageError> {
    if index >= MAX_CHECKPOINT_CHUNKS {
        return Err(StorageError::ResourceLimit);
    }
    EntryName::new(format!("CHECKPOINT-{}-{index:03}", slot.label()))
        .map_err(|_| StorageError::IntegrityFailure)
}

fn invalidate_slot<F: FileSystem>(
    filesystem: &mut F,
    directory: &F::Directory,
    slot: Slot,
) -> Result<(), StorageError> {
    remove_if_present(filesystem, directory, &manifest_name(slot))?;
    filesystem.sync_directory(directory)?;
    Ok(())
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

#[allow(clippy::too_many_arguments)]
fn publish_chunk<F, W, E>(
    filesystem: &mut F,
    context: &CheckpointContext<'_, F::Directory>,
    vault: &mut KeyVault<W, E>,
    scope: NamespaceRef,
    slot: Slot,
    object_id: [u8; 16],
    index: usize,
    chunk: &[u8],
) -> Result<(), StorageError>
where
    F: FileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
{
    if chunk.is_empty() || chunk.len() > CHECKPOINT_CHUNK_BYTES {
        return Err(StorageError::InvalidState);
    }
    let name = chunk_name(slot, index)?;
    remove_if_present(filesystem, context.directory, &name)?;
    let sequence = u64::try_from(index)
        .map_err(|_| StorageError::ResourceLimit)?
        .checked_add(1)
        .ok_or(StorageError::ResourceLimit)?;
    let encoded = vault
        .encrypt(
            checkpoint_context(
                context,
                scope.namespace(),
                object_id,
                sequence,
                FrameClass::Blob64KiB,
            ),
            chunk,
        )?
        .encode()?;
    if u64::try_from(encoded.len()).map_err(|_| StorageError::ResourceLimit)?
        > MAX_ENCODED_CHUNK_BYTES
    {
        return Err(StorageError::ResourceLimit);
    }
    let file = filesystem.create_new(context.directory, &name)?;
    write_all_at(filesystem, &file, 0, &encoded)?;
    filesystem.set_len(
        &file,
        u64::try_from(encoded.len()).map_err(|_| StorageError::ResourceLimit)?,
    )?;
    filesystem.sync_all(&file)?;
    Ok(())
}

fn checkpoint_context<D>(
    context: &CheckpointContext<'_, D>,
    namespace: NamespaceId,
    object_id: [u8; 16],
    sequence: u64,
    frame: FrameClass,
) -> CryptoContext {
    CryptoContext::new(
        context.database,
        Scope::Namespace(namespace),
        context.epoch,
        ObjectRole::Snapshot,
        CryptoObjectId::from_bytes(object_id),
        sequence,
        context.writer,
        MAJOR,
        MINOR,
        frame,
    )
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

const fn cache_crypto_error(error: CryptoError) -> StorageError {
    match error {
        CryptoError::ResourceLimit => StorageError::ResourceLimit,
        CryptoError::UnsupportedProfile => StorageError::UnsupportedProfile,
        _ => StorageError::IntegrityFailure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest {
            scope: NamespaceRef::new(
                DatabaseId::from_bytes([0x11; 16]),
                NamespaceId::from_bytes([0x12; 16]),
            ),
            revision: CommitRevision::FIRST,
            generation: 1,
            certificate_digest: [0x13; 32],
            reducer_profile: [0x14; 32],
            logical_state_digest: [0x15; 32],
            payload_digest: [0x16; 32],
            payload_len: CHECKPOINT_CHUNK_BYTES as u64 + 1,
            chunk_count: 2,
            object_id: [0x17; 16],
        }
    }

    #[test]
    fn authenticated_manifest_decoder_rejects_noncanonical_and_unbounded_shapes() {
        let valid = manifest();
        assert_eq!(
            Manifest::decode(&valid.encode(), valid.scope.database())
                .unwrap()
                .encode(),
            valid.encode()
        );
        assert_eq!(
            validate_manifest_shape(valid),
            Ok((CHECKPOINT_CHUNK_BYTES + 1, 2))
        );

        for (payload_len, chunk_count) in [
            (0, 1),
            (1, 0),
            (MAX_CHECKPOINT_BYTES as u64 + 1, 1),
            (1, u32::try_from(MAX_CHECKPOINT_CHUNKS).unwrap() + 1),
            (CHECKPOINT_CHUNK_BYTES as u64 + 1, 1),
        ] {
            let malformed = Manifest {
                payload_len,
                chunk_count,
                ..valid
            };
            assert_eq!(
                validate_manifest_shape(malformed),
                Err(StorageError::IntegrityFailure)
            );
        }

        for offset in [6, 180, 207] {
            let mut encoded = valid.encode();
            encoded[offset] = 1;
            assert!(matches!(
                Manifest::decode(&encoded, valid.scope.database()),
                Err(StorageError::IntegrityFailure)
            ));
        }
    }
}
