use super::*;

#[test]
fn packed_cache_budget_diagnostics_clear_and_counter_overflow_are_explicit() {
    assert!(PackedPageCache::new(MIN_INDEX_CACHE_BYTES - 1).is_err());
    assert!(PackedPageCache::new(MAX_INDEX_CACHE_BYTES + 1).is_err());
    for budget in [MIN_INDEX_CACHE_BYTES, MAX_INDEX_CACHE_BYTES] {
        let mut cache = PackedPageCache::new(budget).unwrap();
        cache.assert_metadata_allowance();
        assert_eq!(cache.report().unwrap().accounted_bytes, 0);
        assert_eq!(cache.report().unwrap().budget_bytes, budget);
        let debug = format!("{cache:?}");
        assert!(!debug.contains("owner"));
        assert!(!debug.contains("session"));
        assert!(!debug.contains("context"));
        cache.hits = u64::MAX;
        increment(&mut cache.hits, &mut cache.overflowed);
        assert_eq!(cache.report(), Err(StorageError::ResourceLimit));
        cache.clear();
        assert_eq!(cache.report(), Err(StorageError::ResourceLimit));
        cache.assert_metadata_allowance();
    }
}
