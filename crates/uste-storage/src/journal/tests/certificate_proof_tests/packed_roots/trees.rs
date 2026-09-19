use super::*;
use crate::{packed_tree_cursor::TreeCursorLimits, packed_tree_validation::TreeValidationLimits};

fn batches() -> TreeBatchLimits {
    TreeBatchLimits {
        maximum_deltas: 16,
        maximum_input_bytes: 64_000,
        maximum_dirty_nodes: 128,
        maximum_path_branches: 128,
        maximum_read_pages: 128,
        maximum_read_bytes: 128 * ENCODED_PAGE_BYTES as u64,
        pack: PackWriteLimits {
            maximum_pages: 16,
            maximum_records: 128,
            maximum_payload_bytes: 64_000,
        },
    }
}
fn reads() -> TreeLookupLimits {
    TreeLookupLimits {
        maximum_path_branches: 128,
        maximum_pages: 128,
        maximum_encoded_bytes: 128 * ENCODED_PAGE_BYTES as u64,
        maximum_value_bytes: 32_000,
    }
}
fn validation() -> TreeValidationLimits {
    TreeValidationLimits {
        maximum_path_branches: 128,
        maximum_nodes: 128,
        maximum_logical_bytes: 64_000,
        maximum_pages: 128,
        maximum_encoded_bytes: 128 * ENCODED_PAGE_BYTES as u64,
    }
}
fn cursors() -> TreeCursorLimits {
    TreeCursorLimits {
        maximum_path_branches: 128,
        maximum_candidates: 128,
        maximum_returned_bytes: 64_000,
        maximum_pages: 128,
        maximum_encoded_bytes: 128 * ENCODED_PAGE_BYTES as u64,
    }
}
fn delta(key: &[u8], before: Option<&[u8]>, after: Option<&[u8]>) -> IndexDelta {
    IndexDelta::new(
        key.to_vec(),
        before.map(<[u8]>::to_vec),
        after.map(<[u8]>::to_vec),
    )
    .unwrap()
}
fn stage(
    f: &mut Fixture,
    proof: &CertificateAnchorProof,
    base: Option<&CanonicalPackedTree>,
    deltas: &[IndexDelta],
) -> Result<CertifiedPackedTreeStage, StorageError> {
    let scope = scope(f);
    f.store.stage_packed_tree_batch_proven(
        &mut f.fs,
        scope,
        PROFILE,
        1,
        proof,
        base,
        deltas,
        batches(),
    )
}
fn initial(f: &mut Fixture) -> CertifiedPackedTreeStage {
    let proof = f.proof(2).unwrap();
    stage(
        f,
        &proof,
        None,
        &[
            delta(b"a", None, Some(b"A")),
            delta(b"ab", None, Some(b"AB")),
            delta(b"b", None, Some(&vec![9; 20_000])),
        ],
    )
    .unwrap()
}
fn publish_tree(f: &mut Fixture, tree: &CanonicalPackedTree) -> CertifiedPackedRoot {
    let scope = scope(f);
    let input = claims(f);
    f.store
        .publish_packed_root(
            &mut f.fs,
            scope,
            PROFILE,
            input,
            &[tree.family_descriptor()],
            2,
        )
        .unwrap()
}
type Entries = Vec<(Vec<u8>, Vec<u8>)>;
fn entries(f: &mut Fixture, tree: &CanonicalPackedTree) -> Entries {
    let mut cursor = f
        .store
        .open_packed_tree_cursor(tree, b"", None, cursors())
        .unwrap();
    let mut values = Vec::new();
    assert_eq!(cursor.context().scope, tree.context().scope);
    while let Some(entry) = f
        .store
        .next_packed_tree_entry(&mut f.fs, &mut cursor)
        .unwrap()
    {
        values.push((entry.key().to_vec(), entry.value().to_vec()));
    }
    assert_eq!(cursor.report().returned_entries, values.len() as u64);
    values
}

