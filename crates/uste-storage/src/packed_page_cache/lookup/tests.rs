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
/// Resident slots from least to most recently used, following the intrusive list.
fn lru_order(cache: &LookupCache) -> Vec<u32> {
    let mut order = Vec::new();
    let mut cursor = cache.tail;
    while cursor != NONE {
        order.push(cursor);
        cursor = cache.slot(cursor).unwrap().newer;
    }
    order
}
fn lru_first_key_bytes(cache: &LookupCache) -> Vec<u8> {
    lru_order(cache)
        .into_iter()
        .map(|index| cache.slot(index).unwrap().key[0])
        .collect()
}
fn assert_invariants(cache: &LookupCache) {
    let live: Vec<u32> = cache
        .slots
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| slot.as_ref().map(|_| index as u32))
        .collect();
    assert_eq!(live.len(), cache.index.len());
    let mut oldest_first = lru_order(cache);
    let mut newest_first = Vec::new();
    let mut cursor = cache.head;
    while cursor != NONE {
        newest_first.push(cursor);
        cursor = cache.slot(cursor).unwrap().older;
    }
    newest_first.reverse();
    assert_eq!(oldest_first, newest_first);
    oldest_first.sort_unstable();
    assert_eq!(oldest_first, live);
    let mut free = cache.free_slots.clone();
    free.sort_unstable();
    let mut vacant: Vec<u32> = cache
        .slots
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| slot.is_none().then_some(index as u32))
        .collect();
    vacant.sort_unstable();
    assert_eq!(free, vacant);
    assert_eq!(
        cache.used,
        live.iter()
            .map(|index| cache.slot(*index).unwrap().charge)
            .sum()
    );
    let mut live_per_identity = vec![0_usize; cache.identities.len()];
    for index in &live {
        let slot = cache.slot(*index).unwrap();
        assert_eq!(
            slot.charge,
            ENTRY + IDENTITY_BYTES + slot.key.len() + slot.bytes.len()
        );
        live_per_identity[slot.identity as usize] += 1;
        let probe = Probe::new(slot.identity, &slot.key).unwrap();
        assert_eq!(cache.index.get(probe.as_slice()), Some(index));
    }
    let mut free_identities = cache.free_identities.clone();
    free_identities.sort_unstable();
    let mut vacant_identities = Vec::new();
    for (index, retained) in cache.identities.iter().enumerate() {
        match retained {
            Some(retained) => {
                assert_ne!(retained.live, 0);
                assert_eq!(retained.live, live_per_identity[index]);
            }
            None => {
                assert_eq!(live_per_identity[index], 0);
                vacant_identities.push(index as u32);
            }
        }
    }
    assert_eq!(free_identities, vacant_identities);
    assert!(cache.report().unwrap().accounted_bytes <= cache.budget);
    // Conservative logical allowance, not a claim about allocator/RSS behavior.
    assert!(
        size_of::<Option<Slot>>()
            + size_of::<(MapKey, u32)>()
            + 2 * size_of::<Zeroizing<Vec<u8>>>()
            + size_of::<Option<RetainedIdentity>>()
            + 4 * size_of::<usize>()
            <= ENTRY
    );
    assert!(size_of::<LookupCache>() <= FIXED);
}

#[test]
fn structured_cache_keys_preserve_every_identity_byte_and_variable_key_boundary() {
    let variable_keys = [
        vec![0],
        vec![0, 0],
        vec![0, 1],
        vec![1],
        vec![255],
        b"synthetic".to_vec(),
        vec![0; INLINE_PROBE_BYTES - 4],
        vec![0; INLINE_PROBE_BYTES - 3],
        vec![0; MAX_KEY_BYTES - 1],
        vec![0; MAX_KEY_BYTES],
    ];
    let mut cache = LookupCache::new(MAX_INDEX_CACHE_BYTES).unwrap();
    for variant in 0..=IDENTITY_BYTES {
        let mut identity = Identity([0; IDENTITY_BYTES]);
        if variant != 0 {
            identity.0[variant - 1] = 1;
        }
        cache
            .insert(identity, &[0], &[variant as u8], work())
            .unwrap();
    }
    for key in variable_keys.iter().skip(1) {
        cache
            .insert(Identity([0; IDENTITY_BYTES]), key, &[key[0]], work())
            .unwrap();
    }
    for variant in 0..=IDENTITY_BYTES {
        let mut identity = Identity([0; IDENTITY_BYTES]);
        if variant != 0 {
            identity.0[variant - 1] = 1;
        }
        assert_eq!(
            cache
                .get(identity, &[0], limits())
                .unwrap()
                .unwrap()
                .0
                .as_slice(),
            &[variant as u8]
        );
    }
    for key in variable_keys.iter().skip(1) {
        assert_eq!(
            cache
                .get(Identity([0; IDENTITY_BYTES]), key, limits())
                .unwrap()
                .unwrap()
                .0
                .as_slice(),
            &[key[0]]
        );
    }
    assert_eq!(
        cache.report().unwrap().resident_values,
        IDENTITY_BYTES + variable_keys.len()
    );
    assert_eq!(cache.identities.len(), IDENTITY_BYTES + 1);
    assert!(
        cache
            .get(Identity([0; IDENTITY_BYTES]), &[], limits())
            .is_err()
    );
    assert!(
        cache
            .get(
                Identity([0; IDENTITY_BYTES]),
                &vec![0; MAX_KEY_BYTES + 1],
                limits()
            )
            .is_err()
    );
    assert_invariants(&cache);
}

