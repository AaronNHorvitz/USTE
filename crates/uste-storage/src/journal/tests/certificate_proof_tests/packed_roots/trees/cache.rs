use super::*;
use crate::packed_page_cache::PackedPageCache;
mod positive;

fn cache() -> PackedPageCache {
    PackedPageCache::new(256 * 1024).unwrap()
}

fn lookup_cache() -> PackedPageCache {
    PackedPageCache::new_with_lookup_budget(256 * 1024, 64 * 1024).unwrap()
}

fn range_cache() -> PackedPageCache {
    PackedPageCache::new_with_lookup_and_range_budget(384 * 1024, 64 * 1024, 128 * 1024).unwrap()
}

#[test]
fn packed_cached_lookup_and_cursors_preserve_results_and_work_limits() {
    lookup_and_cursors(cache);
}

#[test]
fn packed_positive_cached_lookup_and_cursors_preserve_results_and_work_limits() {
    lookup_and_cursors(lookup_cache);
}

fn lookup_and_cursors(cache: fn() -> PackedPageCache) {
    let mut f = Fixture::new(false);
    let staged = initial(&mut f);
    let tree = staged.tree();
    let reference = f
        .store
        .packed_tree_get(&mut f.fs, tree, b"b", reads())
        .unwrap();
    let mut cache = cache();
    f.fs.arm(FaultPlan::default()).unwrap();
    let cold = f
        .store
        .packed_tree_get_cached(&mut f.fs, tree, b"b", reads(), &mut cache)
        .unwrap();
    assert_eq!(
        cold.value.unwrap().as_slice(),
        reference.value.as_ref().unwrap().as_slice()
    );
    assert_eq!(cold.report, reference.report);
    assert!(f.fs.operation_count(Operation::ReadAt) > 0);
    f.fs.arm(FaultPlan::default()).unwrap();
    let warm = f
        .store
        .packed_tree_get_cached(&mut f.fs, tree, b"b", reads(), &mut cache)
        .unwrap();
    assert_eq!(
        warm.value.unwrap().as_slice(),
        reference.value.as_ref().unwrap().as_slice()
    );
    assert_eq!(warm.report, reference.report);
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    let mut exact = reads();
    exact.maximum_pages = reference.report.pages;
    exact.maximum_encoded_bytes = reference.report.encoded_bytes;
    exact.maximum_value_bytes = 20_000;
    exact.maximum_path_branches = reference.report.path_branches;
    assert!(
        f.store
            .packed_tree_get_cached(&mut f.fs, tree, b"b", exact, &mut cache)
            .is_ok()
    );
    for variant in 0..4 {
        let mut narrow = exact;
        match variant {
            0 => narrow.maximum_pages -= 1,
            1 => narrow.maximum_encoded_bytes -= 1,
            2 => narrow.maximum_value_bytes -= 1,
            _ => narrow.maximum_path_branches -= 1,
        }
        assert!(
            f.store
                .packed_tree_get_cached(&mut f.fs, tree, b"b", narrow, &mut cache)
                .is_err()
        );
    }
    let expected = entries(&mut f, tree);
    for reverse in [false, true] {
        let mut cursor = if reverse {
            f.store
                .open_reverse_packed_tree_cursor(tree, b"", None, cursors())
                .unwrap()
        } else {
            f.store
                .open_packed_tree_cursor(tree, b"", None, cursors())
                .unwrap()
        };
        let mut reference_cursor = if reverse {
            f.store
                .open_reverse_packed_tree_cursor(tree, b"", None, cursors())
                .unwrap()
        } else {
            f.store
                .open_packed_tree_cursor(tree, b"", None, cursors())
                .unwrap()
        };
        let mut result = Vec::new();
        while let Some(entry) = f
            .store
            .next_packed_tree_entry_cached(&mut f.fs, &mut cursor, &mut cache)
            .unwrap()
        {
            result.push((entry.key().to_vec(), entry.value().to_vec()));
        }
        while f
            .store
            .next_packed_tree_entry(&mut f.fs, &mut reference_cursor)
            .unwrap()
            .is_some()
        {}
        assert_eq!(cursor.report(), reference_cursor.report());
        if reverse {
            result.reverse();
        }
        assert_eq!(result, expected);
    }
    assert!(cache.report().unwrap().hits > 0);
    assert!(cache.report().unwrap().accounted_bytes <= 256 * 1024);
    assert_eq!(f.fs.operation_count(Operation::WriteAt), 0);
}