#[test]
fn packed_tree_capabilities_private_history_stages_publish_only_terminal_and_reopen() {
    let mut f = Fixture::new(false);
    let frontier = f.store.checkpoint_anchor();
    let one = f.proof(0).unwrap();
    let two = f.proof(1).unwrap();
    let three = f.proof(2).unwrap();
    let first = stage(&mut f, &one, None, &[delta(b"a", None, Some(b"A"))]).unwrap();
    let second = stage(
        &mut f,
        &two,
        Some(first.tree()),
        &[
            delta(b"ab", None, Some(b"AB")),
            delta(b"b", None, Some(&vec![9; 20_000])),
        ],
    )
    .unwrap();
    let third = stage(
        &mut f,
        &three,
        Some(second.tree()),
        &[
            delta(b"a", Some(b"A"), Some(b"changed")),
            delta(b"ab", Some(b"AB"), None),
        ],
    )
    .unwrap();
    assert!(first.pack().is_some());
    assert!(third.report().written_nodes > 0);
    assert_eq!(f.store.checkpoint_anchor(), frontier);
    assert!(discover(&mut f, &three, limits(2)).unwrap().0.is_empty());
    assert_eq!(
        entries(&mut f, first.tree()),
        vec![(b"a".to_vec(), b"A".to_vec())]
    );
    let expected = vec![
        (b"a".to_vec(), b"changed".to_vec()),
        (b"b".to_vec(), vec![9; 20_000]),
    ];
    assert_eq!(entries(&mut f, third.tree()), expected);
    let published = publish_tree(&mut f, third.tree());
    let (admitted, report) = f
        .store
        .admit_packed_tree(&mut f.fs, &published, 1, validation())
        .unwrap();
    assert_eq!(report.entries, 2);
    assert!(
        f.store
            .packed_tree_get(&mut f.fs, &admitted, b"ab", reads())
            .unwrap()
            .value
            .is_none()
    );
    assert_eq!(
        f.store
            .packed_tree_get(&mut f.fs, &admitted, b"a", reads())
            .unwrap()
            .value
            .unwrap()
            .as_slice(),
        b"changed"
    );
    let old = admitted.clone();
    let mut cursor = f
        .store
        .open_packed_tree_cursor(&admitted, b"", None, cursors())
        .unwrap();
    let mut f = reopen(f);
    f.fs.arm(FaultPlan::default()).unwrap();
    assert!(f.store.validate_packed_tree_binding(&old).is_err());
    assert!(
        f.store
            .packed_tree_get(&mut f.fs, &old, b"a", reads())
            .is_err()
    );
    assert!(
        f.store
            .next_packed_tree_entry(&mut f.fs, &mut cursor)
            .is_err()
    );
    assert_eq!(
        f.store.next_packed_tree_entry(&mut f.fs, &mut cursor).err(),
        Some(StorageError::NeedsRecovery)
    );
    assert_eq!(f.fs.operation_count(Operation::OpenExisting), 0);
    let proof = f.proof(2).unwrap();
    let (roots, _) = discover(&mut f, &proof, limits(2)).unwrap();
    let (tree, _) = f
        .store
        .admit_packed_tree(&mut f.fs, &roots[0], 1, validation())
        .unwrap();
    assert_eq!(entries(&mut f, &tree), expected);
}

