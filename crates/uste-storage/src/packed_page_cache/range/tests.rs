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
/// Resident slots from least to most recently used, following the intrusive list.
fn lru_order(cache: &RangeCache) -> Vec<u32> {
    let mut order = Vec::new();
    let mut cursor = cache.tail;
    while cursor != NONE {
        order.push(cursor);
        cursor = cache.slot(cursor).unwrap().newer;
    }
    order
}
fn assert_invariants(cache: &RangeCache) {
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
        live_per_identity[slot.identity as usize] += 1;
        assert_eq!(cache.index.get(slot.key.as_slice()), Some(index));
        assert_eq!(&slot.key[..4], slot.identity.to_be_bytes());
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
        let order = lru_order(&cache);
        let hits = cache.hits;
        assert!(matches!(
            cache.get(identity(7), b"a", Some(b"c"), false, narrow),
            Err(StorageError::ResourceLimit)
        ));
        assert_eq!(cache.hits, hits + 1);
        assert_eq!(lru_order(&cache), order);
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
fn complete_range_charge_matches_the_full_logical_key_and_identities_are_released() {
    let forward = vec![entry(b"ab", b"one"), entry(b"b", b"two")];
    let report = work(&forward);
    let mut cache = RangeCache::new(4 * MINIMUM).unwrap();
    cache
        .insert(identity(7), b"a", Some(b"c"), false, &forward, report)
        .unwrap();
    cache
        .insert(identity(7), b"a", None, false, &forward, report)
        .unwrap();
    // Reverse ranges are `(lower, upper]` in descending key order; same entries, same charge.
    let backward = vec![entry(b"b", b"two"), entry(b"ab", b"one")];
    cache
        .insert(identity(9), b"a", None, true, &backward, work(&backward))
        .unwrap();
    let payload: usize = forward
        .iter()
        .map(|entry| ITEM + entry.key().len() + entry.value().len())
        .sum();
    let expected = [
        // Identity, six framing bytes, one-byte lower and one-byte upper bound.
        RANGE + lookup::IDENTITY_BYTES + 6 + 1 + 1 + payload,
        // Unbounded upper ranges are still charged the six framing bytes, as the earlier
        // reserved key capacity was.
        RANGE + lookup::IDENTITY_BYTES + 6 + 1 + payload,
        RANGE + lookup::IDENTITY_BYTES + 6 + 1 + payload,
    ];
    let charges: Vec<usize> = lru_order(&cache)
        .into_iter()
        .map(|index| cache.slot(index).unwrap().charge)
        .collect();
    assert_eq!(charges, expected);
    assert_eq!(cache.identities.len(), 2);
    assert_eq!(cache.identities[0].as_ref().unwrap().live, 2);
    assert_eq!(cache.identities[1].as_ref().unwrap().live, 1);
    // Direction and bound identity: the reverse range under identity 9 is distinct.
    assert!(
        cache
            .get(identity(9), b"a", None, false, limits(report))
            .unwrap()
            .is_none()
    );
    assert!(
        cache
            .get(identity(9), b"a", None, true, limits(work(&backward)))
            .unwrap()
            .is_some()
    );
    assert_eq!(lru_order(&cache).len(), 3);
    assert_invariants(&cache);
    // Evict everything under identity 7 with a filler; the identity is released and reused.
    // Charge 512 + 182 + 128 + 1 + 27,648 = 28,471 of the 28,672 usable bytes, so all three
    // earlier ranges (959–960 bytes each) must be evicted.
    let filler = vec![entry(b"k", &vec![0; 4 * MINIMUM - FIXED - 1024])];
    cache
        .insert(identity(3), b"k", None, false, &filler, work(&filler))
        .unwrap();
    assert_invariants(&cache);
    assert_eq!(cache.report().unwrap().evictions, 3);
    assert_eq!(cache.report().unwrap().resident_ranges, 1);
    assert_eq!(cache.identities.iter().flatten().count(), 1);
    assert!(
        cache
            .get(identity(7), b"a", None, false, limits(report))
            .unwrap()
            .is_none()
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
            charge = Some(cache.slot(cache.head).unwrap().charge);
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