#[test]
fn packed_cached_owner_unlock_session_and_exhausted_cursor_never_bypass_keys() {
    owner_and_session(cache);
}

#[test]
fn packed_positive_cached_owner_unlock_session_never_bypass_keys() {
    owner_and_session(lookup_cache);
}

fn owner_and_session(cache: fn() -> PackedPageCache) {
    let mut f = Fixture::new(false);
    let staged = initial(&mut f);
    let tree = staged.tree();
    let mut cache = cache();
    f.store
        .packed_tree_get_cached(&mut f.fs, tree, b"a", reads(), &mut cache)
        .unwrap();
    let mut other = Fixture::new(false);
    let other_stage = initial(&mut other);
    other.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        other
            .store
            .packed_tree_get_cached(&mut other.fs, other_stage.tree(), b"a", reads(), &mut cache)
            .is_err()
    );
    assert_eq!(other.fs.operation_count(Operation::ReadAt), 0);
    f.fs.arm(FaultPlan::default()).unwrap();
    f.store.vault.lock();
    assert!(matches!(
        f.store
            .packed_tree_get_cached(&mut f.fs, tree, b"a", reads(), &mut cache),
        Err(StorageError::Crypto(CryptoError::Locked))
    ));
    assert_eq!(cache.report().unwrap().resident_pages, 0);
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    f.store.vault.unlock(&mut TestKeyAdapter).unwrap();
    f.store
        .packed_tree_get_cached(&mut f.fs, tree, b"a", reads(), &mut cache)
        .unwrap();
    // Even without a read while locked, the next unlock is a distinct plaintext-cache session.
    f.store.vault.lock();
    f.store.vault.unlock(&mut TestKeyAdapter).unwrap();
    f.fs.arm(FaultPlan::default()).unwrap();
    f.store
        .packed_tree_get_cached(&mut f.fs, tree, b"a", reads(), &mut cache)
        .unwrap();
    assert!(f.fs.operation_count(Operation::ReadAt) > 0);
    let mut cursor = f
        .store
        .open_packed_tree_cursor(tree, b"", None, cursors())
        .unwrap();
    while f
        .store
        .next_packed_tree_entry_cached(&mut f.fs, &mut cursor, &mut cache)
        .unwrap()
        .is_some()
    {}
    f.store.vault.lock();
    assert!(
        f.store
            .next_packed_tree_entry_cached(&mut f.fs, &mut cursor, &mut cache)
            .is_err()
    );
    assert_eq!(
        f.store
            .next_packed_tree_entry_cached(&mut f.fs, &mut cursor, &mut cache)
            .err(),
        Some(StorageError::NeedsRecovery)
    );
    // A different unlocked vault under identical external IDs must not receive old plaintext.
    f.store.vault = create_vault(f.store.database, 999_000);
    assert!(
        f.store
            .packed_tree_get_cached(&mut f.fs, tree, b"a", reads(), &mut cache)
            .is_err()
    );
    assert_eq!(cache.report().unwrap().resident_pages, 0);
}

#[test]
fn packed_cached_late_corruption_is_detected_after_clear_not_hidden_as_a_miss() {
    late_corruption(cache);
}

#[test]
fn packed_positive_cached_late_corruption_is_detected_after_clear() {
    late_corruption(lookup_cache);
}

