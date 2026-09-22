use super::*;

#[test]
fn positive_lookup_partition_is_within_one_explicit_total_budget() {
    let minimum = MIN_INDEX_CACHE_BYTES + lookup::MINIMUM;
    for (total, part) in [
        (minimum - 1, lookup::MINIMUM),
        (minimum, lookup::MINIMUM - 1),
        (minimum, 0),
        (minimum, minimum + 1),
        (MAX_INDEX_CACHE_BYTES + 1, lookup::MINIMUM),
    ] {
        assert!(PackedPageCache::new_with_lookup_budget(total, part).is_err());
    }
    for total in [minimum, MAX_INDEX_CACHE_BYTES] {
        let mut cache = PackedPageCache::new_with_lookup_budget(total, lookup::MINIMUM).unwrap();
        let report = cache.report().unwrap();
        assert_eq!(report.budget_bytes, total);
        assert_eq!(report.page_budget_bytes, total - lookup::MINIMUM);
        assert_eq!(report.lookup.unwrap().budget_bytes, lookup::MINIMUM);
        assert_eq!(report.range, None);
        assert_eq!(report.accounted_bytes, 0);
        cache.assert_metadata_allowance();
        cache.clear();
        assert_eq!(cache.report().unwrap(), report);
    }
    let report = PackedPageCache::new(minimum).unwrap().report().unwrap();
    assert_eq!(report.lookup, None);
    assert_eq!(report.range, None);
    assert_eq!(report.page_budget_bytes, minimum);
}

#[test]
fn complete_range_partition_is_within_the_same_explicit_total_budget() {
    let minimum = MIN_INDEX_CACHE_BYTES + lookup::MINIMUM + range::MINIMUM;
    for (total, lookup, range) in [
        (minimum - 1, lookup::MINIMUM, range::MINIMUM),
        (minimum, lookup::MINIMUM - 1, range::MINIMUM),
        (minimum, lookup::MINIMUM, range::MINIMUM - 1),
        (minimum, lookup::MINIMUM, minimum),
        (MAX_INDEX_CACHE_BYTES + 1, lookup::MINIMUM, range::MINIMUM),
    ] {
        assert!(PackedPageCache::new_with_lookup_and_range_budget(total, lookup, range).is_err());
    }
    let mut cache =
        PackedPageCache::new_with_lookup_and_range_budget(minimum, lookup::MINIMUM, range::MINIMUM)
            .unwrap();
    let report = cache.report().unwrap();
    assert_eq!(report.budget_bytes, minimum);
    assert_eq!(report.page_budget_bytes, MIN_INDEX_CACHE_BYTES);
    assert_eq!(report.lookup.unwrap().budget_bytes, lookup::MINIMUM);
    assert_eq!(report.range.unwrap().budget_bytes, range::MINIMUM);
    assert_eq!(report.accounted_bytes, 0);
    cache.clear();
    assert_eq!(cache.report().unwrap(), report);
}

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
