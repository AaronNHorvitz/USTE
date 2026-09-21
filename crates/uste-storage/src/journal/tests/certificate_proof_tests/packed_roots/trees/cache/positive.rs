use super::*;
use crate::packed_page_cache::LookupIdentity;
use crate::packed_tree_record::PackedLocator;

#[test]
fn positive_cache_hits_avoid_decryption_and_absence_is_never_retained() {
    let mut f = Fixture::new(false);
    let staged = initial(&mut f);
    let tree = staged.tree();
    let mut cache = lookup_cache();
    let cold = f
        .store
        .packed_tree_get_cached(&mut f.fs, tree, b"b", reads(), &mut cache)
        .unwrap();
    let before = cache.report().unwrap();
    let crypto = f.store.vault.decrypt_report().unwrap();
    f.fs.arm(FaultPlan::default()).unwrap();
    let warm = f
        .store
        .packed_tree_get_cached(&mut f.fs, tree, b"b", reads(), &mut cache)
        .unwrap();
    assert_eq!(
        cold.value.unwrap().as_slice(),
        warm.value.unwrap().as_slice()
    );
    assert_eq!(cold.report, warm.report);
    assert_eq!(f.store.vault.decrypt_report().unwrap(), crypto);
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    let after = cache.report().unwrap();
    assert_eq!((after.hits, after.misses), (before.hits, before.misses));
    assert_eq!(after.lookup.unwrap().hits, before.lookup.unwrap().hits + 1);
    assert_eq!(after.lookup.unwrap().resident_values, 1);
    f.fs.arm(FaultPlan::default()).unwrap();
    let mapped = f
        .store
        .packed_tree_get_cached_with(&mut f.fs, tree, b"b", reads(), &mut cache, |bytes| {
            (bytes.len(), bytes.first().copied())
        })
        .unwrap();
    assert_eq!(mapped.value, Some((20_000, Some(9))));
    assert_eq!(mapped.report, cold.report);
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    let mapped_report = cache.report().unwrap().lookup.unwrap();
    assert_eq!(mapped_report.hits, after.lookup.unwrap().hits + 1);
    assert_eq!(mapped_report.resident_values, 1);
    for _ in 0..3 {
        assert!(
            f.store
                .packed_tree_get_cached(&mut f.fs, tree, b"missing", reads(), &mut cache)
                .unwrap()
                .value
                .is_none()
        );
    }
    let missing = cache.report().unwrap().lookup.unwrap();
    assert_eq!(missing.resident_values, 1);
    assert_eq!(missing.hits, mapped_report.hits);
    assert_eq!(missing.misses, mapped_report.misses + 3);
    assert!(cache.report().unwrap().accounted_bytes <= 256 * 1024);
    f.store.vault.lock();
    assert!(
        f.store
            .packed_tree_get_cached(&mut f.fs, tree, b"b", reads(), &mut cache)
            .is_err()
    );
    assert_eq!(cache.report().unwrap().accounted_bytes, 0);
    assert_eq!(cache.report().unwrap().lookup.unwrap().resident_values, 0);
}

#[test]
fn positive_cache_identity_encodes_every_context_locator_and_commitment_component() {
    let mut f = Fixture::new(false);
    let staged = initial(&mut f);
    let tree = staged.tree();
    let context = tree.context();
    let descriptor = tree.family_descriptor();
    let root = descriptor.root.unwrap();
    let commitment = descriptor.commitment;
    let mut cache = lookup_cache();
    f.store
        .packed_tree_get_cached(&mut f.fs, tree, b"b", reads(), &mut cache)
        .unwrap();
    // Exercise the real identity constructor against a value inserted only by authenticated I/O.
    for variant in 0..14 {
        let mut c = context;
        let mut r = root;
        let mut expected = commitment;
        match variant {
            0 => {
                c.scope = NamespaceRef::new(DatabaseId::from_bytes([111; 16]), c.scope.namespace())
            }
            1 => {
                c.scope = NamespaceRef::new(
                    c.scope.database(),
                    uste_types::NamespaceId::from_bytes([111; 16]),
                )
            }
            2 => c.profile[0] ^= 1,
            3 => c.family += 1,
            4 => c.revision = CommitRevision::new(c.revision.get() + 1).unwrap(),
            5..=10 => {
                let mut encoded = root.encode_fixed();
                // Object, creation revision, epoch, writer, page and slot are independent.
                let offset = [0, 23, 31, 32, 51, 53][variant - 5];
                encoded[offset] = encoded[offset].wrapping_add(1);
                let mut owner = root
                    .resolve(c.scope, c.profile, c.family, c.revision)
                    .unwrap();
                owner.creation_revision = CommitRevision::new(u64::MAX).unwrap();
                r = PackedLocator::decode_fixed(&encoded, owner).unwrap();
            }
            _ => {
                let mut digest = *commitment.digest();
                if variant == 13 {
                    digest[0] ^= 1;
                }
                expected = ordered_commitment::OrderedCommitment::claimed_nonempty(
                    commitment.entries() + u64::from(variant == 11),
                    commitment.logical_bytes() + u64::from(variant == 12),
                    digest,
                )
                .unwrap();
            }
        }
        assert!(
            cache
                .lookup_get(
                    &f.store.vault,
                    LookupIdentity::new(c, r, expected),
                    b"b",
                    reads()
                )
                .unwrap()
                .is_none()
        );
    }
    let hit = cache
        .lookup_get(
            &f.store.vault,
            LookupIdentity::new(context, root, commitment),
            b"b",
            reads(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(hit.0.as_slice(), vec![9; 20_000]);
    assert_eq!(cache.report().unwrap().lookup.unwrap().resident_values, 1);
}
