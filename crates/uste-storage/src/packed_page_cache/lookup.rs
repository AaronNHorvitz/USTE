//! Bounded positive lookup retention. The enclosing cache supplies owner/session authority.
//!
//! Resident values are indexed by a hash map keyed on the interned identity index and the
//! logical key bytes, and ordered by an intrusive doubly linked exact least-recently-used list.
//! Hit, miss, eviction, bypass and logical accounting semantics are identical to the earlier
//! ordered-map representation; only the constant per-hit bookkeeping cost differs.
use super::*;
use crate::ordered_commitment::{MAX_KEY_BYTES, MAX_VALUE_BYTES, OrderedCommitment};
use crate::packed_tree_lookup::{TreeLookupLimits, TreeLookupReport, TreeReadContext};
use crate::packed_tree_record::PackedLocator;
use std::{
    borrow::Borrow,
    collections::HashMap,
    hash::{Hash, Hasher},
};
use zeroize::Zeroizing;

pub(super) const IDENTITY_BYTES: usize = 175;
const FIXED: usize = 4096;
const ENTRY: usize = 512;
pub(super) const MINIMUM: usize = 8192;
pub(crate) type CachedLookup = (Zeroizing<Vec<u8>>, TreeLookupReport);
const NONE: u32 = u32::MAX;
const INLINE_PROBE_BYTES: usize = 64;

#[derive(Clone, Copy)]
pub(crate) struct Identity(pub(super) [u8; IDENTITY_BYTES]);
impl Identity {
    pub(crate) fn new(
        c: TreeReadContext,
        root: PackedLocator,
        expected: OrderedCommitment,
    ) -> Self {
        let mut bytes = [0; IDENTITY_BYTES];
        let mut offset = 0;
        for part in [
            c.scope.database().as_bytes().as_slice(),
            c.scope.namespace().as_bytes(),
            &c.profile,
            &[c.family],
            &c.revision.get().to_be_bytes(),
            &root.encode_fixed(),
            &expected.entries().to_be_bytes(),
            &expected.logical_bytes().to_be_bytes(),
            expected.digest(),
        ] {
            bytes[offset..offset + part.len()].copy_from_slice(part);
            offset += part.len();
        }
        debug_assert_eq!(offset, IDENTITY_BYTES);
        Self(bytes)
    }
}

/// Distinct result-cache observations; proof-work and physical-device bytes are not counted here.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackedLookupCacheReport {
    pub budget_bytes: usize,
    pub accounted_bytes: usize,
    pub resident_values: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub oversized_bypasses: u64,
}

/// One interned authenticated identity shared by every resident value under it.
struct RetainedIdentity {
    bytes: Zeroizing<[u8; IDENTITY_BYTES]>,
    live: usize,
}

/// Hash-map key: the interned identity index followed by the logical key bytes.
struct MapKey(Zeroizing<Vec<u8>>);
impl Borrow<[u8]> for MapKey {
    fn borrow(&self) -> &[u8] {
        self.0.as_slice()
    }
}
impl PartialEq for MapKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_slice() == other.0.as_slice()
    }
}
impl Eq for MapKey {}
impl Hash for MapKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.as_slice().hash(state);
    }
}

/// Stack-first probe buffer so ordinary keys never allocate on the hit path.
enum Probe {
    Inline(Zeroizing<[u8; INLINE_PROBE_BYTES]>, usize),
    Heap(Zeroizing<Vec<u8>>),
}
impl Probe {
    fn new(identity: u32, key: &[u8]) -> Result<Self, StorageError> {
        let total = key
            .len()
            .checked_add(4)
            .ok_or(StorageError::ResourceLimit)?;
        if total <= INLINE_PROBE_BYTES {
            let mut inline = Zeroizing::new([0; INLINE_PROBE_BYTES]);
            inline[..4].copy_from_slice(&identity.to_be_bytes());
            inline[4..total].copy_from_slice(key);
            return Ok(Self::Inline(inline, total));
        }
        let mut heap = Zeroizing::new(Vec::new());
        heap.try_reserve_exact(total)
            .map_err(|_| StorageError::ResourceLimit)?;
        heap.extend_from_slice(&identity.to_be_bytes());
        heap.extend_from_slice(key);
        Ok(Self::Heap(heap))
    }
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Inline(bytes, len) => &bytes[..*len],
            Self::Heap(bytes) => bytes.as_slice(),
        }
    }
    fn into_key(self) -> Result<MapKey, StorageError> {
        match self {
            Self::Heap(bytes) => Ok(MapKey(bytes)),
            Self::Inline(bytes, len) => {
                let mut heap = Zeroizing::new(Vec::new());
                heap.try_reserve_exact(len)
                    .map_err(|_| StorageError::ResourceLimit)?;
                heap.extend_from_slice(&bytes[..len]);
                Ok(MapKey(heap))
            }
        }
    }
}

