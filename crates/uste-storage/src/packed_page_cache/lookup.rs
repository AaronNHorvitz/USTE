//! Bounded positive lookup retention. The enclosing cache supplies owner/session authority.
use super::*;
use crate::ordered_commitment::{MAX_KEY_BYTES, MAX_VALUE_BYTES, OrderedCommitment};
use crate::packed_tree_lookup::{TreeLookupLimits, TreeLookupReport, TreeReadContext};
use crate::packed_tree_record::PackedLocator;
use std::{borrow::Borrow, collections::BTreeMap};
use zeroize::Zeroizing;

const IDENTITY_BYTES: usize = 175;
const FIXED: usize = 4096;
const ENTRY: usize = 512;
pub(super) const MINIMUM: usize = 8192;
pub(crate) type CachedLookup = (Zeroizing<Vec<u8>>, TreeLookupReport);

#[derive(Clone, Copy)]
pub(crate) struct Identity([u8; IDENTITY_BYTES]);
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
    fn key(self, key: &[u8]) -> Result<Zeroizing<Vec<u8>>, StorageError> {
        if key.is_empty() || key.len() > MAX_KEY_BYTES {
            return Err(StorageError::ResourceLimit);
        }
        // Most searches distinguish logical keys within one root. Avoid comparing the
        // shared identity prefix on every ordered-map branch. The fixed-size suffix
        // keeps this encoding injective even for variable-length/prefix-related keys.
        let mut bytes = copy(key, IDENTITY_BYTES)?;
        bytes.extend_from_slice(&self.0);
        Ok(bytes)
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

#[derive(Clone)]
struct Key(Arc<Zeroizing<Vec<u8>>>);
impl Borrow<[u8]> for Key {
    fn borrow(&self) -> &[u8] {
        self.0.as_slice()
    }
}
impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_slice() == other.0.as_slice()
    }
}
impl Eq for Key {}
impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Key {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.0.as_slice().cmp(other.0.as_slice())
    }
}
struct Value {
    bytes: Zeroizing<Vec<u8>>,
    work: TreeLookupReport,
    stamp: u128,
    charge: usize,
}
pub(super) struct LookupCache {
    budget: usize,
    used: usize,
    clock: u128,
    values: BTreeMap<Key, Value>,
    order: BTreeMap<u128, Key>,
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
            clock: 0,
            values: BTreeMap::new(),
            order: BTreeMap::new(),
            hits: 0,
            misses: 0,
            evictions: 0,
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
    pub(super) fn report(&self) -> Result<PackedLookupCacheReport, StorageError> {
        if self.overflowed {
            return Err(StorageError::ResourceLimit);
        }
        Ok(PackedLookupCacheReport {
            budget_bytes: self.budget,
            accounted_bytes: if self.values.is_empty() {
                0
            } else {
                FIXED + self.used
            },
            resident_values: self.values.len(),
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            oversized_bypasses: self.bypasses,
        })
    }
    pub(super) fn get(
        &mut self,
        identity: Identity,
        key: &[u8],
        limits: TreeLookupLimits,
    ) -> Result<Option<CachedLookup>, StorageError> {
        let encoded = identity.key(key)?;
        let Some((stored, value)) = self.values.get_key_value(encoded.as_slice()) else {
            increment(&mut self.misses, &mut self.overflowed);
            return Ok(None);
        };
        let next = self
            .clock
            .checked_add(1)
            .ok_or(StorageError::ResourceLimit)?;
        increment(&mut self.hits, &mut self.overflowed);
        if value.work.pages > limits.maximum_pages
            || value.work.encoded_bytes > limits.maximum_encoded_bytes
            || value.work.path_branches > limits.maximum_path_branches
            || value.bytes.len() as u64 > limits.maximum_value_bytes
        {
            return Err(StorageError::ResourceLimit);
        }
        let output = copy(&value.bytes, 0)?;
        let work = value.work;
        let old = value.stamp;
        let stored = stored.clone();
        if self.order.remove(&old).as_ref() != Some(&stored) {
            return Err(StorageError::IntegrityFailure);
        }
        self.order.insert(next, stored.clone());
        self.values
            .get_mut(&stored)
            .ok_or(StorageError::IntegrityFailure)?
            .stamp = next;
        self.clock = next;
        Ok(Some((output, work)))
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
        let encoded = identity.key(key)?;
        if self.values.contains_key(encoded.as_slice()) {
            return Err(StorageError::InvalidState);
        }
        let estimated = ENTRY
            .checked_add(encoded.capacity())
            .and_then(|n| n.checked_add(bytes.len()))
            .ok_or(StorageError::ResourceLimit)?;
        if estimated > self.budget - FIXED {
            increment(&mut self.bypasses, &mut self.overflowed);
            return Ok(());
        }
        let next = self
            .clock
            .checked_add(1)
            .ok_or(StorageError::ResourceLimit)?;
        let output = copy(bytes, 0)?;
        let charge = ENTRY
            .checked_add(encoded.capacity())
            .and_then(|n| n.checked_add(output.capacity()))
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
                .remove(&oldest)
                .ok_or(StorageError::IntegrityFailure)?;
            if stamp != removed.stamp {
                return Err(StorageError::IntegrityFailure);
            }
            self.used = self
                .used
                .checked_sub(removed.charge)
                .ok_or(StorageError::IntegrityFailure)?;
            increment(&mut self.evictions, &mut self.overflowed);
        }
        let key = Key(Arc::new(encoded));
        self.order.insert(next, key.clone());
        self.values.insert(
            key,
            Value {
                bytes: output,
                work,
                stamp: next,
                charge,
            },
        );
        self.used += charge;
        self.clock = next;
        Ok(())
    }
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
