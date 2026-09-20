use super::*;
use crate::{MAX_INDEX_CACHE_BYTES, MIN_INDEX_CACHE_BYTES, packed_page_cache::PackedCacheReport};

fn buffered(
    f: &mut Fixture,
    target: &CertificateAnchorProof,
    base: Option<&CanonicalPackedTree>,
    deltas: &[IndexDelta],
    limits: TreeBatchLimits,
    budget: usize,
) -> Result<(CertifiedPackedTreeStage, PackedCacheReport), StorageError> {
    let selected = scope(f);
    f.store.stage_packed_tree_batch_proven_buffered(
        &mut f.fs, selected, PROFILE, 1, target, base, deltas, limits, budget,
    )
}

#[test]
fn buffered_staging_preserves_reference_roots_proof_charges_and_fresh_cache_bounds() {
    let mut f = Fixture::new(false);
    let base = initial(&mut f);
    let target = f.proof(2).unwrap();
    let deltas = [
        delta(b"a", Some(b"A"), Some(b"changed")),
        delta(b"ab", Some(b"AB"), None),
        delta(b"ba", None, Some(b"new")),
    ];
    let expected = stage(&mut f, &target, Some(base.tree()), &deltas).unwrap();
    let expected_entries = vec![
        (b"a".to_vec(), b"changed".to_vec()),
        (b"b".to_vec(), vec![9; 20_000]),
        (b"ba".to_vec(), b"new".to_vec()),
    ];
    assert_eq!(entries(&mut f, expected.tree()), expected_entries);
    let exact = TreeBatchLimits {
        maximum_read_pages: expected.report().read_pages,
        maximum_read_bytes: expected.report().read_bytes,
        ..batches()
    };
    for budget in [MIN_INDEX_CACHE_BYTES, 1024 * 1024] {
        let before = f.store.vault.decrypt_report().unwrap();
        let (actual, cache) =
            buffered(&mut f, &target, Some(base.tree()), &deltas, exact, budget).unwrap();
        let after = f.store.vault.decrypt_report().unwrap();
        assert_eq!(actual.report(), expected.report());
        assert_eq!(
            actual.tree().family_descriptor().commitment,
            expected.tree().family_descriptor().commitment
        );
        assert_eq!(cache.hits + cache.misses, actual.report().read_pages);
        assert!(cache.hits > 0);
        assert!(cache.accounted_bytes <= budget);
        assert_eq!(
            after.successful_calls - before.successful_calls,
            cache.misses
        );
        assert_eq!(entries(&mut f, actual.tree()), expected_entries);
        let (_, repeated) =
            buffered(&mut f, &target, Some(base.tree()), &deltas, exact, budget).unwrap();
        assert_eq!(cache, repeated);
        for narrow in [
            TreeBatchLimits {
                maximum_read_pages: exact.maximum_read_pages - 1,
                ..exact
            },
            TreeBatchLimits {
                maximum_read_bytes: exact.maximum_read_bytes - 1,
                ..exact
            },
        ] {
            f.fs.arm(FaultPlan::default()).unwrap();
            assert_eq!(
                buffered(&mut f, &target, Some(base.tree()), &deltas, narrow, budget).err(),
                Some(StorageError::ResourceLimit)
            );
            assert_eq!(f.fs.operation_count(Operation::CreateNew), 0);
        }
    }
    assert_eq!(
        entries(&mut f, base.tree()),
        vec![
            (b"a".to_vec(), b"A".to_vec()),
            (b"ab".to_vec(), b"AB".to_vec()),
            (b"b".to_vec(), vec![9; 20_000])
        ]
    );
}

