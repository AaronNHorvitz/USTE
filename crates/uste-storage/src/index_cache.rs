//! Safe slot-addressed LRU links; keys remain in the bounded ordered lookup map.
use super::*;

const NONE: usize = usize::MAX;

struct Slot<K, V> {
    key: K,
    page: V,
    previous: usize,
    next: usize,
}

pub(crate) struct CachePages<K, V> {
    keys: BTreeMap<K, usize>,
    slots: Vec<Slot<K, V>>,
    oldest: usize,
    newest: usize,
}

impl<K: Copy + Ord, V> Default for CachePages<K, V> {
    fn default() -> Self {
        Self {
            keys: BTreeMap::new(),
            slots: Vec::new(),
            oldest: NONE,
            newest: NONE,
        }
    }
}

impl<K: Copy + Ord, V> CachePages<K, V> {
    #[cfg(test)]
    pub(crate) fn allocated_metadata_bytes(&self) -> usize {
        self.slots.capacity() * size_of::<Slot<K, V>>()
            + self.keys.len() * 2 * size_of::<(K, usize)>()
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.keys.contains_key(key)
    }

    pub fn page_mut(&mut self, index: usize) -> Option<&mut V> {
        self.slots.get_mut(index).map(|slot| &mut slot.page)
    }

    pub fn touch(&mut self, key: &K) -> Option<usize> {
        let index = *self.keys.get(key)?;
        if index != self.newest {
            self.unlink(index);
            self.append(index);
        }
        Some(index)
    }

    /// Reserve before changing either map or links. Full caches reuse the oldest slot.
    pub fn insert(
        &mut self,
        key: K,
        page: V,
        capacity: usize,
    ) -> Result<(usize, bool), StorageError> {
        if capacity == 0 || self.len() > capacity || self.contains_key(&key) {
            return Err(StorageError::IntegrityFailure);
        }
        let evicted = self.len() == capacity;
        let slot = Slot {
            key,
            page,
            previous: NONE,
            next: NONE,
        };
        let index = if evicted {
            let index = self.oldest;
            let previous_key = self
                .slots
                .get(index)
                .ok_or(StorageError::IntegrityFailure)?
                .key;
            if self.keys.remove(&previous_key) != Some(index) {
                return Err(StorageError::IntegrityFailure);
            }
            self.unlink(index);
            // Release the old value; secret-owning values zeroize when their last owner drops.
            self.slots[index] = slot;
            index
        } else {
            self.slots
                .try_reserve(1)
                .map_err(|_| StorageError::ResourceLimit)?;
            let index = self.slots.len();
            self.slots.push(slot);
            index
        };
        self.keys.insert(key, index);
        self.append(index);
        Ok((index, evicted))
    }

    fn unlink(&mut self, index: usize) {
        let previous = self.slots[index].previous;
        let next = self.slots[index].next;
        if previous == NONE {
            self.oldest = next;
        } else {
            self.slots[previous].next = next;
        }
        if next == NONE {
            self.newest = previous;
        } else {
            self.slots[next].previous = previous;
        }
    }

    fn append(&mut self, index: usize) {
        self.slots[index].previous = self.newest;
        self.slots[index].next = NONE;
        if self.newest == NONE {
            self.oldest = index;
        } else {
            self.slots[self.newest].next = index;
        }
        self.newest = index;
    }
}

#[cfg(test)]
impl CachePages<CacheKey, CachedPage> {
    pub(super) fn reserved_slots(&self) -> usize {
        self.slots.capacity()
    }
    pub(super) fn resident_slot(&self, key: &CacheKey) -> Option<usize> {
        self.keys.get(key).copied()
    }
    pub(super) fn get_mut(&mut self, key: &CacheKey) -> Option<&mut CachedPage> {
        let index = *self.keys.get(key)?;
        self.page_mut(index)
    }
    pub(super) fn values(&self) -> impl Iterator<Item = &CachedPage> {
        self.slots.iter().map(|slot| &slot.page)
    }
    pub(super) fn ordered_keys(&self) -> impl Iterator<Item = CacheKey> + '_ {
        std::iter::successors((self.oldest != NONE).then_some(self.oldest), |index| {
            let next = self.slots[*index].next;
            (next != NONE).then_some(next)
        })
        .take(self.len() + 1)
        .map(|index| self.slots[index].key)
    }
    pub(super) fn assert_invariants(&self) {
        assert_eq!(self.keys.len(), self.slots.len());
        let ordered: Vec<_> = self.ordered_keys().collect();
        assert_eq!(ordered.len(), self.len());
        let mut seen = std::collections::BTreeSet::new();
        for key in ordered {
            assert!(seen.insert(key));
        }
        for (index, slot) in self.slots.iter().enumerate() {
            assert_eq!(self.keys.get(&slot.key), Some(&index));
            if slot.previous == NONE {
                assert_eq!(self.oldest, index);
            } else {
                assert_eq!(self.slots[slot.previous].next, index);
                assert!(self.slots[slot.previous].page.last_used < slot.page.last_used);
            }
            if slot.next == NONE {
                assert_eq!(self.newest, index);
            } else {
                assert_eq!(self.slots[slot.next].previous, index);
            }
        }
        if self.is_empty() {
            assert_eq!((self.oldest, self.newest), (NONE, NONE));
            assert_eq!(self.slots.capacity(), 0);
        }
        // Include reserved vector capacity and a conservative doubled inline map-entry cost.
        // This remains logical accounting, not a measurement of std's allocator/tree overhead.
        assert!(
            self.slots.capacity() * size_of::<Slot<CacheKey, CachedPage>>()
                + self.keys.len() * 2 * size_of::<(CacheKey, usize)>()
                <= CACHE_FIXED_OVERHEAD + self.len() * CACHE_ENTRY_OVERHEAD
        );
    }
    pub(super) fn inline_allowance() -> usize {
        2 * size_of::<Slot<CacheKey, CachedPage>>() + 2 * size_of::<(CacheKey, usize)>()
    }
}
