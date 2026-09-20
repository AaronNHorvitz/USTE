use super::*;

fn work() -> TreeLookupReport {
    TreeLookupReport {
        pages: 4,
        encoded_bytes: 4 * ENCODED_PAGE_BYTES as u64,
        path_branches: 2,
        value_chunks: 1,
    }
}
fn limits() -> TreeLookupLimits {
    TreeLookupLimits {
        maximum_path_branches: 2,
        maximum_pages: 4,
        maximum_encoded_bytes: work().encoded_bytes,
        maximum_value_bytes: 100,
    }
}
fn assert_invariants(cache: &LookupCache) {
    assert_eq!(cache.values.len(), cache.order.len());
    assert_eq!(cache.used, cache.values.values().map(|v| v.charge).sum());
    for (key, value) in &cache.values {
        assert!(cache.order.get(&value.stamp) == Some(key));
        assert_eq!(
            value.charge,
            ENTRY + key.0.capacity() + value.bytes.capacity()
        );
        assert_eq!(Arc::strong_count(&key.0), 2);
    }
    assert!(cache.report().unwrap().accounted_bytes <= cache.budget);
    // Conservative logical allowance, not a claim about allocator/RSS behavior.
    assert!(
        3 * size_of::<(Key, Value)>()
            + 3 * size_of::<(u128, Key)>()
            + size_of::<Zeroizing<Vec<u8>>>()
            + 2 * size_of::<usize>()
            <= ENTRY
    );
    assert!(size_of::<LookupCache>() <= FIXED);
}

#[test]
fn positive_lookup_cache_matches_independent_variable_byte_lru() {
    let mut cache = LookupCache::new(MINIMUM).unwrap();
    let mut reference: Vec<(u8, usize)> = Vec::new();
    let mut hits = 0;
    let mut misses = 0;
    let mut evictions = 0;
    for step in 0..10_000_u32 {
        let key = ((step * 37 + step / 11) % 19) as u8;
        let length = usize::from(key) * 3 + 1;
        let expected = reference
            .iter()
            .position(|(candidate, _)| *candidate == key);
        let found = cache
            .get(Identity([3; IDENTITY_BYTES]), &[key], limits())
            .unwrap();
        if let Some(index) = expected {
            hits += 1;
            let entry = reference.remove(index);
            reference.push(entry);
            let (bytes, report) = found.unwrap();
            assert_eq!(bytes.as_slice(), vec![key; length]);
            assert_eq!(report, work());
        } else {
            misses += 1;
            assert!(found.is_none());
            let charge = ENTRY + IDENTITY_BYTES + 1 + length;
            while reference.iter().map(|(_, n)| n).sum::<usize>() + charge > MINIMUM - FIXED {
                reference.remove(0);
                evictions += 1;
            }
            reference.push((key, charge));
            cache
                .insert(
                    Identity([3; IDENTITY_BYTES]),
                    &[key],
                    &vec![key; length],
                    work(),
                )
                .unwrap();
        }
        assert_invariants(&cache);
        assert_eq!(cache.report().unwrap().hits, hits);
        assert_eq!(cache.report().unwrap().misses, misses);
        assert_eq!(cache.report().unwrap().evictions, evictions);
        let actual: Vec<_> = cache
            .order
            .values()
            .map(|key| key.0[IDENTITY_BYTES])
            .collect();
        assert_eq!(
            actual,
            reference.iter().map(|(key, _)| *key).collect::<Vec<_>>()
        );
    }
}

#[test]
fn positive_lookup_cache_limits_identity_bypass_and_clear_are_exact() {
    let identity = Identity([7; IDENTITY_BYTES]);
    let mut cache = LookupCache::new(MINIMUM).unwrap();
    cache
        .insert(identity, b"synthetic secret", &[8; 100], work())
        .unwrap();
    for changed in 0..IDENTITY_BYTES {
        let mut other = identity;
        other.0[changed] ^= 1;
        assert!(
            cache
                .get(other, b"synthetic secret", limits())
                .unwrap()
                .is_none()
        );
    }
    assert!(
        cache
            .get(identity, b"synthetic secret!", limits())
            .unwrap()
            .is_none()
    );
    for field in 0..4 {
        let mut narrow = limits();
        match field {
            0 => narrow.maximum_path_branches -= 1,
            1 => narrow.maximum_pages -= 1,
            2 => narrow.maximum_encoded_bytes -= 1,
            _ => narrow.maximum_value_bytes -= 1,
        }
        let clock = cache.clock;
        assert!(matches!(
            cache.get(identity, b"synthetic secret", narrow),
            Err(StorageError::ResourceLimit)
        ));
        assert_eq!(cache.clock, clock);
        assert_invariants(&cache);
    }
    let before = cache.report().unwrap();
    assert!(
        cache
            .insert(identity, b"synthetic secret", b"replacement", work())
            .is_err()
    );
    assert_eq!(cache.report().unwrap(), before);
    cache
        .insert(identity, b"oversized", &vec![0; MINIMUM], work())
        .unwrap();
    assert_eq!(cache.report().unwrap().oversized_bypasses, 1);
    assert_eq!(cache.report().unwrap().resident_values, 1);
    assert_eq!(
        cache
            .get(identity, b"synthetic secret", limits())
            .unwrap()
            .unwrap()
            .0
            .as_slice(),
        &[8; 100]
    );
    let hits = cache.hits;
    cache.clear();
    assert_eq!(cache.report().unwrap().accounted_bytes, 0);
    assert_eq!(cache.report().unwrap().hits, hits);
    assert!(
        cache
            .get(identity, b"synthetic secret", limits())
            .unwrap()
            .is_none()
    );
    assert_invariants(&cache);
}

#[test]
fn positive_lookup_cache_refuses_invalid_work_and_retains_overflow_diagnostics() {
    let mut cache = LookupCache::new(MINIMUM).unwrap();
    for field in 0..3 {
        let mut wrong = work();
        match field {
            0 => wrong.pages += 1,
            1 => wrong.encoded_bytes += 1,
            _ => wrong.path_branches += 1,
        }
        assert!(
            cache
                .insert(Identity([0; IDENTITY_BYTES]), b"key", b"value", wrong)
                .is_err()
        );
        assert_eq!(cache.report().unwrap().resident_values, 0);
    }
    cache
        .insert(Identity([0; IDENTITY_BYTES]), b"key", b"value", work())
        .unwrap();
    cache.clock = u128::MAX;
    assert!(matches!(
        cache.get(Identity([0; IDENTITY_BYTES]), b"key", limits()),
        Err(StorageError::ResourceLimit)
    ));
    assert_invariants(&cache);
    cache.clear();
    cache.misses = u64::MAX;
    assert!(
        cache
            .get(Identity([0; IDENTITY_BYTES]), b"missing", limits())
            .unwrap()
            .is_none()
    );
    assert_eq!(cache.report(), Err(StorageError::ResourceLimit));
    cache.clear();
    assert_eq!(cache.report(), Err(StorageError::ResourceLimit));
}