#[test]
fn shared_identity_is_interned_once_and_released_with_its_last_value() {
    let identity = Identity([7; IDENTITY_BYTES]);
    let other = Identity([9; IDENTITY_BYTES]);
    let mut cache = LookupCache::new(2 * MINIMUM).unwrap();
    for byte in (0..8).rev() {
        let key = [byte, 255 - byte];
        cache.insert(identity, &key, &key, work()).unwrap();
    }
    cache.insert(other, b"other", b"other", work()).unwrap();
    assert_eq!(cache.identities.len(), 2);
    assert_eq!(cache.identities[0].as_ref().unwrap().live, 8);
    assert_eq!(cache.identities[1].as_ref().unwrap().live, 1);
    assert_eq!(
        lru_first_key_bytes(&cache),
        vec![7, 6, 5, 4, 3, 2, 1, 0, b'o']
    );
    assert_invariants(&cache);
    // A filler sized to evict exactly the eight older values frees the first interned
    // identity, whose index the filler's own new identity then reuses.
    let filler = vec![0; 10_500];
    cache
        .insert(Identity([1; IDENTITY_BYTES]), b"filler", &filler, work())
        .unwrap();
    assert_invariants(&cache);
    assert_eq!(cache.report().unwrap().evictions, 8);
    assert_eq!(cache.report().unwrap().resident_values, 2);
    assert_eq!(cache.identities.len(), 2);
    assert!(cache.free_identities.is_empty());
    assert_eq!(
        *cache.identities[0].as_ref().unwrap().bytes,
        [1; IDENTITY_BYTES]
    );
    assert_eq!(
        *cache.identities[1].as_ref().unwrap().bytes,
        [9; IDENTITY_BYTES]
    );
    assert_eq!(lru_first_key_bytes(&cache), vec![b'o', b'f']);
    assert!(cache.get(identity, &[0, 255], limits()).unwrap().is_none());
    cache.clear();
    assert!(cache.identities.is_empty());
    assert!(cache.slots.is_empty());
    assert_invariants(&cache);
}

#[test]
fn scoped_mapper_reads_the_same_resident_allocation_without_an_output_copy() {
    let identity = Identity([11; IDENTITY_BYTES]);
    let mut cache = LookupCache::new(MINIMUM).unwrap();
    cache
        .insert(identity, b"mapped", b"resident plaintext", work())
        .unwrap();
    let mut first_map = Some(|bytes: &[u8]| (bytes.as_ptr() as usize, bytes.len(), bytes[0]));
    let (first, first_work) = cache
        .get_with(identity, b"mapped", limits(), &mut first_map)
        .unwrap()
        .unwrap();
    let mut second_map = Some(|bytes: &[u8]| (bytes.as_ptr() as usize, bytes.len(), bytes[0]));
    let (second, second_work) = cache
        .get_with(identity, b"mapped", limits(), &mut second_map)
        .unwrap()
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.1, b"resident plaintext".len());
    assert_eq!(first.2, b'r');
    assert_eq!((first_work, second_work), (work(), work()));
    assert_eq!(cache.report().unwrap().hits, 2);
    assert_invariants(&cache);
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
        assert_eq!(
            lru_first_key_bytes(&cache),
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
    cache.insert(identity, b"newer", &[9; 10], work()).unwrap();
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
        let order = lru_order(&cache);
        let hits = cache.hits;
        assert!(matches!(
            cache.get(identity, b"synthetic secret", narrow),
            Err(StorageError::ResourceLimit)
        ));
        // A refused hit is counted but never promotes the value.
        assert_eq!(cache.hits, hits + 1);
        assert_eq!(lru_order(&cache), order);
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
    assert_eq!(cache.report().unwrap().resident_values, 2);
    assert_eq!(
        cache
            .get(identity, b"synthetic secret", limits())
            .unwrap()
            .unwrap()
            .0
            .as_slice(),
        &[8; 100]
    );
    assert_eq!(lru_first_key_bytes(&cache), vec![b'n', b's']);
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
    cache.hits = u64::MAX;
    assert!(
        cache
            .get(Identity([0; IDENTITY_BYTES]), b"key", limits())
            .unwrap()
            .is_some()
    );
    assert_eq!(cache.report(), Err(StorageError::ResourceLimit));
    assert_eq!(cache.index.len(), lru_order(&cache).len());
    cache.clear();
    cache.overflowed = false;
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
