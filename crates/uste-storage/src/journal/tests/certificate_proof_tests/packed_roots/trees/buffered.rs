use super::*;
use crate::{MAX_INDEX_CACHE_BYTES, MIN_INDEX_CACHE_BYTES};

#[test]
fn buffered_admission_rejects_authenticated_noncanonical_branches() {
    use crate::{
        packed_index_pack::ImmutablePackWriter,
        packed_index_page::PackedRecordKind,
        packed_tree_record::{ChildReference, PackedLocator, TreeNode},
    };
    for (bit, reverse, valid) in [(7, false, true), (8, false, false), (7, true, false)] {
        let mut f = Fixture::new(false);
        let input = claims(&f);
        let namespace = scope(&f);
        let staged = initial(&mut f);
        let base = staged
            .tree()
            .family_descriptor()
            .root
            .unwrap()
            .resolve(namespace, PROFILE, 1, input.revision)
            .unwrap();
        let mut writer = ImmutablePackWriter::create(
            &mut f.fs,
            &f.store.database_directory,
            PackedPageContext {
                object: [0; 16],
                page: 0,
                ..base
            },
            PackWriteLimits {
                maximum_pages: 1,
                maximum_records: 3,
                maximum_payload_bytes: 1024,
            },
            &mut CounterEntropy::new(550_000),
        )
        .unwrap();
        let mut children = Vec::new();
        for key in [0_u8, 3] {
            let key = [key];
            let node = TreeNode::Leaf {
                key: &key,
                value: ordered_commitment::value_commitment(&[]).unwrap(),
                first_chunk: None,
            };
            let claimed = node.commitment(writer.context()).unwrap();
            let bytes = node.encode(writer.context()).unwrap();
            let location = PackedLocator::from_address(
                writer
                    .append(
                        &mut f.fs,
                        &mut f.store.vault,
                        PackedRecordKind::TreeNode,
                        &bytes,
                    )
                    .unwrap(),
            );
            children.push(ChildReference { location, claimed });
        }
        if reverse {
            children.reverse();
        }
        let node = TreeNode::Branch {
            bit,
            left: children[0],
            right: children[1],
        };
        let commitment = node.commitment(writer.context()).unwrap();
        let bytes = node.encode(writer.context()).unwrap();
        let location = PackedLocator::from_address(
            writer
                .append(
                    &mut f.fs,
                    &mut f.store.vault,
                    PackedRecordKind::TreeNode,
                    &bytes,
                )
                .unwrap(),
        );
        writer.finish(&mut f.fs, &mut f.store.vault).unwrap();
        let root = f
            .store
            .publish_packed_root(
                &mut f.fs,
                namespace,
                PROFILE,
                input,
                &[PackedRootFamily {
                    family: 1,
                    commitment,
                    root: Some(location),
                }],
                2,
            )
            .unwrap();
        let result = f.store.admit_packed_tree_buffered(
            &mut f.fs,
            &root,
            1,
            validation(),
            MIN_INDEX_CACHE_BYTES,
        );
        if valid {
            let (_, report, cache) = result.unwrap();
            assert_eq!(report.entries, 2);
            assert_eq!(cache.misses, 1);
            assert_eq!(cache.hits, 2);
        } else {
            assert_eq!(result.err(), Some(StorageError::IntegrityFailure));
        }
    }
}

#[test]
fn buffered_admission_matches_uncached_and_preserves_every_proof_limit() {
    let mut f = Fixture::new(false);
    let staged = initial(&mut f);
    let root = publish_tree(&mut f, staged.tree());
    let (_, expected) = f
        .store
        .admit_packed_tree(&mut f.fs, &root, 1, validation())
        .unwrap();
    let exact = TreeValidationLimits {
        maximum_path_branches: expected.maximum_depth,
        maximum_nodes: expected.nodes,
        maximum_logical_bytes: expected.logical_bytes,
        maximum_pages: expected.pages,
        maximum_encoded_bytes: expected.encoded_bytes,
    };
    for budget in [MIN_INDEX_CACHE_BYTES, 1024 * 1024] {
        f.fs.arm(FaultPlan::default()).unwrap();
        let before = f.store.vault.decrypt_report().unwrap();
        let (tree, report, cache) = f
            .store
            .admit_packed_tree_buffered(&mut f.fs, &root, 1, exact, budget)
            .unwrap();
        let after = f.store.vault.decrypt_report().unwrap();
        assert_eq!(report, expected);
        assert!(cache.hits > 0);
        assert!(cache.accounted_bytes <= budget);
        assert_eq!(cache.hits + cache.misses, expected.pages);
        assert_eq!(f.fs.operation_count(Operation::ReadAt), cache.misses);
        assert_eq!(
            after.successful_calls - before.successful_calls,
            cache.misses
        );
        assert_eq!(
            after.authenticated_encoded_bytes - before.authenticated_encoded_bytes,
            cache.misses * ENCODED_PAGE_BYTES as u64
        );
        assert_eq!(entries(&mut f, &tree).len(), 3);
        for limits in [
            TreeValidationLimits {
                maximum_path_branches: exact.maximum_path_branches - 1,
                ..exact
            },
            TreeValidationLimits {
                maximum_nodes: exact.maximum_nodes - 1,
                ..exact
            },
            TreeValidationLimits {
                maximum_logical_bytes: exact.maximum_logical_bytes - 1,
                ..exact
            },
            TreeValidationLimits {
                maximum_pages: exact.maximum_pages - 1,
                ..exact
            },
            TreeValidationLimits {
                maximum_encoded_bytes: exact.maximum_encoded_bytes - 1,
                ..exact
            },
        ] {
            assert_eq!(
                f.store
                    .admit_packed_tree_buffered(&mut f.fs, &root, 1, limits, budget)
                    .err(),
                Some(StorageError::ResourceLimit)
            );
        }
    }
}