fn late_corruption(cache: fn() -> PackedPageCache) {
    let mut f = Fixture::new(false);
    let staged = initial(&mut f);
    let tree = staged.tree();
    let mut cache = cache();
    f.store
        .packed_tree_get_cached(&mut f.fs, tree, b"b", reads(), &mut cache)
        .unwrap();
    let context = tree.context();
    let physical = tree
        .family_descriptor()
        .root
        .unwrap()
        .resolve(
            context.scope,
            context.profile,
            context.family,
            context.revision,
        )
        .unwrap();
    let name = EntryName::new(format!(
        "pack-{}",
        physical
            .object
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ))
    .unwrap();
    let file =
        f.fs.open_existing(&f.store.database_directory, &name)
            .unwrap();
    let offset = physical.page * ENCODED_PAGE_BYTES as u64 + 137;
    let mut byte = [0];
    f.fs.read_at(&file, offset, &mut byte).unwrap();
    byte[0] ^= 1;
    f.fs.write_at(&file, offset, &byte).unwrap();
    // Cached immutable plaintext is not a new observation of disk freshness.
    assert!(
        f.store
            .packed_tree_get_cached(&mut f.fs, tree, b"b", reads(), &mut cache)
            .is_ok()
    );
    assert!(
        f.store
            .packed_tree_get(&mut f.fs, tree, b"b", reads())
            .is_err()
    );
    cache.clear();
    assert!(
        f.store
            .packed_tree_get_cached(&mut f.fs, tree, b"b", reads(), &mut cache)
            .is_err()
    );
    assert_eq!(cache.report().unwrap().resident_pages, 0);
    byte[0] ^= 1;
    f.fs.write_at(&file, offset, &byte).unwrap();
    assert!(
        f.store
            .packed_tree_get_cached(&mut f.fs, tree, b"b", reads(), &mut cache)
            .is_ok()
    );
}

#[test]
fn packed_cached_cold_faults_return_no_value_and_reopen_with_empty_cache() {
    cold_faults(cache);
}

#[test]
fn packed_positive_cached_cold_faults_never_retain_values_and_reopen() {
    cold_faults(lookup_cache);
}

fn cold_faults(cache: fn() -> PackedPageCache) {
    let operations = [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
    ];
    let mut f = Fixture::new(false);
    let staged = initial(&mut f);
    let mut baseline = cache();
    f.fs.arm(FaultPlan::default()).unwrap();
    f.store
        .packed_tree_get_cached(&mut f.fs, staged.tree(), b"b", reads(), &mut baseline)
        .unwrap();
    let counts = operations.map(|operation| f.fs.operation_count(operation));
    let mut cases = 0;
    for (operation, count) in operations.into_iter().zip(counts) {
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let mut f = Fixture::new(false);
                let staged = initial(&mut f);
                publish_tree(&mut f, staged.tree());
                let mut cache = cache();
                f.fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                assert!(
                    f.store
                        .packed_tree_get_cached(&mut f.fs, staged.tree(), b"b", reads(), &mut cache)
                        .is_err()
                );
                assert_eq!(f.fs.pending_faults(), 0);
                if let Some(report) = cache.report().unwrap().lookup {
                    assert_eq!(report.resident_values, 0);
                }
                assert_eq!(f.fs.operation_count(Operation::WriteAt), 0);
                let mut f = reopen(f);
                let proof = f.proof(2).unwrap();
                let roots = discover(&mut f, &proof, limits(2)).unwrap().0;
                let (tree, _) = f
                    .store
                    .admit_packed_tree(&mut f.fs, &roots[0], 1, validation())
                    .unwrap();
                // Old owner-bound cached pages cannot silently survive reopening.
                assert!(
                    f.store
                        .packed_tree_get_cached(&mut f.fs, &tree, b"b", reads(), &mut cache)
                        .is_err()
                );
                cache.clear();
                assert_eq!(
                    f.store
                        .packed_tree_get_cached(&mut f.fs, &tree, b"b", reads(), &mut cache)
                        .unwrap()
                        .value
                        .unwrap()
                        .as_slice(),
                    vec![9; 20_000]
                );
                cases += 1;
            }
        }
    }
    assert!(cases > 0);
    eprintln!("packed cached cold lookup fault cases: {cases}");
}