#[test]
fn packed_tree_capabilities_refuse_identity_newer_base_and_foreign_owner_before_io() {
    let mut f = Fixture::new(false);
    let staged = initial(&mut f);
    let tree = staged.tree();
    let target = f.proof(2).unwrap();
    let older = f.proof(0).unwrap();
    let scope = scope(&f);
    let foreign = Fixture::new(false);
    let foreign_proof = foreign.store.current_certificate_anchor_proof().unwrap();
    f.fs.arm(FaultPlan::default()).unwrap();
    for (changed, profile, family) in [
        (
            NamespaceRef::new(
                scope.database(),
                uste_types::NamespaceId::from_bytes([8; 16]),
            ),
            PROFILE,
            1,
        ),
        (scope, [45; 32], 1),
        (scope, PROFILE, 2),
        (scope, PROFILE, 0),
        (
            NamespaceRef::new(DatabaseId::from_bytes([0; 16]), scope.namespace()),
            PROFILE,
            1,
        ),
    ] {
        assert!(
            f.store
                .stage_packed_tree_batch_proven(
                    &mut f.fs,
                    changed,
                    profile,
                    family,
                    &target,
                    Some(tree),
                    &[],
                    batches()
                )
                .is_err()
        );
    }
    assert!(stage(&mut f, &older, Some(tree), &[]).is_err());
    assert!(stage(&mut f, &foreign_proof, None, &[]).is_err());
    assert!(
        foreign
            .store
            .packed_tree_get(&mut f.fs, tree, b"a", reads())
            .is_err()
    );
    let mut cursor = f
        .store
        .open_packed_tree_cursor(tree, b"", None, cursors())
        .unwrap();
    assert!(
        foreign
            .store
            .next_packed_tree_entry(&mut f.fs, &mut cursor)
            .is_err()
    );
    assert_eq!(
        f.store.next_packed_tree_entry(&mut f.fs, &mut cursor).err(),
        Some(StorageError::NeedsRecovery)
    );
    f.store.poisoned = true;
    assert!(
        f.store
            .packed_tree_get(&mut f.fs, tree, b"a", reads())
            .is_err()
    );
    assert!(stage(&mut f, &target, Some(tree), &[]).is_err());
    f.store.poisoned = false;
    assert_eq!(f.fs.operation_count(Operation::OpenExisting), 0);
    assert_eq!(f.fs.operation_count(Operation::CreateNew), 0);
    // Same exclusive owner may retain historical reads after append, but staging needs a fresh frontier proof.
    f.store
        .append_group(
            &mut f.fs,
            CommitInput {
                encoded_group: b"four",
                logical_event_digest: [4; 32],
            },
        )
        .unwrap();
    f.fs.arm(FaultPlan::default()).unwrap();
    assert!(stage(&mut f, &target, Some(tree), &[]).is_err());
    assert_eq!(f.fs.operation_count(Operation::OpenExisting), 0);
    assert_eq!(
        f.store
            .packed_tree_get(&mut f.fs, tree, b"a", reads())
            .unwrap()
            .value
            .unwrap()
            .as_slice(),
        b"A"
    );
}

#[test]
fn packed_tree_capabilities_explicit_empty_family_missing_family_and_late_corruption() {
    let mut f = Fixture::new(false);
    let empty = publish(&mut f, 2).unwrap();
    let (tree, report) = f
        .store
        .admit_packed_tree(&mut f.fs, &empty, 1, validation())
        .unwrap();
    assert_eq!(report.entries, 0);
    assert!(entries(&mut f, &tree).is_empty());
    f.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        f.store
            .admit_packed_tree(&mut f.fs, &empty, 2, validation())
            .is_err()
    );
    assert_eq!(f.fs.operation_count(Operation::OpenExisting), 0);
    let staged = initial(&mut f);
    let root = publish_tree(&mut f, staged.tree());
    assert_eq!(
        f.store
            .admit_packed_tree(
                &mut f.fs,
                &root,
                1,
                TreeValidationLimits {
                    maximum_pages: 0,
                    ..validation()
                }
            )
            .err(),
        Some(StorageError::ResourceLimit)
    );
    let (tree, _) = f
        .store
        .admit_packed_tree(&mut f.fs, &root, 1, validation())
        .unwrap();
    let mut cursor = f
        .store
        .open_packed_tree_cursor(&tree, b"", None, cursors())
        .unwrap();
    assert_eq!(
        f.store
            .next_packed_tree_entry(&mut f.fs, &mut cursor)
            .unwrap()
            .unwrap()
            .key(),
        b"a"
    );
    assert_eq!(
        f.store
            .next_packed_tree_entry(&mut f.fs, &mut cursor)
            .unwrap()
            .unwrap()
            .key(),
        b"ab"
    );
    let location = tree.family_descriptor().root.unwrap();
    let c = tree.context();
    let physical = location
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
            .packed_tree_get(&mut f.fs, &tree, b"b", reads())
            .is_err()
    );
    assert!(
        f.store
            .admit_packed_tree(&mut f.fs, &root, 1, validation())
            .is_err()
    );
    assert!(
        f.store
            .next_packed_tree_entry(&mut f.fs, &mut cursor)
            .is_err()
    );
    assert_eq!(
        f.store.next_packed_tree_entry(&mut f.fs, &mut cursor).err(),
        Some(StorageError::NeedsRecovery)
    );
}

