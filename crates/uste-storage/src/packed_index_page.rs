//! Strict encrypted framing for immutable packed index records (Decision 0129).
//! This privileged framing surface neither validates typed payloads nor authorizes access.
use uste_crypto::{
    CryptoContext, CryptoObjectId, EncryptedEnvelope, EntropySource, FrameClass, KeyEpoch,
    KeyVault, ObjectRole, Scope, SecretBytes, WriterIncarnationId,
};
use uste_types::{CommitRevision, NamespaceRef};
use zeroize::Zeroizing;

use crate::journal::StorageError;

pub const PAGE_BYTES: usize = 16_384;
pub const ENCODED_PAGE_BYTES: usize = 20_545;
pub const MAX_SLOTS: u16 = 128;
pub const MAX_PAGES: u64 = 16_777_216;
const DIRECTORY: usize = 80;
const DATA: usize = DIRECTORY + MAX_SLOTS as usize * 4;
pub const MAX_RECORD_PAYLOAD: usize = PAGE_BYTES - DATA - 1;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct PackedPageContext {
    pub scope: NamespaceRef,
    pub epoch: KeyEpoch,
    pub writer: WriterIncarnationId,
    pub creation_revision: CommitRevision,
    pub profile: [u8; 32],
    pub family: u8,
    pub object: [u8; 16],
    pub page: u64,
}

impl PackedPageContext {
    fn validate(self) -> Result<(), StorageError> {
        if self.family == 0 || self.object == [0; 16] {
            return Err(StorageError::InvalidState);
        }
        if self.page >= MAX_PAGES {
            return Err(StorageError::ResourceLimit);
        }
        Ok(())
    }