#[test]
fn buffered_staging_generated_multibatch_history_matches_sorted_reference() {
    let mut f = Fixture::new(false);
    let target = f.proof(2).unwrap();
    let mut base: Option<CertifiedPackedTreeStage> = None;
    let mut reference = BTreeMap::<Vec<u8>, Vec<u8>>::new();
    for round in 0..16_u8 {
        let mut changes = Vec::new();
        for ordinal in 0..16_u8 {
            if !(round + ordinal).is_multiple_of(3) {
                continue;
            }
            let key = vec![b'a' + ordinal];
            let before = reference.get(&key).cloned();
            let after = if before.is_some() && round.is_multiple_of(4) {
                None
            } else {
                Some(vec![round; 1900 + usize::from(ordinal) * 7])
            };
            changes.push(IndexDelta::new(key.clone(), before, after.clone()).unwrap());
            if let Some(value) = after {
                reference.insert(key, value);
            } else {
                reference.remove(&key);
            }
        }
        let previous = base.as_ref().map(CertifiedPackedTreeStage::tree);
        let expected = stage(&mut f, &target, previous, &changes).unwrap();
        let budget = if round.is_multiple_of(2) {
            MIN_INDEX_CACHE_BYTES
        } else {
            1024 * 1024
        };
        let (actual, cache) =
            buffered(&mut f, &target, previous, &changes, batches(), budget).unwrap();
        assert_eq!(actual.report(), expected.report());
        assert_eq!(
            actual.tree().family_descriptor().commitment,
            expected.tree().family_descriptor().commitment
        );
        assert!(cache.accounted_bytes <= budget);
        assert_eq!(
            entries(&mut f, actual.tree()),
            reference.clone().into_iter().collect::<Entries>()
        );
        base = Some(actual);
    }
}

#[test]
fn buffered_staging_refuses_invalid_budgets_owners_and_lock_before_io() {
    let mut f = Fixture::new(false);
    let base = initial(&mut f);
    let target = f.proof(2).unwrap();
    let old_target = f.proof(0).unwrap();
    let mut foreign = Fixture::new(false);
    let foreign_base = initial(&mut foreign);
    let foreign_target = foreign.proof(2).unwrap();
    f.fs.arm(FaultPlan::default()).unwrap();
    for budget in [0, MIN_INDEX_CACHE_BYTES - 1, MAX_INDEX_CACHE_BYTES + 1] {
        assert_eq!(
            buffered(&mut f, &target, None, &[], batches(), budget).err(),
            Some(StorageError::ResourceLimit)
        );
    }
    assert!(
        buffered(
            &mut f,
            &foreign_target,
            None,
            &[],
            batches(),
            MIN_INDEX_CACHE_BYTES
        )
        .is_err()
    );
    assert!(
        buffered(
            &mut f,
            &old_target,
            Some(base.tree()),
            &[],
            batches(),
            MIN_INDEX_CACHE_BYTES
        )
        .is_err()
    );
    assert!(
        buffered(
            &mut f,
            &target,
            Some(foreign_base.tree()),
            &[],
            batches(),
            MIN_INDEX_CACHE_BYTES
        )
        .is_err()
    );
    let selected = scope(&f);
    for (selected, profile, family) in [
        (selected, [0; 32], 1),
        (selected, PROFILE, 0),
        (selected, PROFILE, 2),
        (
            NamespaceRef::new(
                selected.database(),
                uste_types::NamespaceId::from_bytes([8; 16]),
            ),
            PROFILE,
            1,
        ),
    ] {
        assert!(
            f.store
                .stage_packed_tree_batch_proven_buffered(
                    &mut f.fs,
                    selected,
                    profile,
                    family,
                    &target,
                    Some(base.tree()),
                    &[],
                    batches(),
                    MIN_INDEX_CACHE_BYTES
                )
                .is_err()
        );
    }
    f.store.vault.lock();
    assert_eq!(
        buffered(&mut f, &target, None, &[], batches(), MIN_INDEX_CACHE_BYTES).err(),
        Some(StorageError::Crypto(CryptoError::Locked))
    );
    f.store.vault.unlock(&mut TestKeyAdapter).unwrap();
    f.store.poisoned = true;
    assert!(buffered(&mut f, &target, None, &[], batches(), MIN_INDEX_CACHE_BYTES).is_err());
    f.store.poisoned = false;
    let (empty, cache) =
        buffered(&mut f, &target, None, &[], batches(), MIN_INDEX_CACHE_BYTES).unwrap();
    assert!(empty.tree().family_descriptor().root.is_none());
    assert_eq!(cache.misses + cache.hits, 0);
    assert_eq!(f.fs.operation_count(Operation::OpenExisting), 0);
    assert_eq!(f.fs.operation_count(Operation::CreateNew), 0);
}