#[test]
fn buffered_admission_fresh_reads_reject_late_corruption() {
    let mut f = Fixture::new(false);
    let staged = initial(&mut f);
    let root = publish_tree(&mut f, staged.tree());
    let (tree, _, first) = f
        .store
        .admit_packed_tree_buffered(&mut f.fs, &root, 1, validation(), 1024 * 1024)
        .unwrap();
    let (_, _, second) = f
        .store
        .admit_packed_tree_buffered(&mut f.fs, &root, 1, validation(), 1024 * 1024)
        .unwrap();
    assert_eq!(first, second);
    assert!(second.misses > 0);
    let c = tree.context();
    let physical = tree
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
    assert!(
        f.store
            .admit_packed_tree_buffered(&mut f.fs, &root, 1, validation(), 1024 * 1024)
            .is_err()
    );
}

#[test]
fn buffered_admission_binding_budget_empty_and_lock_fail_closed() {
    let mut f = Fixture::new(false);
    let root = publish(&mut f, 2).unwrap();
    let (_, report, cache) = f
        .store
        .admit_packed_tree_buffered(&mut f.fs, &root, 1, validation(), MIN_INDEX_CACHE_BYTES)
        .unwrap();
    assert_eq!(report.entries, 0);
    assert_eq!(cache.accounted_bytes, 0);
    let foreign = Fixture::new(false);
    f.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        foreign
            .store
            .admit_packed_tree_buffered(&mut f.fs, &root, 1, validation(), MIN_INDEX_CACHE_BYTES)
            .is_err()
    );
    assert!(
        f.store
            .admit_packed_tree_buffered(&mut f.fs, &root, 2, validation(), MIN_INDEX_CACHE_BYTES)
            .is_err()
    );
    for budget in [0, MIN_INDEX_CACHE_BYTES - 1, MAX_INDEX_CACHE_BYTES + 1] {
        assert_eq!(
            f.store
                .admit_packed_tree_buffered(&mut f.fs, &root, 1, validation(), budget)
                .err(),
            Some(StorageError::ResourceLimit)
        );
    }
    f.store.vault.lock();
    assert_eq!(
        f.store
            .admit_packed_tree_buffered(&mut f.fs, &root, 1, validation(), MIN_INDEX_CACHE_BYTES)
            .err(),
        Some(StorageError::Crypto(CryptoError::Locked))
    );
    assert_eq!(f.fs.operation_count(Operation::OpenExisting), 0);
    f.store.vault.unlock(&mut TestKeyAdapter).unwrap();
    assert!(
        f.store
            .admit_packed_tree_buffered(&mut f.fs, &root, 1, validation(), MIN_INDEX_CACHE_BYTES)
            .is_ok()
    );
    let mut f = reopen(f);
    assert!(
        f.store
            .admit_packed_tree_buffered(&mut f.fs, &root, 1, validation(), MIN_INDEX_CACHE_BYTES)
            .is_err()
    );
}

#[test]
fn buffered_admission_every_read_fault_returns_no_capability_and_restarts() {
    let mut observed = Fixture::new(false);
    let staged = initial(&mut observed);
    let root = publish_tree(&mut observed, staged.tree());
    observed.fs.arm(FaultPlan::default()).unwrap();
    let (_, _, report) = observed
        .store
        .admit_packed_tree_buffered(&mut observed.fs, &root, 1, validation(), 1024 * 1024)
        .unwrap();
    let mut cases = 0;
    for operation in [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
    ] {
        for occurrence in 1..=observed.fs.operation_count(operation) {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let mut f = Fixture::new(false);
                let staged = initial(&mut f);
                let root = publish_tree(&mut f, staged.tree());
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
                        .admit_packed_tree_buffered(&mut f.fs, &root, 1, validation(), 1024 * 1024)
                        .is_err()
                );
                assert_eq!(f.fs.pending_faults(), 0);
                let mut f = reopen(f);
                let proof = f.proof(2).unwrap();
                let (roots, _) = discover(&mut f, &proof, limits(2)).unwrap();
                let (tree, _, _) = f
                    .store
                    .admit_packed_tree_buffered(&mut f.fs, &roots[0], 1, validation(), 1024 * 1024)
                    .unwrap();
                assert_eq!(entries(&mut f, &tree).len(), 3);
                cases += 1;
            }
        }
    }
    assert_eq!(cases, report.misses * 9);
    assert!(cases > 0 && cases < 81);
}