#[test]
fn packed_cached_empty_trees_and_zero_work_still_require_live_owner_and_keys() {
    let mut f = Fixture::new(false);
    let proof = f.proof(2).unwrap();
    let empty = stage(&mut f, &proof, None, &[]).unwrap();
    let mut cache = cache();
    let zero = TreeLookupLimits {
        maximum_path_branches: 0,
        maximum_pages: 0,
        maximum_encoded_bytes: 0,
        maximum_value_bytes: 0,
    };
    f.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        f.store
            .packed_tree_get_cached(&mut f.fs, empty.tree(), b"missing", zero, &mut cache)
            .unwrap()
            .value
            .is_none()
    );
    let mut cursor = f
        .store
        .open_packed_tree_cursor(
            empty.tree(),
            b"",
            None,
            TreeCursorLimits {
                maximum_path_branches: 0,
                maximum_candidates: 0,
                maximum_returned_bytes: 0,
                maximum_pages: 0,
                maximum_encoded_bytes: 0,
            },
        )
        .unwrap();
    assert!(
        f.store
            .next_packed_tree_entry_cached(&mut f.fs, &mut cursor, &mut cache)
            .unwrap()
            .is_none()
    );
    f.store.vault.lock();
    assert!(
        f.store
            .packed_tree_get_cached(&mut f.fs, empty.tree(), b"missing", zero, &mut cache)
            .is_err()
    );
    assert!(
        f.store
            .next_packed_tree_entry_cached(&mut f.fs, &mut cursor, &mut cache)
            .is_err()
    );
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(f.fs.operation_count(Operation::WriteAt), 0);
    assert_eq!(cache.report().unwrap().resident_pages, 0);
}

#[test]
fn packed_cached_cursor_faults_are_sticky_and_warm_work_is_identical() {
    let operations = [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
    ];
    let mut cases = 0;
    for reverse in [false, true] {
        let mut f = Fixture::new(false);
        let staged = initial(&mut f);
        let mut cache = cache();
        let mut cursor = if reverse {
            f.store
                .open_reverse_packed_tree_cursor(staged.tree(), b"", None, cursors())
                .unwrap()
        } else {
            f.store
                .open_packed_tree_cursor(staged.tree(), b"", None, cursors())
                .unwrap()
        };
        f.fs.arm(FaultPlan::default()).unwrap();
        while f
            .store
            .next_packed_tree_entry_cached(&mut f.fs, &mut cursor, &mut cache)
            .unwrap()
            .is_some()
        {}
        let counts = operations.map(|operation| f.fs.operation_count(operation));
        let expected = cursor.report();
        let mut warm = if reverse {
            f.store
                .open_reverse_packed_tree_cursor(staged.tree(), b"", None, cursors())
                .unwrap()
        } else {
            f.store
                .open_packed_tree_cursor(staged.tree(), b"", None, cursors())
                .unwrap()
        };
        f.fs.arm(FaultPlan::default()).unwrap();
        while f
            .store
            .next_packed_tree_entry_cached(&mut f.fs, &mut warm, &mut cache)
            .unwrap()
            .is_some()
        {}
        assert_eq!(warm.report(), expected);
        assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
        for (operation, count) in operations.into_iter().zip(counts) {
            for occurrence in 1..=count {
                for action in [
                    FaultAction::Error(AdapterErrorKind::Io),
                    FaultAction::CrashBefore,
                    FaultAction::CrashAfter,
                ] {
                    let mut f = Fixture::new(false);
                    let staged = initial(&mut f);
                    let mut cache = PackedPageCache::new(256 * 1024).unwrap();
                    let mut cursor = if reverse {
                        f.store
                            .open_reverse_packed_tree_cursor(staged.tree(), b"", None, cursors())
                            .unwrap()
                    } else {
                        f.store
                            .open_packed_tree_cursor(staged.tree(), b"", None, cursors())
                            .unwrap()
                    };
                    f.fs.arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                    loop {
                        match f.store.next_packed_tree_entry_cached(
                            &mut f.fs,
                            &mut cursor,
                            &mut cache,
                        ) {
                            Ok(Some(_)) => {}
                            Ok(None) => panic!("missing injected fault"),
                            Err(_) => break,
                        }
                    }
                    assert_eq!(f.fs.pending_faults(), 0);
                    assert_eq!(
                        f.store
                            .next_packed_tree_entry_cached(&mut f.fs, &mut cursor, &mut cache)
                            .err(),
                        Some(StorageError::NeedsRecovery)
                    );
                    assert_eq!(f.fs.operation_count(Operation::WriteAt), 0);
                    cases += 1;
                }
            }
        }
    }
    assert!(cases > 0);
    eprintln!("packed cached directional cursor fault cases: {cases}");
}