#[test]
fn buffered_staging_rechecks_late_conflicts_and_later_ciphertext_corruption() {
    let mut f = Fixture::new(false);
    let base = initial(&mut f);
    let target = f.proof(2).unwrap();
    let deltas = [delta(b"a", Some(b"A"), Some(b"changed"))];
    buffered(
        &mut f,
        &target,
        Some(base.tree()),
        &deltas,
        batches(),
        1024 * 1024,
    )
    .unwrap();
    f.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        buffered(
            &mut f,
            &target,
            Some(base.tree()),
            &[
                delta(b"a", Some(b"A"), Some(b"changed")),
                delta(b"ab", Some(b"wrong"), None)
            ],
            batches(),
            1024 * 1024
        )
        .is_err()
    );
    assert_eq!(f.fs.operation_count(Operation::CreateNew), 0);
    let c = base.tree().context();
    let physical = base
        .tree()
        .family_descriptor()
        .root
        .unwrap()
        .resolve(c.scope, c.profile, c.family, c.revision)
        .unwrap();
    let file =
        f.fs.open_existing(
            &f.store.database_directory,
            &entry(&format!("pack-{}", hex_id(physical.object))),
        )
        .unwrap();
    let offset = physical.page * ENCODED_PAGE_BYTES as u64 + 100;
    let mut byte = [0];
    read_exact_at(&mut f.fs, &file, offset, &mut byte).unwrap();
    byte[0] ^= 1;
    write_all_at(&mut f.fs, &file, offset, &byte).unwrap();
    f.fs.sync_all(&file).unwrap();
    f.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        buffered(
            &mut f,
            &target,
            Some(base.tree()),
            &deltas,
            batches(),
            1024 * 1024
        )
        .is_err()
    );
    assert_eq!(f.fs.operation_count(Operation::CreateNew), 0);
}

#[test]
fn buffered_staging_all_observed_faults_preserve_the_published_base_and_journal() {
    let mut observed = Fixture::new(false);
    let base = initial(&mut observed);
    publish_tree(&mut observed, base.tree());
    let target = observed.proof(2).unwrap();
    let deltas = [delta(b"a", Some(b"A"), Some(b"changed"))];
    observed.fs.arm(FaultPlan::default()).unwrap();
    buffered(
        &mut observed,
        &target,
        Some(base.tree()),
        &deltas,
        batches(),
        1024 * 1024,
    )
    .unwrap();
    let mut cases = 0;
    for operation in [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SetLen,
        Operation::SyncAll,
        Operation::SyncDirectory,
    ] {
        for occurrence in 1..=observed.fs.operation_count(operation) {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let mut f = Fixture::new(false);
                let base = initial(&mut f);
                publish_tree(&mut f, base.tree());
                let target = f.proof(2).unwrap();
                let frontier = f.store.checkpoint_anchor();
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
                    buffered(
                        &mut f,
                        &target,
                        Some(base.tree()),
                        &deltas,
                        batches(),
                        1024 * 1024
                    )
                    .is_err()
                );
                assert_eq!(f.fs.pending_faults(), 0);
                assert_eq!(f.store.checkpoint_anchor(), frontier);
                let mut f = reopen(f);
                let proof = f.proof(2).unwrap();
                let (roots, _) = discover(&mut f, &proof, limits(2)).unwrap();
                assert_eq!(roots.len(), 1);
                let (base, _) = f
                    .store
                    .admit_packed_tree(&mut f.fs, &roots[0], 1, validation())
                    .unwrap();
                assert_eq!(entries(&mut f, &base)[0], (b"a".to_vec(), b"A".to_vec()));
                let (retried, _) =
                    buffered(&mut f, &proof, Some(&base), &deltas, batches(), 1024 * 1024).unwrap();
                assert_eq!(
                    entries(&mut f, retried.tree())[0],
                    (b"a".to_vec(), b"changed".to_vec())
                );
                cases += 1;
            }
        }
    }
    assert!(cases > 0 && cases < 42);
}
