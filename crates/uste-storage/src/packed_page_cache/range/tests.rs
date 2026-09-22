use super::*;

fn identity(byte: u8) -> LookupIdentity {
    LookupIdentity([byte; lookup::IDENTITY_BYTES])
}
fn entry(key: &[u8], value: &[u8]) -> PackedCursorEntry {
    PackedCursorEntry::copy_from_slices(key, value).unwrap()
}
fn work(entries: &[PackedCursorEntry]) -> TreeCursorReport {
    TreeCursorReport {
        candidates: entries.len() as u64 + 1,
        returned_entries: entries.len() as u64,
        returned_bytes: entries
            .iter()
            .map(|entry| (entry.key().len() + entry.value().len()) as u64)
            .sum(),
        pages: 7,
        encoded_bytes: 7 * ENCODED_PAGE_BYTES as u64,
        path_branches: 3,
        value_chunks: 1,
    }
}
fn limits(work: TreeCursorReport) -> TreeCursorLimits {
    TreeCursorLimits {
        maximum_path_branches: work.path_branches,
        maximum_candidates: work.candidates,
        maximum_returned_bytes: work.returned_bytes,
        maximum_pages: work.pages,
        maximum_encoded_bytes: work.encoded_bytes,
    }
}
fn assert_invariants(cache: &RangeCache) {
    assert_eq!(cache.values.len(), cache.order.len());
    assert_eq!(
        cache.used,
        cache.values.values().map(|value| value.charge).sum()
    );
    for (key, value) in &cache.values {
        assert!(
            cache
                .order
                .get(&value.stamp)
                .is_some_and(|ordered| Arc::ptr_eq(ordered, &key.0))
        );
        assert_eq!(Arc::strong_count(&key.0), 2);
    }
    assert!(cache.report().unwrap().accounted_bytes <= cache.budget);
    let report = cache.report().unwrap();
    assert!(report.maximum_accounted_bytes >= report.accounted_bytes);
    assert!(report.maximum_accounted_bytes <= cache.budget);
    assert!(size_of::<RangeCache>() <= FIXED);
}

#[test]
fn complete_ranges_bind_identity_direction_bounds_and_every_limit() {
    let forward = vec![entry(b"a", b"one"), entry(b"b", b"two")];
    let report = work(&forward);
    let mut cache = RangeCache::new(4 * MINIMUM).unwrap();
    cache
        .insert(identity(7), b"a", Some(b"c"), false, &forward, report)
        .unwrap();
    for variant in 0..4 {
        let (id, lower, upper, reverse) = match variant {
            0 => (identity(8), b"a".as_slice(), Some(b"c".as_slice()), false),
            1 => (identity(7), b"".as_slice(), Some(b"c".as_slice()), false),
            2 => (identity(7), b"a".as_slice(), Some(b"d".as_slice()), false),
            _ => (identity(7), b"a".as_slice(), Some(b"c".as_slice()), true),
        };
        assert!(
            cache
                .get(id, lower, upper, reverse, limits(report))
                .unwrap()
                .is_none()
        );
    }
    for field in 0..5 {
        let mut narrow = limits(report);
        match field {
            0 => narrow.maximum_path_branches -= 1,
            1 => narrow.maximum_candidates -= 1,
            2 => narrow.maximum_returned_bytes -= 1,
            3 => narrow.maximum_pages -= 1,
            _ => narrow.maximum_encoded_bytes -= 1,
        }
        let clock = cache.clock;
        assert!(matches!(
            cache.get(identity(7), b"a", Some(b"c"), false, narrow),
            Err(StorageError::ResourceLimit)
        ));
        assert_eq!(cache.clock, clock);
    }
    let hit = cache
        .get(identity(7), b"a", Some(b"c"), false, limits(report))
        .unwrap()
        .unwrap();
    assert_eq!(hit.report, report);
    assert_eq!(
        hit.entries
            .iter()
            .map(|entry| (entry.key().to_vec(), entry.value().to_vec()))
            .collect::<Vec<_>>(),
        [
            (b"a".to_vec(), b"one".to_vec()),
            (b"b".to_vec(), b"two".to_vec())
        ]
    );
    assert_invariants(&cache);
}

#[test]
fn complete_range_lru_bypass_clear_and_validation_are_explicit() {
    let mut cache = RangeCache::new(MINIMUM).unwrap();
    let entries = vec![entry(b"a", &[1; 800])];
    let report = work(&entries);
    let mut charge = None;
    for byte in 0..12 {
        cache
            .insert(identity(byte), b"", None, false, &entries, report)
            .unwrap();
        if charge.is_none() {
            charge = Some(cache.values.values().next().unwrap().charge);
        }
        assert_invariants(&cache);
    }
    let pressured = cache.report().unwrap();
    assert!(pressured.evictions > 0);
    assert_eq!(
        pressured.evicted_bytes,
        pressured.evictions * u64::try_from(charge.unwrap()).unwrap()
    );
    assert!(pressured.maximum_accounted_bytes > 0);
    let resident = cache.report().unwrap().resident_ranges;
    let oversized = vec![entry(b"a", &vec![2; MINIMUM])];
    cache
        .insert(identity(99), b"", None, false, &oversized, work(&oversized))
        .unwrap();
    assert_eq!(cache.report().unwrap().resident_ranges, resident);
    assert_eq!(cache.report().unwrap().oversized_bypasses, 1);

    let invalid = [
        vec![entry(b"b", b"1"), entry(b"a", b"2")],
        vec![entry(b"a", b"1"), entry(b"a", b"2")],
        vec![entry(b"z", b"1")],
    ];
    for entries in &invalid {
        assert!(
            cache
                .insert(
                    identity(120),
                    b"a",
                    Some(b"c"),
                    false,
                    entries,
                    work(entries),
                )
                .is_err()
        );
    }
    let counters = cache.report().unwrap();
    cache.clear();
    let cleared = cache.report().unwrap();
    assert_eq!(cleared.accounted_bytes, 0);
    assert_eq!(cleared.resident_ranges, 0);
    assert_eq!(cleared.hits, counters.hits);
    assert_eq!(cleared.misses, counters.misses);
    assert_eq!(cleared.evictions, counters.evictions);
    assert_eq!(cleared.evicted_bytes, counters.evicted_bytes);
    assert_eq!(
        cleared.maximum_accounted_bytes,
        counters.maximum_accounted_bytes
    );
    assert_invariants(&cache);

    let mut overflow = RangeCache::new(MINIMUM).unwrap();
    overflow.evicted_bytes = u64::MAX;
    for byte in 0..12 {
        overflow
            .insert(identity(byte), b"", None, false, &entries, report)
            .unwrap();
    }
    assert!(matches!(
        overflow.report(),
        Err(StorageError::ResourceLimit)
    ));
}
