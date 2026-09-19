//! Closed borrowed node/chunk grammar. Structural validity is not canonical-root admission.
use crate::{
    journal::StorageError,
    ordered_commitment::{
        self as logical, CommitmentContext, LeafProof, OrderedCommitment, ValueCommitment,
    },
    packed_index_pack::PackedRecordAddress,
    packed_index_page::{
        MAX_PAGES, MAX_RECORD_PAYLOAD, MAX_SLOTS, PackedPageContext, PackedRecord, PackedRecordKind,
    },
};
use uste_crypto::{KeyEpoch, WriterIncarnationId};
use uste_types::{CommitRevision, NamespaceRef};
use zeroize::Zeroizing;

pub const LOCATOR_BYTES: usize = 54;
pub const MAX_CHUNK_DATA: usize = MAX_RECORD_PAYLOAD - 8 - LOCATOR_BYTES;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct PackedLocator {
    object: [u8; 16],
    revision: CommitRevision,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    page: u32,
    slot: u16,
}

impl PackedLocator {
    /// Resolve a relative locator under an exact enclosing tree identity, without authorization.
    pub fn resolve(
        self,
        scope: NamespaceRef,
        profile: [u8; 32],
        family: u8,
        maximum_revision: CommitRevision,
    ) -> Result<PackedPageContext, StorageError> {
        let context = PackedPageContext {
            scope,
            profile,
            family,
            object: self.object,
            creation_revision: self.revision,
            epoch: self.epoch,
            writer: self.writer,
            page: self.page as u64,
        };
        context.validate()?;
        if self.slot >= MAX_SLOTS || self.revision > maximum_revision {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(context)
    }

    /// Scope/profile/family are supplied by the enclosing authenticated tree, not serialized here.
    pub fn from_address(address: PackedRecordAddress) -> Self {
        let c = address.context();
        Self {
            object: c.object,
            revision: c.creation_revision,
            epoch: c.epoch,
            writer: c.writer,
            page: c.page as u32,
            slot: address.slot(),
        }
    }
    pub fn page_context(self, owner: PackedPageContext) -> PackedPageContext {
        PackedPageContext {
            object: self.object,
            creation_revision: self.revision,
            epoch: self.epoch,
            writer: self.writer,
            page: self.page as u64,
            ..owner
        }
    }
    pub fn slot(self) -> u16 {
        self.slot
    }
    fn validate(self, owner: PackedPageContext) -> Result<(), StorageError> {
        if self.object == [0; 16]
            || self.page as u64 >= MAX_PAGES
            || self.slot >= MAX_SLOTS
            || self.revision > owner.creation_revision
        {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(())
    }
    fn encode(self, out: &mut Vec<u8>) {
        out.extend(self.encode_fixed());
    }
    pub(crate) fn encode_fixed(self) -> [u8; LOCATOR_BYTES] {
        let mut out = [0; LOCATOR_BYTES];
        out[..16].copy_from_slice(&self.object);
        out[16..24].copy_from_slice(&self.revision.get().to_be_bytes());
        out[24..32].copy_from_slice(&self.epoch.get().to_be_bytes());
        out[32..48].copy_from_slice(self.writer.as_bytes());
        out[48..52].copy_from_slice(&self.page.to_be_bytes());
        out[52..54].copy_from_slice(&self.slot.to_be_bytes());
        out
    }
    pub(crate) fn decode_fixed(
        bytes: &[u8],
        owner: PackedPageContext,
    ) -> Result<Self, StorageError> {
        owner.validate()?;
        let mut input = Cursor(bytes);
        let location = Self::decode(&mut input, owner)?;
        input.finish()?;
        Ok(location)
    }
    fn decode(input: &mut Cursor<'_>, owner: PackedPageContext) -> Result<Self, StorageError> {
        let value = Self {
            object: input.array()?,
            revision: CommitRevision::new(u64::from_be_bytes(input.array()?))
                .map_err(|_| StorageError::IntegrityFailure)?,
            epoch: KeyEpoch::new(u64::from_be_bytes(input.array()?))
                .map_err(|_| StorageError::IntegrityFailure)?,
            writer: WriterIncarnationId::from_bytes(input.array()?),
            page: u32::from_be_bytes(input.array()?),
            slot: u16::from_be_bytes(input.array()?),
        };
        value.validate(owner)?;
        Ok(value)
    }
}

#[derive(Clone, Copy)]
pub struct ChildReference {
    pub location: PackedLocator,
    pub claimed: OrderedCommitment,
}
impl ChildReference {
    fn encode(self, out: &mut Vec<u8>) {
        self.location.encode(out);
        out.extend(self.claimed.entries().to_be_bytes());
        out.extend(self.claimed.logical_bytes().to_be_bytes());
        out.extend(self.claimed.digest());
    }
    fn decode(input: &mut Cursor<'_>, owner: PackedPageContext) -> Result<Self, StorageError> {
        let location = PackedLocator::decode(input, owner)?;
        let entries = u64::from_be_bytes(input.array()?);
        let bytes = u64::from_be_bytes(input.array()?);
        let claimed = OrderedCommitment::claimed_nonempty(entries, bytes, input.array()?)
            .map_err(|_| StorageError::IntegrityFailure)?;
        Ok(Self { location, claimed })
    }
}

#[derive(Clone, Copy)]
pub enum TreeNode<'a> {
    Leaf {
        key: &'a [u8],
        value: ValueCommitment,
        first_chunk: Option<PackedLocator>,
    },
    Branch {
        bit: u32,
        left: ChildReference,
        right: ChildReference,
    },
}

impl<'a> TreeNode<'a> {
    pub fn commitment(self, owner: PackedPageContext) -> Result<OrderedCommitment, StorageError> {
        owner.validate()?;
        let context = CommitmentContext::new(owner.scope, owner.profile, owner.family)
            .map_err(|_| StorageError::IntegrityFailure)?;
        match self {
            Self::Leaf {
                key,
                value,
                first_chunk,
            } => {
                if (value.length == 0) != first_chunk.is_none() {
                    return Err(StorageError::IntegrityFailure);
                }
                if let Some(location) = first_chunk {
                    location.validate(owner)?;
                }
                if value.length == 0
                    && value
                        != logical::value_commitment(b"")
                            .map_err(|_| StorageError::IntegrityFailure)?
                {
                    return Err(StorageError::IntegrityFailure);
                }
                logical::leaf_commitment(context, LeafProof { key, value })
                    .map_err(|_| StorageError::IntegrityFailure)
            }
            Self::Branch { bit, left, right } => {
                left.location.validate(owner)?;
                right.location.validate(owner)?;
                if left.location == right.location {
                    return Err(StorageError::IntegrityFailure);
                }
                logical::branch_commitment(context, bit, left.claimed, right.claimed)
                    .map_err(|_| StorageError::IntegrityFailure)
            }
        }
    }

