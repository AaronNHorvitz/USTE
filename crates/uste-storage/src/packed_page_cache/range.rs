//! Bounded complete-range retention. The enclosing cache supplies owner/session authority.
use super::*;
use crate::{
    ordered_commitment::{MAX_BRANCH_BITS, MAX_KEY_BYTES},
    packed_tree_cursor::{
        MAX_CURSOR_CANDIDATES, MAX_CURSOR_ENCODED_BYTES, MAX_CURSOR_PAGES, PackedCursorEntry,
        TreeCursorLimits, TreeCursorReport,
    },
};
use std::{borrow::Borrow, collections::BTreeMap};
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
struct EntryKey {
    bytes: Zeroizing<Vec<u8>>,
}
#[derive(Clone)]
struct RangeKey(Arc<EntryKey>);
impl Borrow<[u8]> for RangeKey {
    fn borrow(&self) -> &[u8] {
        self.0.bytes.as_slice()
    }
}
impl PartialEq for RangeKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.bytes == other.0.bytes
    }
}
impl Eq for RangeKey {}
impl PartialOrd for RangeKey {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for RangeKey {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.0.bytes.cmp(&other.0.bytes)
    }
}
struct Value {
    entries: Vec<StoredEntry>,
    work: TreeCursorReport,
    stamp: u128,
    charge: usize,
}
pub(super) struct RangeCache {
    budget: usize,
    used: usize,
    maximum_used: usize,
    clock: u128,
    values: BTreeMap<RangeKey, Value>,
    order: BTreeMap<u128, Arc<EntryKey>>,
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
            clock: 0,
            values: BTreeMap::new(),
            order: BTreeMap::new(),
            hits: 0,
            misses: 0,
            evictions: 0,
            evicted_bytes: 0,
            bypasses: 0,
            overflowed: false,
        })
    }
    pub(super) fn clear(&mut self) {
        self.values.clear();
        self.order.clear();
        self.used = 0;
        self.clock = 0;
    }
    pub(super) fn report(&self) -> Result<PackedRangeCacheReport, StorageError> {
        if self.overflowed {
            return Err(StorageError::ResourceLimit);
        }
        let resident_entries = self.values.values().try_fold(0_usize, |total, value| {
            total
                .checked_add(value.entries.len())
                .ok_or(StorageError::ResourceLimit)
        })?;
        Ok(PackedRangeCacheReport {
            budget_bytes: self.budget,
            accounted_bytes: if self.values.is_empty() {
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
            resident_ranges: self.values.len(),
            resident_entries,
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            evicted_bytes: self.evicted_bytes,
            oversized_bypasses: self.bypasses,
        })
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
        let logical = encode_key(identity, lower, upper, reverse).map_err(M::from)?;
        let Some((stored, value)) = self.values.get_key_value(logical.as_slice()) else {
            increment(&mut self.misses, &mut self.overflowed);
            return Ok(None);
        };
        let next = self
            .clock
            .checked_add(1)
            .ok_or(StorageError::ResourceLimit)?;
        increment(&mut self.hits, &mut self.overflowed);
        admit(value.work, limits)?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(value.entries.len())
            .map_err(|_| M::from(StorageError::ResourceLimit))?;
        let mut map_error = None;
        for entry in &value.entries {
            match map(&entry.key, &entry.value) {
                Ok(mapped) if map_error.is_none() => entries.push(mapped),
                Ok(_) => {}
                Err(error) if map_error.is_none() => map_error = Some(error),
                Err(_) => {}
            }
        }
        let work = value.work;
        let old = value.stamp;
        let stored = stored.0.clone();
        let ordered = self
            .order
            .remove(&old)
            .ok_or(StorageError::IntegrityFailure)?;
        if !Arc::ptr_eq(&ordered, &stored) {
            return Err(M::from(StorageError::IntegrityFailure));
        }
        self.order.insert(next, stored.clone());
        self.values
            .get_mut(logical.as_slice())
            .ok_or(StorageError::IntegrityFailure)?
            .stamp = next;
        self.clock = next;
        if let Some(error) = map_error {
            return Err(error);
        }
        Ok(Some(CachedRange {
            entries,
            report: work,
        }))
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
        let logical = encode_key(identity, lower, upper, reverse)?;
        if self.values.contains_key(logical.as_slice()) {
            return Err(StorageError::InvalidState);
        }
        let payload = entries.iter().try_fold(0_usize, |total, entry| {
            total
                .checked_add(ITEM)
                .and_then(|n| n.checked_add(entry.key().len()))
                .and_then(|n| n.checked_add(entry.value().len()))
                .ok_or(StorageError::ResourceLimit)
        })?;
        let estimated = RANGE
            .checked_add(logical.len())
            .and_then(|n| n.checked_add(payload))
            .ok_or(StorageError::ResourceLimit)?;
        if estimated > self.budget - FIXED {
            increment(&mut self.bypasses, &mut self.overflowed);
            return Ok(());
        }
        let next = self
            .clock
            .checked_add(1)
            .ok_or(StorageError::ResourceLimit)?;
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
        let charge = RANGE
            .checked_add(logical.capacity())
            .and_then(|n| {
                retained.iter().try_fold(n, |total, entry| {
                    total
                        .checked_add(ITEM)
                        .and_then(|n| n.checked_add(entry.key.capacity()))
                        .and_then(|n| n.checked_add(entry.value.capacity()))
                })
            })
            .ok_or(StorageError::ResourceLimit)?;
        if charge > self.budget - FIXED {
            increment(&mut self.bypasses, &mut self.overflowed);
            return Ok(());
        }
        while self.used > self.budget - FIXED - charge {
            let (stamp, oldest) = self
                .order
                .pop_first()
                .ok_or(StorageError::IntegrityFailure)?;
            let removed = self
                .values
                .remove(oldest.bytes.as_slice())
                .ok_or(StorageError::IntegrityFailure)?;
            if stamp != removed.stamp {
                return Err(StorageError::IntegrityFailure);
            }
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
        }
        let key = Arc::new(EntryKey { bytes: logical });
        self.order.insert(next, key.clone());
        self.values.insert(
            RangeKey(key),
            Value {
                entries: retained,
                work,
                stamp: next,
                charge,
            },
        );
        self.used = self
            .used
            .checked_add(charge)
            .ok_or(StorageError::ResourceLimit)?;
        self.maximum_used = self.maximum_used.max(self.used);
        self.clock = next;
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

fn encode_key(
    identity: LookupIdentity,
    lower: &[u8],
    upper: Option<&[u8]>,
    reverse: bool,
) -> Result<Zeroizing<Vec<u8>>, StorageError> {
    if upper.is_some_and(|end| lower > end) {
        return Err(StorageError::InvalidState);
    }
    if lower.len() > MAX_KEY_BYTES || upper.is_some_and(|end| end.len() > MAX_KEY_BYTES) {
        return Err(StorageError::ResourceLimit);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    let upper_bytes = upper.map_or(0, <[u8]>::len);
    bytes
        .try_reserve_exact(
            lookup::IDENTITY_BYTES
                .checked_add(6)
                .and_then(|n| n.checked_add(lower.len()))
                .and_then(|n| n.checked_add(upper_bytes))
                .ok_or(StorageError::ResourceLimit)?,
        )
        .map_err(|_| StorageError::ResourceLimit)?;
    bytes.extend_from_slice(&identity.0);
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