struct Slot {
    identity: u32,
    key: Zeroizing<Vec<u8>>,
    bytes: Zeroizing<Vec<u8>>,
    work: TreeLookupReport,
    charge: usize,
    /// More recently used neighbour; `NONE` at the head.
    newer: u32,
    /// Less recently used neighbour; `NONE` at the tail.
    older: u32,
}

pub(super) struct LookupCache {
    budget: usize,
    used: usize,
    identities: Vec<Option<RetainedIdentity>>,
    free_identities: Vec<u32>,
    slots: Vec<Option<Slot>>,
    free_slots: Vec<u32>,
    index: HashMap<MapKey, u32>,
    /// Most recently used slot.
    head: u32,
    /// Least recently used slot; evicted first.
    tail: u32,
    hits: u64,
    misses: u64,
    evictions: u64,
    bypasses: u64,
    overflowed: bool,
}
impl LookupCache {
    pub(super) fn new(budget: usize) -> Result<Self, StorageError> {
        if !(MINIMUM..=MAX_INDEX_CACHE_BYTES).contains(&budget) {
            return Err(StorageError::ResourceLimit);
        }
        Ok(Self {
            budget,
            used: 0,
            identities: Vec::new(),
            free_identities: Vec::new(),
            slots: Vec::new(),
            free_slots: Vec::new(),
            index: HashMap::new(),
            head: NONE,
            tail: NONE,
            hits: 0,
            misses: 0,
            evictions: 0,
            bypasses: 0,
            overflowed: false,
        })
    }
    pub(super) fn clear(&mut self) {
        self.index.clear();
        self.slots.clear();
        self.free_slots.clear();
        self.identities.clear();
        self.free_identities.clear();
        self.head = NONE;
        self.tail = NONE;
        self.used = 0;
    }
    pub(super) fn report(&self) -> Result<PackedLookupCacheReport, StorageError> {
        if self.overflowed {
            return Err(StorageError::ResourceLimit);
        }
        Ok(PackedLookupCacheReport {
            budget_bytes: self.budget,
            accounted_bytes: if self.index.is_empty() {
                0
            } else {
                FIXED + self.used
            },
            resident_values: self.index.len(),
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            oversized_bypasses: self.bypasses,
        })
    }
    fn identity_index(&self, identity: &Identity) -> Option<u32> {
        self.identities
            .iter()
            .position(|slot| slot.as_ref().is_some_and(|slot| *slot.bytes == identity.0))
            .and_then(|index| u32::try_from(index).ok())
    }
    fn slot(&self, index: u32) -> Result<&Slot, StorageError> {
        self.slots
            .get(index as usize)
            .and_then(Option::as_ref)
            .ok_or(StorageError::IntegrityFailure)
    }
    fn slot_mut(&mut self, index: u32) -> Result<&mut Slot, StorageError> {
        self.slots
            .get_mut(index as usize)
            .and_then(Option::as_mut)
            .ok_or(StorageError::IntegrityFailure)
    }
    fn unlink(&mut self, index: u32) -> Result<(), StorageError> {
        let (newer, older) = {
            let slot = self.slot(index)?;
            (slot.newer, slot.older)
        };
        if newer == NONE {
            if self.head != index {
                return Err(StorageError::IntegrityFailure);
            }
            self.head = older;
        } else {
            self.slot_mut(newer)?.older = older;
        }
        if older == NONE {
            if self.tail != index {
                return Err(StorageError::IntegrityFailure);
            }
            self.tail = newer;
        } else {
            self.slot_mut(older)?.newer = newer;
        }
        Ok(())
    }
    fn push_head(&mut self, index: u32) -> Result<(), StorageError> {
        let head = self.head;
        {
            let slot = self.slot_mut(index)?;
            slot.newer = NONE;
            slot.older = head;
        }
        if head == NONE {
            self.tail = index;
        } else {
            self.slot_mut(head)?.newer = index;
        }
        self.head = index;
        Ok(())
    }
    /// Locate a resident value, count the observation and admit it against `limits` without
    /// changing recency; the caller promotes the slot only after admission succeeds.
    fn locate(
        &mut self,
        identity: Identity,
        key: &[u8],
        limits: TreeLookupLimits,
    ) -> Result<Option<u32>, StorageError> {
        validate_key(key)?;
        let found = match self.identity_index(&identity) {
            Some(interned) => {
                let probe = Probe::new(interned, key)?;
                self.index.get(probe.as_slice()).copied()
            }
            None => None,
        };
        let Some(index) = found else {
            increment(&mut self.misses, &mut self.overflowed);
            return Ok(None);
        };
        increment(&mut self.hits, &mut self.overflowed);
        let slot = self.slot(index)?;
        if slot.work.pages > limits.maximum_pages
            || slot.work.encoded_bytes > limits.maximum_encoded_bytes
            || slot.work.path_branches > limits.maximum_path_branches
            || slot.bytes.len() as u64 > limits.maximum_value_bytes
        {
            return Err(StorageError::ResourceLimit);
        }
        Ok(Some(index))
    }
    fn promote(&mut self, index: u32) -> Result<(), StorageError> {
        if self.head == index {
            return Ok(());
        }
        self.unlink(index)?;
        self.push_head(index)
    }
    pub(super) fn get(
        &mut self,
        identity: Identity,
        key: &[u8],
        limits: TreeLookupLimits,
    ) -> Result<Option<CachedLookup>, StorageError> {
        let Some(index) = self.locate(identity, key, limits)? else {
            return Ok(None);
        };
        self.promote(index)?;
        let slot = self.slot(index)?;
        let output = copy(&slot.bytes, 0)?;
        Ok(Some((output, slot.work)))
    }
    pub(super) fn get_with<R, F: FnOnce(&[u8]) -> R>(
        &mut self,
        identity: Identity,
        key: &[u8],
        limits: TreeLookupLimits,
        map: &mut Option<F>,
    ) -> Result<Option<(R, TreeLookupReport)>, StorageError> {
        let Some(index) = self.locate(identity, key, limits)? else {
            return Ok(None);
        };
        self.promote(index)?;
        // Complete internal integrity/LRU updates before exposing plaintext to the mapper.
        // `R` cannot borrow from this argument, so cache ownership never escapes.
        let slot = self.slot(index)?;
        let output = map.take().ok_or(StorageError::InvalidState)?(&slot.bytes);
        Ok(Some((output, slot.work)))
    }
    fn evict_oldest(&mut self) -> Result<(), StorageError> {
        let index = self.tail;
        if index == NONE {
            return Err(StorageError::IntegrityFailure);
        }
        self.unlink(index)?;
        let removed = self
            .slots
            .get_mut(index as usize)
            .and_then(Option::take)
            .ok_or(StorageError::IntegrityFailure)?;
        let probe = Probe::new(removed.identity, &removed.key)?;
        if self.index.remove(probe.as_slice()) != Some(index) {
            return Err(StorageError::IntegrityFailure);
        }
        self.release_identity(removed.identity)?;
        self.free_slots.push(index);
        self.used = self
            .used
            .checked_sub(removed.charge)
            .ok_or(StorageError::IntegrityFailure)?;
        increment(&mut self.evictions, &mut self.overflowed);
        Ok(())
    }
    fn release_identity(&mut self, index: u32) -> Result<(), StorageError> {
        let entry = self
            .identities
            .get_mut(index as usize)
            .ok_or(StorageError::IntegrityFailure)?;
        let retained = entry.as_mut().ok_or(StorageError::IntegrityFailure)?;
        retained.live = retained
            .live
            .checked_sub(1)
            .ok_or(StorageError::IntegrityFailure)?;
        if retained.live == 0 {
            *entry = None;
            self.free_identities.push(index);
        }
        Ok(())
    }
    fn retain_identity(&mut self, identity: Identity) -> Result<u32, StorageError> {
        if let Some(index) = self.identity_index(&identity) {
            self.identities
                .get_mut(index as usize)
                .and_then(Option::as_mut)
                .ok_or(StorageError::IntegrityFailure)?
                .live += 1;
            return Ok(index);
        }
        let retained = RetainedIdentity {
            bytes: Zeroizing::new(identity.0),
            live: 1,
        };
        if let Some(index) = self.free_identities.pop() {
            let entry = self
                .identities
                .get_mut(index as usize)
                .ok_or(StorageError::IntegrityFailure)?;
            if entry.is_some() {
                return Err(StorageError::IntegrityFailure);
            }
            *entry = Some(retained);
            return Ok(index);
        }
        let index =
            u32::try_from(self.identities.len()).map_err(|_| StorageError::ResourceLimit)?;
        if index == NONE {
            return Err(StorageError::ResourceLimit);
        }
        self.identities
            .try_reserve(1)
            .map_err(|_| StorageError::ResourceLimit)?;
        self.identities.push(Some(retained));
        Ok(index)
    }
    pub(super) fn insert(
        &mut self,
        identity: Identity,
        key: &[u8],
        bytes: &[u8],
        work: TreeLookupReport,
    ) -> Result<(), StorageError> {
        if bytes.len() > MAX_VALUE_BYTES {
            return Err(StorageError::ResourceLimit);
        }
        if work.pages != u64::from(work.path_branches) + 1 + u64::from(work.value_chunks)
            || work.path_branches > crate::ordered_commitment::MAX_BRANCH_BITS
            || work.pages > crate::packed_tree_lookup::MAX_LOOKUP_PAGES
            || work.pages.checked_mul(ENCODED_PAGE_BYTES as u64) != Some(work.encoded_bytes)
        {
            return Err(StorageError::InvalidState);
        }
        validate_key(key)?;
        if let Some(interned) = self.identity_index(&identity)
            && self
                .index
                .contains_key(Probe::new(interned, key)?.as_slice())
        {
            return Err(StorageError::InvalidState);
        }
        // Logical charge: fixed entry allowance plus identity, exact key and exact value bytes.
        let charge = ENTRY
            .checked_add(IDENTITY_BYTES)
            .and_then(|n| n.checked_add(key.len()))
            .and_then(|n| n.checked_add(bytes.len()))
            .ok_or(StorageError::ResourceLimit)?;
        if charge > self.budget - FIXED {
            increment(&mut self.bypasses, &mut self.overflowed);
            return Ok(());
        }
        let logical = copy(key, 0)?;
        let output = copy(bytes, 0)?;
        while self.used > self.budget - FIXED - charge {
            self.evict_oldest()?;
        }
        self.index
            .try_reserve(1)
            .map_err(|_| StorageError::ResourceLimit)?;
        if self.free_slots.is_empty() {
            self.slots
                .try_reserve(1)
                .map_err(|_| StorageError::ResourceLimit)?;
        }
        let interned = self.retain_identity(identity)?;
        let map_key = Probe::new(interned, key)?.into_key()?;
        let slot = Slot {
            identity: interned,
            key: logical,
            bytes: output,
            work,
            charge,
            newer: NONE,
            older: NONE,
        };
        let index = match self.free_slots.pop() {
            Some(index) => {
                let entry = self
                    .slots
                    .get_mut(index as usize)
                    .ok_or(StorageError::IntegrityFailure)?;
                if entry.is_some() {
                    return Err(StorageError::IntegrityFailure);
                }
                *entry = Some(slot);
                index
            }
            None => {
                let index =
                    u32::try_from(self.slots.len()).map_err(|_| StorageError::ResourceLimit)?;
                if index == NONE {
                    return Err(StorageError::ResourceLimit);
                }
                self.slots.push(Some(slot));
                index
            }
        };
        if self.index.insert(map_key, index).is_some() {
            return Err(StorageError::IntegrityFailure);
        }
        self.push_head(index)?;
        self.used += charge;
        Ok(())
    }
}
fn validate_key(key: &[u8]) -> Result<(), StorageError> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES {
        return Err(StorageError::ResourceLimit);
    }
    Ok(())
}
fn copy(bytes: &[u8], extra: usize) -> Result<Zeroizing<Vec<u8>>, StorageError> {
    let mut output = Zeroizing::new(Vec::new());
    output
        .try_reserve_exact(
            bytes
                .len()
                .checked_add(extra)
                .ok_or(StorageError::ResourceLimit)?,
        )
        .map_err(|_| StorageError::ResourceLimit)?;
    output.extend_from_slice(bytes);
    Ok(output)
}

#[cfg(test)]
mod tests;
