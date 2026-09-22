//! Bounded positive lookup retention. The enclosing cache supplies owner/session authority.
use super::*;
use crate::ordered_commitment::{MAX_KEY_BYTES, MAX_VALUE_BYTES, OrderedCommitment};
use crate::packed_tree_lookup::{TreeLookupLimits, TreeLookupReport, TreeReadContext};
use crate::packed_tree_record::PackedLocator;
use std::{borrow::Borrow, collections::BTreeMap};
use zeroize::Zeroizing;

pub(super) const IDENTITY_BYTES: usize = 175;
const FIXED: usize = 4096;
const ENTRY: usize = 512;
pub(super) const MINIMUM: usize = 8192;
pub(crate) type CachedLookup = (Zeroizing<Vec<u8>>, TreeLookupReport);

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

struct RetainedIdentity(Zeroizing<[u8; IDENTITY_BYTES]>);

#[derive(Clone)]
struct IdentityKey(Arc<RetainedIdentity>);
impl Borrow<[u8; IDENTITY_BYTES]> for IdentityKey {
    fn borrow(&self) -> &[u8; IDENTITY_BYTES] {
        &self.0.0
    }
}
impl PartialEq for IdentityKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.0 == other.0.0
    }
}
impl Eq for IdentityKey {}
impl PartialOrd for IdentityKey {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for IdentityKey {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.0.0.cmp(&other.0.0)
    }
}

struct EntryKey {
    identity: IdentityKey,
    bytes: Zeroizing<Vec<u8>>,
}

#[derive(Clone)]
struct LogicalKey(Arc<EntryKey>);
impl Borrow<[u8]> for LogicalKey {
    fn borrow(&self) -> &[u8] {
        self.0.bytes.as_slice()
    }
}
impl PartialEq for LogicalKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.bytes.as_slice() == other.0.bytes.as_slice()
    }
}
impl Eq for LogicalKey {}
impl PartialOrd for LogicalKey {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for LogicalKey {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.0.bytes.as_slice().cmp(other.0.bytes.as_slice())
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
    values: BTreeMap<IdentityKey, BTreeMap<LogicalKey, Value>>,
    order: BTreeMap<u128, Arc<EntryKey>>,
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
            accounted_bytes: if self.order.is_empty() {
                0
            } else {
                FIXED + self.used
            },
            resident_values: self.order.len(),
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
        validate_key(key)?;
        let Some((stored, value)) = self
            .values
            .get(&identity.0)
            .and_then(|values| values.get_key_value(key))
        else {
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
        let stored = stored.0.clone();
        let ordered = self
            .order
            .remove(&old)
            .ok_or(StorageError::IntegrityFailure)?;
        if !Arc::ptr_eq(&ordered, &stored) {
            return Err(StorageError::IntegrityFailure);
        }
        self.order.insert(next, stored.clone());
        self.values
            .get_mut(&identity.0)
            .and_then(|values| values.get_mut(key))
            .ok_or(StorageError::IntegrityFailure)?
            .stamp = next;
        self.clock = next;
        Ok(Some((output, work)))
    }
    pub(super) fn get_with<R, F: FnOnce(&[u8]) -> R>(
        &mut self,
        identity: Identity,
        key: &[u8],
        limits: TreeLookupLimits,
        map: &mut Option<F>,
    ) -> Result<Option<(R, TreeLookupReport)>, StorageError> {
        validate_key(key)?;
        let Some((stored, value)) = self
            .values
            .get(&identity.0)
            .and_then(|values| values.get_key_value(key))
        else {
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
        let work = value.work;
        let old = value.stamp;
        let stored = stored.0.clone();
        let ordered = self
            .order
            .remove(&old)
            .ok_or(StorageError::IntegrityFailure)?;
        if !Arc::ptr_eq(&ordered, &stored) {
            return Err(StorageError::IntegrityFailure);
        }
        self.order.insert(next, stored.clone());
        self.values
            .get_mut(&identity.0)
            .and_then(|values| values.get_mut(key))
            .ok_or(StorageError::IntegrityFailure)?
            .stamp = next;
        self.clock = next;
        // Complete internal integrity/LRU updates before exposing plaintext to the mapper.
        // `R` cannot borrow from this argument, so cache ownership never escapes.
        let bytes = &self
            .values
            .get(&identity.0)
            .and_then(|values| values.get(key))
            .ok_or(StorageError::IntegrityFailure)?
            .bytes;
        let output = map.take().ok_or(StorageError::InvalidState)?(bytes);
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
        validate_key(key)?;
        if self
            .values
            .get(&identity.0)
            .is_some_and(|values| values.contains_key(key))
        {
            return Err(StorageError::InvalidState);
        }
        let estimated = ENTRY
            .checked_add(IDENTITY_BYTES)
            .and_then(|n| n.checked_add(key.len()))
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
        let logical = copy(key, 0)?;
        let output = copy(bytes, 0)?;
        let charge = ENTRY
            .checked_add(IDENTITY_BYTES)
            .and_then(|n| n.checked_add(logical.capacity()))
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
            let values = self
                .values
                .get_mut(&*oldest.identity.0.0)
                .ok_or(StorageError::IntegrityFailure)?;
            let removed = values
                .remove(oldest.bytes.as_slice())
                .ok_or(StorageError::IntegrityFailure)?;
            let empty = values.is_empty();
            if stamp != removed.stamp {
                return Err(StorageError::IntegrityFailure);
            }
            if empty && self.values.remove(&*oldest.identity.0.0).is_none() {
                return Err(StorageError::IntegrityFailure);
            }
            self.used = self
                .used
                .checked_sub(removed.charge)
                .ok_or(StorageError::IntegrityFailure)?;
            increment(&mut self.evictions, &mut self.overflowed);
        }
        let retained_identity = self
            .values
            .get_key_value(&identity.0)
            .map(|(identity, _)| identity.clone())
            .unwrap_or_else(|| IdentityKey(Arc::new(RetainedIdentity(Zeroizing::new(identity.0)))));
        let key = Arc::new(EntryKey {
            identity: retained_identity.clone(),
            bytes: logical,
        });
        self.order.insert(next, key.clone());
        self.values.entry(retained_identity).or_default().insert(
            LogicalKey(key),
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