    pub fn decode(
        owner: PackedPageContext,
        record: PackedRecord<'a>,
    ) -> Result<Self, StorageError> {
        if record.kind != PackedRecordKind::TreeNode || record.payload.len() > MAX_RECORD_PAYLOAD {
            return Err(StorageError::IntegrityFailure);
        }
        let mut input = Cursor(record.payload);
        if input.byte()? != 1 {
            return Err(StorageError::IntegrityFailure);
        }
        let node = match input.byte()? {
            1 => {
                let length = u16::from_be_bytes(input.array()?) as usize;
                let key = input.take(length)?;
                let value = ValueCommitment {
                    length: u32::from_be_bytes(input.array()?) as u64,
                    digest: input.array()?,
                };
                let first_chunk = input.location(owner)?;
                Self::Leaf {
                    key,
                    value,
                    first_chunk,
                }
            }
            2 => Self::Branch {
                bit: u32::from_be_bytes(input.array()?),
                left: ChildReference::decode(&mut input, owner)?,
                right: ChildReference::decode(&mut input, owner)?,
            },
            _ => return Err(StorageError::IntegrityFailure),
        };
        input.finish()?;
        node.commitment(owner)?;
        Ok(node)
    }

    pub fn encode(self, owner: PackedPageContext) -> Result<Zeroizing<Vec<u8>>, StorageError> {
        self.commitment(owner)?;
        let length = match self {
            Self::Leaf {
                key, first_chunk, ..
            } => 41 + key.len() + first_chunk.map_or(0, |_| LOCATOR_BYTES),
            Self::Branch { .. } => 2 + 4 + 2 * (LOCATOR_BYTES + 48),
        };
        let mut out = buffer(length)?;
        out.push(1);
        match self {
            Self::Leaf {
                key,
                value,
                first_chunk,
            } => {
                out.push(1);
                out.extend((key.len() as u16).to_be_bytes());
                out.extend(key);
                out.extend((value.length as u32).to_be_bytes());
                out.extend(value.digest);
                encode_location(first_chunk, &mut out);
            }
            Self::Branch { bit, left, right } => {
                out.push(2);
                out.extend(bit.to_be_bytes());
                left.encode(&mut out);
                right.encode(&mut out);
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Copy)]
pub struct ValueChunk<'a> {
    pub remaining: u32,
    pub next: Option<PackedLocator>,
    pub data: &'a [u8],
}
impl<'a> ValueChunk<'a> {
    fn validate(self, owner: PackedPageContext) -> Result<(), StorageError> {
        owner.validate()?;
        if self.remaining == 0
            || self.remaining as usize > logical::MAX_VALUE_BYTES
            || self.data.len() != (self.remaining as usize).min(MAX_CHUNK_DATA)
            || self.next.is_some() != (self.remaining as usize > self.data.len())
        {
            return Err(StorageError::IntegrityFailure);
        }
        if let Some(next) = self.next {
            next.validate(owner)?;
        }
        Ok(())
    }
    pub fn decode(
        owner: PackedPageContext,
        record: PackedRecord<'a>,
    ) -> Result<Self, StorageError> {
        if record.kind != PackedRecordKind::ValueChunk || record.payload.len() > MAX_RECORD_PAYLOAD
        {
            return Err(StorageError::IntegrityFailure);
        }
        let mut input = Cursor(record.payload);
        if input.byte()? != 1 {
            return Err(StorageError::IntegrityFailure);
        }
        let remaining = u32::from_be_bytes(input.array()?);
        let length = u16::from_be_bytes(input.array()?) as usize;
        let next = input.location(owner)?;
        let data = input.take(length)?;
        input.finish()?;
        let chunk = Self {
            remaining,
            next,
            data,
        };
        chunk.validate(owner)?;
        Ok(chunk)
    }
    pub fn encode(self, owner: PackedPageContext) -> Result<Zeroizing<Vec<u8>>, StorageError> {
        self.validate(owner)?;
        let mut out = buffer(8 + self.data.len() + self.next.map_or(0, |_| LOCATOR_BYTES))?;
        out.push(1);
        out.extend(self.remaining.to_be_bytes());
        out.extend((self.data.len() as u16).to_be_bytes());
        encode_location(self.next, &mut out);
        out.extend(self.data);
        Ok(out)
    }
}

fn buffer(length: usize) -> Result<Zeroizing<Vec<u8>>, StorageError> {
    if length > MAX_RECORD_PAYLOAD {
        return Err(StorageError::ResourceLimit);
    }
    let mut out = Vec::new();
    out.try_reserve_exact(length)
        .map_err(|_| StorageError::ResourceLimit)?;
    Ok(Zeroizing::new(out))
}
fn encode_location(value: Option<PackedLocator>, out: &mut Vec<u8>) {
    out.push(u8::from(value.is_some()));
    if let Some(value) = value {
        value.encode(out);
    }
}
struct Cursor<'a>(&'a [u8]);
impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], StorageError> {
        let bytes = self.0.get(..count).ok_or(StorageError::IntegrityFailure)?;
        self.0 = &self.0[count..];
        Ok(bytes)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], StorageError> {
        self.take(N)?
            .try_into()
            .map_err(|_| StorageError::IntegrityFailure)
    }
    fn byte(&mut self) -> Result<u8, StorageError> {
        Ok(self.array::<1>()?[0])
    }
    fn location(
        &mut self,
        owner: PackedPageContext,
    ) -> Result<Option<PackedLocator>, StorageError> {
        match self.byte()? {
            0 => Ok(None),
            1 => Ok(Some(PackedLocator::decode(self, owner)?)),
            _ => Err(StorageError::IntegrityFailure),
        }
    }
    fn finish(self) -> Result<(), StorageError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(StorageError::IntegrityFailure)
        }
    }
}
