//! Bounded complete-range retention. The enclosing cache supplies owner/session authority.
//!
//! Resident ranges are indexed by a hash map keyed on the interned identity index, direction
//! and both bounds, and ordered by an intrusive exact least-recently-used list. Hit, miss,
//! eviction, bypass and logical accounting semantics match the earlier ordered-map form.
use super::*;
use crate::{
    ordered_commitment::{MAX_BRANCH_BITS, MAX_KEY_BYTES},
    packed_tree_cursor::{
        MAX_CURSOR_CANDIDATES, MAX_CURSOR_ENCODED_BYTES, MAX_CURSOR_PAGES, PackedCursorEntry,
        TreeCursorLimits, TreeCursorReport,
    },
};
use std::{
    borrow::Borrow,
    collections::HashMap,
    hash::{Hash, Hasher},
};
use zeroize::Zeroizing;

const FIXED: usize = 4096;
const RANGE: usize = 512;
const ITEM: usize = 128;
pub(super) const MINIMUM: usize = 8192;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackedRangeCacheReport {
    pub budget_bytes: usize,
    pub accounted_bytes: usize,
    pub maximum_accounted_bytes: usize,
    pub resident_ranges: usize,
    pub resident_entries: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub evicted_bytes: u64,
    pub oversized_bypasses: u64,
}

pub(crate) struct CachedRange<T = PackedCursorEntry> {
    pub(crate) entries: Vec<T>,
    pub(crate) report: TreeCursorReport,
}

