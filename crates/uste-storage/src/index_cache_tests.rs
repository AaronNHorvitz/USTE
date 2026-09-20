use super::*;
use std::{cell::Cell, cmp::Ordering};

#[derive(Clone, Copy)]
struct CountedKey<'a> {
    identity: u64,
    comparisons: &'a Cell<usize>,
}
impl PartialEq for CountedKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
    }
}
impl Eq for CountedKey<'_> {}
impl PartialOrd for CountedKey<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for CountedKey<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.comparisons.set(self.comparisons.get() + 1);
        self.identity.cmp(&other.identity)
    }
}

#[test]
fn newest_cache_slot_avoids_ordered_search_without_changing_eviction_or_capacity() {
    let comparisons = Cell::new(0);
    let key = |identity| CountedKey {
        identity,
        comparisons: &comparisons,
    };
    let mut cache = CachePages::default();
    assert_eq!(cache.touch(&key(0)), None);
    for id in 0..8 {
        assert_eq!(cache.insert(key(id), id, 8).unwrap(), (id as usize, false));
    }
    let allocated = cache.allocated_metadata_bytes();
    comparisons.set(0);
    for _ in 0..10_000 {
        assert_eq!(cache.touch(&key(7)), Some(7));
        assert_eq!(cache.page_mut(7).copied(), Some(7));
    }
    assert_eq!(comparisons.get(), 0);
    assert_eq!(cache.allocated_metadata_bytes(), allocated);
    assert_eq!(cache.touch(&key(0)), Some(0));
    assert!(comparisons.get() > 0);
    comparisons.set(0);
    assert_eq!(cache.touch(&key(0)), Some(0));
    assert_eq!(comparisons.get(), 0);
    assert_eq!(cache.touch(&key(99)), None);
    assert_eq!(cache.newest, 0);
    assert!(cache.insert(key(0), 99, 8).is_err());
    assert_eq!(cache.page_mut(0).copied(), Some(0));
    // Touching zero moved it to newest; one, not zero, is now the oldest.
    assert_eq!(cache.insert(key(8), 8, 8).unwrap(), (1, true));
    assert_eq!(cache.touch(&key(1)), None);
    comparisons.set(0);
    assert_eq!(cache.touch(&key(8)), Some(1));
    assert_eq!(comparisons.get(), 0);
    assert_eq!(cache.allocated_metadata_bytes(), allocated);
    assert_eq!(cache.len(), 8);
}

#[test]
fn newest_cache_slot_matches_the_whole_key_and_single_slot_replacement() {
    let mut cache = CachePages::default();
    assert!(cache.insert([0; 8], 0, 0).is_err());
    for field in 0..8 {
        let mut key = [0; 8];
        key[field] = 1;
        assert_eq!(cache.touch(&key), None);
        assert_eq!(cache.insert(key, field, 1).unwrap(), (0, field != 0));
        for other in 0..8 {
            let mut candidate = key;
            candidate[other] ^= 2;
            assert_eq!(cache.touch(&candidate), None);
        }
        assert_eq!(cache.touch(&key), Some(0));
        assert_eq!(cache.page_mut(0).copied(), Some(field));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.slots[0].previous, NONE);
        assert_eq!(cache.slots[0].next, NONE);
    }
}