#[test]
fn packed_cached_lru_matches_independent_trace_and_every_context_component() {
    let mut f = Fixture::new(false);
    let proof = f.proof(2).unwrap();
    let mut contexts = Vec::new();
    for _ in 0..4 {
        let staged = initial(&mut f);
        let pack = staged.pack().unwrap();
        for page in 0..pack.pages() {
            contexts.push(PackedPageContext {
                page,
                ..pack.context()
            });
        }
    }
    assert!(contexts.len() > 3);
    let mut cache = PackedPageCache::new(8 * 1024 + 3 * (16 * 1024 + 1024)).unwrap();
    cache
        .bind(&proof, f.store.vault.unlocked_session().unwrap())
        .unwrap();
    let mut lru = Vec::new();
    let (mut hits, mut misses, mut evictions) = (0, 0, 0);
    f.fs.arm(FaultPlan::default()).unwrap();
    let mut random = 7_u64;
    for _ in 0..20_000 {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        let key = contexts[(random >> 32) as usize % contexts.len()];
        if let Some(index) = lru.iter().position(|old| *old == key) {
            lru.remove(index);
            hits += 1;
        } else {
            misses += 1;
            if lru.len() == 3 {
                lru.remove(0);
                evictions += 1;
            }
        }
        lru.push(key);
        assert!(
            cache
                .page(
                    &mut f.fs,
                    &f.store.database_directory,
                    &f.store.vault,
                    key,
                    0
                )
                .unwrap()
                .record(0)
                .is_some()
        );
        let report = cache.report().unwrap();
        assert_eq!(
            (report.hits, report.misses, report.evictions),
            (hits, misses, evictions)
        );
        assert_eq!(report.resident_pages, lru.len());
        assert!(report.accounted_bytes <= report.budget_bytes);
        cache.assert_metadata_allowance();
    }
    assert_eq!(f.fs.operation_count(Operation::ReadAt), misses);
    let original = contexts[0];
    cache
        .page(
            &mut f.fs,
            &f.store.database_directory,
            &f.store.vault,
            original,
            0,
        )
        .unwrap();
    for variant in 0..9 {
        let mut altered = original;
        match variant {
            0 => {
                altered.scope =
                    NamespaceRef::new(DatabaseId::from_bytes([111; 16]), altered.scope.namespace())
            }
            1 => {
                altered.scope = NamespaceRef::new(
                    altered.scope.database(),
                    uste_types::NamespaceId::from_bytes([111; 16]),
                )
            }
            2 => altered.epoch = KeyEpoch::new(2).unwrap(),
            3 => altered.writer = WriterIncarnationId::from_bytes([111; 16]),
            4 => altered.creation_revision = CommitRevision::new(2).unwrap(),
            5 => altered.profile = [111; 32],
            6 => altered.family = 111,
            7 => altered.object = [111; 16],
            _ => altered.page = 111,
        }
        let before = cache.report().unwrap().hits;
        assert!(
            cache
                .page(
                    &mut f.fs,
                    &f.store.database_directory,
                    &f.store.vault,
                    altered,
                    0
                )
                .is_err()
        );
        assert_eq!(cache.report().unwrap().hits, before);
    }
    cache.clear();
    assert_eq!(cache.report().unwrap().resident_pages, 0);
    assert_eq!(cache.report().unwrap().accounted_bytes, 0);
}