struct StoredEntry {
    key: Zeroizing<Vec<u8>>,
    value: Zeroizing<Vec<u8>>,
}
/// One interned authenticated identity shared by every resident range under it.
struct RetainedIdentity {
    bytes: Zeroizing<[u8; lookup::IDENTITY_BYTES]>,
    live: usize,
}
/// Hash-map key: the interned identity index, direction, and both encoded bounds.
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
struct Slot {
    identity: u32,
    /// The complete hash key, retained so eviction can unindex without re-encoding.
    key: Zeroizing<Vec<u8>>,
    entries: Vec<StoredEntry>,
    work: TreeCursorReport,
    charge: usize,
    /// More recently used neighbour; `NONE` at the head.
    newer: u32,
    /// Less recently used neighbour; `NONE` at the tail.
    older: u32,
}
const NONE: u32 = u32::MAX;
pub(super) struct RangeCache {
    budget: usize,
    used: usize,
    maximum_used: usize,
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
    evicted_bytes: u64,
    bypasses: u64,
    overflowed: bool,
}
impl RangeCache {
    pub(super) fn new(budget: usize) -> Result<Self, StorageError> {
        if !(MINIMUM..=MAX_INDEX_CACHE_BYTES).contains(&budget) {
            return Err(StorageError::ResourceLimit);
        }
        Ok(Self {
            budget,
            used: 0,
            maximum_used: 0,
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
            evicted_bytes: 0,
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
    pub(super) fn report(&self) -> Result<PackedRangeCacheReport, StorageError> {
        if self.overflowed {
            return Err(StorageError::ResourceLimit);
        }
        let resident_entries = self
            .slots
            .iter()
            .flatten()
            .try_fold(0_usize, |total, slot| {
                total
                    .checked_add(slot.entries.len())
                    .ok_or(StorageError::ResourceLimit)
            })?;
        Ok(PackedRangeCacheReport {
            budget_bytes: self.budget,
            accounted_bytes: if self.index.is_empty() {
                0
            } else {
                FIXED
                    .checked_add(self.used)
                    .ok_or(StorageError::ResourceLimit)?
            },
            maximum_accounted_bytes: if self.maximum_used == 0 {
                0
            } else {
                FIXED
                    .checked_add(self.maximum_used)
                    .ok_or(StorageError::ResourceLimit)?
            },
            resident_ranges: self.index.len(),
            resident_entries,
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            evicted_bytes: self.evicted_bytes,
            oversized_bypasses: self.bypasses,
        })
    }
    fn identity_index(&self, identity: &LookupIdentity) -> Option<u32> {
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
    fn promote(&mut self, index: u32) -> Result<(), StorageError> {
        if self.head == index {
            return Ok(());
        }
        self.unlink(index)?;
        self.push_head(index)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn get(
        &mut self,
        identity: LookupIdentity,
        lower: &[u8],
        upper: Option<&[u8]>,
        reverse: bool,
        limits: TreeCursorLimits,
    ) -> Result<Option<CachedRange>, StorageError> {
        self.get_with(
            identity,
            lower,
            upper,
            reverse,
            limits,
            &mut |key, value| PackedCursorEntry::copy_from_slices(key, value),
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn get_with<T, M>(
        &mut self,
        identity: LookupIdentity,
        lower: &[u8],
        upper: Option<&[u8]>,
        reverse: bool,
        limits: TreeCursorLimits,
        map: &mut impl FnMut(&[u8], &[u8]) -> Result<T, M>,
    ) -> Result<Option<CachedRange<T>>, M>
    where
        M: From<StorageError>,
    {
        validate_bounds(lower, upper).map_err(M::from)?;
        let found = match self.identity_index(&identity) {
            Some(interned) => {
                let probe = encode_key(interned, lower, upper, reverse).map_err(M::from)?;
                self.index.get(probe.as_slice()).copied()
            }
            None => None,
        };
        let Some(index) = found else {
            increment(&mut self.misses, &mut self.overflowed);
            return Ok(None);
        };
        increment(&mut self.hits, &mut self.overflowed);
        let slot = self.slot(index).map_err(M::from)?;
        admit(slot.work, limits)?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(slot.entries.len())
            .map_err(|_| M::from(StorageError::ResourceLimit))?;
        let mut map_error = None;
        for entry in &slot.entries {
            match map(&entry.key, &entry.value) {
                Ok(mapped) if map_error.is_none() => entries.push(mapped),
                Ok(_) => {}
                Err(error) if map_error.is_none() => map_error = Some(error),
                Err(_) => {}
            }
        }
        let work = slot.work;
        self.promote(index).map_err(M::from)?;
        if let Some(error) = map_error {
            return Err(error);
        }
        Ok(Some(CachedRange {
            entries,
            report: work,
        }))
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
        if self.index.remove(removed.key.as_slice()) != Some(index) {
            return Err(StorageError::IntegrityFailure);
        }
        self.release_identity(removed.identity)?;
        self.free_slots.push(index);
        self.used = self
            .used
            .checked_sub(removed.charge)
            .ok_or(StorageError::IntegrityFailure)?;
        increment(&mut self.evictions, &mut self.overflowed);
        if let Ok(bytes) = u64::try_from(removed.charge) {
            if let Some(total) = self.evicted_bytes.checked_add(bytes) {
                self.evicted_bytes = total;
            } else {
                self.overflowed = true;
            }
        } else {
            self.overflowed = true;
        }
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
    fn retain_identity(&mut self, identity: LookupIdentity) -> Result<u32, StorageError> {
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
    #[allow(clippy::too_many_arguments)]
    pub(super) fn insert(
        &mut self,
        identity: LookupIdentity,
        lower: &[u8],
        upper: Option<&[u8]>,
        reverse: bool,
        entries: &[PackedCursorEntry],
        work: TreeCursorReport,
    ) -> Result<(), StorageError> {
        validate_result(lower, upper, reverse, entries, work)?;
        validate_bounds(lower, upper)?;
        if let Some(interned) = self.identity_index(&identity)
            && self
                .index
                .contains_key(encode_key(interned, lower, upper, reverse)?.as_slice())
        {
            return Err(StorageError::InvalidState);
        }
        let payload = entries.iter().try_fold(0_usize, |total, entry| {
            total
                .checked_add(ITEM)
                .and_then(|n| n.checked_add(entry.key().len()))
                .and_then(|n| n.checked_add(entry.value().len()))
                .ok_or(StorageError::ResourceLimit)
        })?;
        // Logical charge: fixed range allowance, the complete logical key including the full
        // 175-byte identity, and every item's fixed allowance plus exact key and value bytes.
        let charge = RANGE
            .checked_add(logical_key_bytes(lower, upper)?)
            .and_then(|n| n.checked_add(payload))
            .ok_or(StorageError::ResourceLimit)?;
        if charge > self.budget - FIXED {
            increment(&mut self.bypasses, &mut self.overflowed);
            return Ok(());
        }
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(entries.len())
            .map_err(|_| StorageError::ResourceLimit)?;
        for entry in entries {
            retained.push(StoredEntry {
                key: copy(entry.key())?,
                value: copy(entry.value())?,
            });
        }
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
        let key = encode_key(interned, lower, upper, reverse)?;
        let slot = Slot {
            identity: interned,
            key: copy(&key)?,
            entries: retained,
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
        if self.index.insert(MapKey(key), index).is_some() {
            return Err(StorageError::IntegrityFailure);
        }
        self.push_head(index)?;
        self.used = self
            .used
            .checked_add(charge)
            .ok_or(StorageError::ResourceLimit)?;
        self.maximum_used = self.maximum_used.max(self.used);
        Ok(())
    }
}

fn admit(work: TreeCursorReport, limits: TreeCursorLimits) -> Result<(), StorageError> {
    if work.path_branches > limits.maximum_path_branches
        || work.candidates > limits.maximum_candidates
        || work.returned_bytes > limits.maximum_returned_bytes
        || work.pages > limits.maximum_pages
        || work.encoded_bytes > limits.maximum_encoded_bytes
    {
        return Err(StorageError::ResourceLimit);
    }
    Ok(())
}

fn validate_result(
    lower: &[u8],
    upper: Option<&[u8]>,
    reverse: bool,
    entries: &[PackedCursorEntry],
    work: TreeCursorReport,
) -> Result<(), StorageError> {
    if work.path_branches > MAX_BRANCH_BITS
        || work.candidates > MAX_CURSOR_CANDIDATES
        || work.pages > MAX_CURSOR_PAGES
        || work.encoded_bytes > MAX_CURSOR_ENCODED_BYTES
        || work.pages.checked_mul(ENCODED_PAGE_BYTES as u64) != Some(work.encoded_bytes)
        || u64::from(work.path_branches) > work.pages
        || work.value_chunks > work.pages
        || work.returned_entries != entries.len() as u64
        || work.candidates < work.returned_entries
    {
        return Err(StorageError::InvalidState);
    }
    let returned = entries.iter().try_fold(0_u64, |total, entry| {
        total
            .checked_add(entry.key().len() as u64)
            .and_then(|n| n.checked_add(entry.value().len() as u64))
            .ok_or(StorageError::ResourceLimit)
    })?;
    if returned != work.returned_bytes {
        return Err(StorageError::InvalidState);
    }
    let mut previous: Option<&[u8]> = None;
    for entry in entries {
        let key = entry.key();
        let in_range = if reverse {
            key > lower && upper.is_none_or(|end| key <= end)
        } else {
            key >= lower && upper.is_none_or(|end| key < end)
        };
        let ordered = previous.is_none_or(|old| if reverse { key < old } else { key > old });
        if !in_range || !ordered {
            return Err(StorageError::InvalidState);
        }
        previous = Some(key);
    }
    Ok(())
}

fn validate_bounds(lower: &[u8], upper: Option<&[u8]>) -> Result<(), StorageError> {
    if upper.is_some_and(|end| lower > end) {
        return Err(StorageError::InvalidState);
    }
    if lower.len() > MAX_KEY_BYTES || upper.is_some_and(|end| end.len() > MAX_KEY_BYTES) {
        return Err(StorageError::ResourceLimit);
    }
    Ok(())
}

/// Logical key length charged for accounting. This reproduces exactly the reserved capacity of
/// the earlier owned key: the complete 175-byte identity, six framing bytes and both bounds.
/// The six framing bytes are charged even for an unbounded upper range, as before.
fn logical_key_bytes(lower: &[u8], upper: Option<&[u8]>) -> Result<usize, StorageError> {
    lookup::IDENTITY_BYTES
        .checked_add(6)
        .and_then(|n| n.checked_add(lower.len()))
        .and_then(|n| n.checked_add(upper.map_or(0, <[u8]>::len)))
        .ok_or(StorageError::ResourceLimit)
}

fn encode_key(
    identity: u32,
    lower: &[u8],
    upper: Option<&[u8]>,
    reverse: bool,
) -> Result<Zeroizing<Vec<u8>>, StorageError> {
    validate_bounds(lower, upper)?;
    let mut bytes = Zeroizing::new(Vec::new());
    let upper_bytes = upper.map_or(0, <[u8]>::len);
    bytes
        .try_reserve_exact(
            10_usize
                .checked_add(lower.len())
                .and_then(|n| n.checked_add(upper_bytes))
                .ok_or(StorageError::ResourceLimit)?,
        )
        .map_err(|_| StorageError::ResourceLimit)?;
    bytes.extend_from_slice(&identity.to_be_bytes());
    bytes.push(u8::from(reverse));
    bytes.extend_from_slice(
        &u16::try_from(lower.len())
            .map_err(|_| StorageError::ResourceLimit)?
            .to_be_bytes(),
    );
    bytes.extend_from_slice(lower);
    match upper {
        None => bytes.push(0),
        Some(upper) => {
            bytes.push(1);
            bytes.extend_from_slice(
                &u16::try_from(upper.len())
                    .map_err(|_| StorageError::ResourceLimit)?
                    .to_be_bytes(),
            );
            bytes.extend_from_slice(upper);
        }
    }
    Ok(bytes)
}

fn copy(bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>, StorageError> {
    let mut output = Zeroizing::new(Vec::new());
    output
        .try_reserve_exact(bytes.len())
        .map_err(|_| StorageError::ResourceLimit)?;
    output.extend_from_slice(bytes);
    Ok(output)
}

#[cfg(test)]
mod tests;