    fn crypto(self) -> CryptoContext {
        CryptoContext::new(
            self.scope.database(),
            Scope::Namespace(self.scope.namespace()),
            self.epoch,
            ObjectRole::IndexPage,
            CryptoObjectId::from_bytes(self.object),
            self.page + 1,
            self.writer,
            2,
            0,
            FrameClass::Small4KiB,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PackedRecordKind {
    TreeNode = 1,
    ValueChunk = 2,
}

impl PackedRecordKind {
    fn decode(byte: u8) -> Result<Self, StorageError> {
        match byte {
            1 => Ok(Self::TreeNode),
            2 => Ok(Self::ValueChunk),
            _ => Err(StorageError::IntegrityFailure),
        }
    }
}

/// Still requires typed payload validation; a record is not a proof or a capability.
pub struct PackedRecord<'a> {
    pub kind: PackedRecordKind,
    pub payload: &'a [u8],
}

pub struct PackedPageBuilder {
    context: PackedPageContext,
    bytes: Zeroizing<Vec<u8>>,
    count: u16,
    used: usize,
}

impl PackedPageBuilder {
    pub fn new(context: PackedPageContext) -> Result<Self, StorageError> {
        context.validate()?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(PAGE_BYTES)
            .map_err(|_| StorageError::ResourceLimit)?;
        bytes.resize(PAGE_BYTES, 0);
        bytes[..4].copy_from_slice(b"UICP");
        bytes[4] = 2;
        bytes[6] = context.family;
        bytes[8..16].copy_from_slice(&context.creation_revision.get().to_be_bytes());
        bytes[16..24].copy_from_slice(&context.page.to_be_bytes());
        bytes[26..28].copy_from_slice(&(DATA as u16).to_be_bytes());
        bytes[32..64].copy_from_slice(&context.profile);
        bytes[64..80].copy_from_slice(&context.object);
        Ok(Self {
            context,
            bytes: Zeroizing::new(bytes),
            count: 0,
            used: DATA,
        })
    }

    pub fn push(&mut self, kind: PackedRecordKind, payload: &[u8]) -> Result<u16, StorageError> {
        if payload.is_empty() {
            return Err(StorageError::InvalidState);
        }
        if self.count == MAX_SLOTS
            || payload.len() > MAX_RECORD_PAYLOAD
            || payload.len() + 1 > PAGE_BYTES - self.used
        {
            return Err(StorageError::ResourceLimit);
        }
        let slot = self.count;
        let directory = DIRECTORY + slot as usize * 4;
        let length = payload.len() + 1;
        self.bytes[directory..directory + 2].copy_from_slice(&(self.used as u16).to_be_bytes());
        self.bytes[directory + 2..directory + 4].copy_from_slice(&(length as u16).to_be_bytes());
        self.bytes[self.used] = kind as u8;
        self.bytes[self.used + 1..self.used + length].copy_from_slice(payload);
        self.used += length;
        self.count += 1;
        self.bytes[24..26].copy_from_slice(&self.count.to_be_bytes());
        self.bytes[26..28].copy_from_slice(&(self.used as u16).to_be_bytes());
        Ok(slot)
    }

    pub fn seal<W, E: EntropySource>(
        &self,
        vault: &mut KeyVault<W, E>,
    ) -> Result<Vec<u8>, StorageError> {
        if self.count == 0 {
            return Err(StorageError::InvalidState);
        }
        let encoded = vault
            .encrypt(self.context.crypto(), &self.bytes)?
            .encode()?;
        if encoded.len() != ENCODED_PAGE_BYTES {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(encoded)
    }
}

/// Authenticated framing only, retained in a zeroizing owner; no public plaintext-copy API.
pub struct PackedPage {
    bytes: SecretBytes,
    count: u16,
}

impl PackedPage {
    pub fn open<W, E: EntropySource>(
        vault: &KeyVault<W, E>,
        context: PackedPageContext,
        encoded: &[u8],
    ) -> Result<Self, StorageError> {
        context.validate()?;
        if encoded.len() != ENCODED_PAGE_BYTES {
            return Err(StorageError::IntegrityFailure);
        }
        let envelope = EncryptedEnvelope::decode(encoded)?;
        let bytes = vault.decrypt(context.crypto(), &envelope)?;
        let count = validate_plaintext(context, bytes.as_slice())?;
        Ok(Self { bytes, count })
    }

    pub fn record_count(&self) -> u16 {
        self.count
    }

    pub fn record(&self, slot: u16) -> Option<PackedRecord<'_>> {
        if slot >= self.count {
            return None;
        }
        let bytes = self.bytes.as_slice();
        let directory = DIRECTORY + slot as usize * 4;
        let start = read_u16(bytes, directory) as usize;
        let length = read_u16(bytes, directory + 2) as usize;
        Some(PackedRecord {
            kind: PackedRecordKind::decode(bytes[start]).ok()?,
            payload: &bytes[start + 1..start + length],
        })
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}

fn validate_plaintext(context: PackedPageContext, bytes: &[u8]) -> Result<u16, StorageError> {
    if bytes.len() != PAGE_BYTES
        || &bytes[..4] != b"UICP"
        || bytes[4] != 2
        || bytes[5] != 0
        || bytes[6] != context.family
        || bytes[7] != 0
        || bytes[8..16] != context.creation_revision.get().to_be_bytes()
        || bytes[16..24] != context.page.to_be_bytes()
        || bytes[28..32] != [0; 4]
        || bytes[32..64] != context.profile
        || bytes[64..80] != context.object
    {
        return Err(StorageError::IntegrityFailure);
    }
    let count = read_u16(bytes, 24);
    let used = read_u16(bytes, 26) as usize;
    if count == 0 || count > MAX_SLOTS || !(DATA..=PAGE_BYTES).contains(&used) {
        return Err(StorageError::IntegrityFailure);
    }
    let mut next = DATA;
    for slot in 0..count {
        let directory = DIRECTORY + slot as usize * 4;
        let start = read_u16(bytes, directory) as usize;
        let length = read_u16(bytes, directory + 2) as usize;
        if start != next || length < 2 || length > used - next {
            return Err(StorageError::IntegrityFailure);
        }
        PackedRecordKind::decode(bytes[start])?;
        next += length;
    }
    if next != used
        || bytes[DIRECTORY + count as usize * 4..DATA]
            .iter()
            .any(|b| *b != 0)
        || bytes[used..].iter().any(|b| *b != 0)
    {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(count)
}