#[test]
fn packed_tree_capabilities_every_admission_read_fault_returns_no_capability() {
    let mut observed = Fixture::new(false);
    let staged = initial(&mut observed);
    let root = publish_tree(&mut observed, staged.tree());
    observed.fs.arm(FaultPlan::default()).unwrap();
    let (_, report) = observed
        .store
        .admit_packed_tree(&mut observed.fs, &root, 1, validation())
        .unwrap();
    assert_eq!(report.pages, 9);
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
                        .admit_packed_tree(&mut f.fs, &root, 1, validation())
                        .is_err()
                );
                assert_eq!(f.fs.pending_faults(), 0);
                let mut f = reopen(f);
                let proof = f.proof(2).unwrap();
                let (roots, _) = discover(&mut f, &proof, limits(2)).unwrap();
                let (tree, _) = f
                    .store
                    .admit_packed_tree(&mut f.fs, &roots[0], 1, validation())
                    .unwrap();
                assert_eq!(entries(&mut f, &tree).len(), 3);
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 81);
}

#[test]
fn packed_tree_capabilities_staging_faults_preserve_published_base_and_journal() {
    let mut observed = Fixture::new(false);
    let base = initial(&mut observed);
    publish_tree(&mut observed, base.tree());
    let target = observed.proof(2).unwrap();
    let deltas = [delta(b"a", Some(b"A"), Some(b"changed"))];
    observed.fs.arm(FaultPlan::default()).unwrap();
    let changed = stage(&mut observed, &target, Some(base.tree()), &deltas).unwrap();
    assert_eq!(changed.report().read_pages, 3);
    assert_eq!(changed.report().written_pages, 1);
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
                assert!(stage(&mut f, &target, Some(base.tree()), &deltas).is_err());
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
                assert_eq!(
                    f.store
                        .packed_tree_get(&mut f.fs, &base, b"a", reads())
                        .unwrap()
                        .value
                        .unwrap()
                        .as_slice(),
                    b"A"
                );
                let retried = stage(&mut f, &proof, Some(&base), &deltas).unwrap();
                assert_eq!(
                    f.store
                        .packed_tree_get(&mut f.fs, retried.tree(), b"a", reads())
                        .unwrap()
                        .value
                        .unwrap()
                        .as_slice(),
                    b"changed"
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 42);
}

#[test]
fn packed_tree_capabilities_locked_vault_refuses_even_empty_and_finished_reads() {
    let mut f = Fixture::new(false);
    let root = publish(&mut f, 2).unwrap();
    let target = f.proof(2).unwrap();
    let (tree, _) = f
        .store
        .admit_packed_tree(&mut f.fs, &root, 1, validation())
        .unwrap();
    let mut cursor = f
        .store
        .open_packed_tree_cursor(&tree, b"", None, cursors())
        .unwrap();
    assert!(
        f.store
            .next_packed_tree_entry(&mut f.fs, &mut cursor)
            .unwrap()
            .is_none()
    );
    f.fs.arm(FaultPlan::default()).unwrap();
    f.store.vault.lock();
    let expected = Some(StorageError::Crypto(CryptoError::Locked));
    assert_eq!(f.store.validate_packed_tree_binding(&tree).err(), expected);
    assert_eq!(
        f.store
            .admit_packed_tree(&mut f.fs, &root, 1, validation())
            .err(),
        expected
    );
    assert_eq!(
        f.store
            .packed_tree_get(&mut f.fs, &tree, b"a", reads())
            .err(),
        expected
    );
    assert_eq!(
        f.store
            .open_packed_tree_cursor(&tree, b"", None, cursors())
            .err(),
        expected
    );
    assert_eq!(
        f.store.next_packed_tree_entry(&mut f.fs, &mut cursor).err(),
        expected
    );
    assert_eq!(stage(&mut f, &target, None, &[]).err(), expected);
    f.store.vault.unlock(&mut TestKeyAdapter).unwrap();
    assert_eq!(f.store.validate_packed_tree_binding(&tree), Ok(()));
    assert_eq!(
        f.store.next_packed_tree_entry(&mut f.fs, &mut cursor).err(),
        Some(StorageError::NeedsRecovery)
    );
    assert_eq!(f.fs.operation_count(Operation::OpenExisting), 0);
    assert_eq!(f.fs.operation_count(Operation::CreateNew), 0);
}
