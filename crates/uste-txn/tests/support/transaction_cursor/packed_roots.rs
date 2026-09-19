use super::*;
use uste_storage::{
    journal::PackedRootDiscoveryLimits, packed_root_manifest::PackedRootClaims,
    packed_tree_validation::TreeValidationLimits,
};

fn claims(transaction: &uste_txn::RecoveredFrontierTransaction) -> PackedRootClaims {
    PackedRootClaims {
        revision: transaction.revision(),
        generation: 1,
        certificate_digest: *transaction.certificate_digest(),
        reducer_profile: [9; 32],
        state_commitment_profile: [10; 32],
        state_digest: [11; 32],
    }
}
fn discovery() -> PackedRootDiscoveryLimits {
    PackedRootDiscoveryLimits::new(2, 2 * 4177).unwrap()
}
fn validation() -> TreeValidationLimits {
    TreeValidationLimits {
        maximum_path_branches: 128,
        maximum_nodes: 128,
        maximum_logical_bytes: 64000,
        maximum_pages: 128,
        maximum_encoded_bytes: 128 * 20545,
    }
}

type Coordinator =
    CommitCoordinator<CounterState, Fs, TestEnvelope, CounterEntropy, CounterEntropy>;
fn ordinary(namespace: NamespaceRef, name: &str) -> (Fs, Coordinator, PackedRootClaims) {
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let mut coordinator = CommitCoordinator::create(
        &mut fs,
        namespace,
        RetentionDays::new(30).unwrap(),
        EntryName::new(name).unwrap(),
        create_vault(namespace.database(), 1201),
        CounterEntropy(1202),
        CounterState::default(),
    )
    .unwrap();
    coordinator
        .commit(
            &mut fs,
            request(1, 11, &mutation(0, 1)),
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();
    let anchor = coordinator
        .reducer_and_index_maintenance()
        .unwrap()
        .indexes
        .checkpoint_anchor()
        .unwrap();
    (
        fs,
        coordinator,
        PackedRootClaims {
            revision: anchor.0,
            certificate_digest: anchor.1,
            generation: 1,
            reducer_profile: [9; 32],
            state_commitment_profile: [10; 32],
            state_digest: [11; 32],
        },
    )
}

#[test]
fn packed_scoped_ordinary_discovery_is_current_and_rejects_wrong_claims_before_creation() {
    let (mut fs, mut coordinator, claims) = ordinary(scope(), "packed-root-ordinary");
    {
        let mut disjoint = coordinator.reducer_and_index_maintenance().unwrap();
        let stage = disjoint
            .indexes
            .packed_indexes(&mut fs, certificate_limits())
            .unwrap()
            .stage(&mut fs, [7; 32], 1, None, &[], batches())
            .unwrap();
        let family = stage.tree().family_descriptor();
        fs.arm(FaultPlan::default()).unwrap();
        let mut wrong = claims;
        wrong.certificate_digest[0] ^= 1;
        assert!(
            disjoint
                .indexes
                .publish_packed_root(&mut fs, [7; 32], wrong, &[family], 2)
                .is_err()
        );
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
        disjoint
            .indexes
            .publish_packed_root(&mut fs, [7; 32], claims, &[family], 2)
            .unwrap();
        let (roots, report) = disjoint
            .indexes
            .discover_packed_roots(&mut fs, [7; 32], certificate_limits(), discovery())
            .unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(report.encoded_bytes, 4177);
        let maintenance = disjoint
            .indexes
            .packed_indexes(&mut fs, certificate_limits())
            .unwrap();
        let (tree, report) = maintenance
            .admit(&mut fs, &roots[0], 1, validation())
            .unwrap();
        assert_eq!(report.entries, 0);
        assert!(
            maintenance
                .get(&mut fs, &tree, b"missing", reads())
                .unwrap()
                .value
                .is_none()
        );
    }
    coordinator
        .commit(
            &mut fs,
            request(2, 12, &mutation(1, 1)),
            &mut clock(2),
            &NeverCancel,
        )
        .unwrap();
    let disjoint = coordinator.reducer_and_index_maintenance().unwrap();
    assert!(
        disjoint
            .indexes
            .discover_packed_roots(&mut fs, [7; 32], certificate_limits(), discovery())
            .unwrap()
            .0
            .is_empty()
    );
}

#[test]
fn packed_scoped_wrong_namespace_admission_and_cursor_fail_before_tree_io() {
    let other_scope = NamespaceRef::new(scope().database(), NamespaceId::from_bytes([8; 16]));
    let (mut other_fs, mut other, claims) = ordinary(other_scope, "packed-other-scope");
    let (root, tree, mut cursor) = {
        let mut disjoint = other.reducer_and_index_maintenance().unwrap();
        let stage = disjoint
            .indexes
            .packed_indexes(&mut other_fs, certificate_limits())
            .unwrap()
            .stage(&mut other_fs, [7; 32], 1, None, &[], batches())
            .unwrap();
        let root = disjoint
            .indexes
            .publish_packed_root(
                &mut other_fs,
                [7; 32],
                claims,
                &[stage.tree().family_descriptor()],
                2,
            )
            .unwrap();
        let maintenance = disjoint
            .indexes
            .packed_indexes(&mut other_fs, certificate_limits())
            .unwrap();
        let cursor = maintenance
            .cursor(stage.tree(), b"", None, cursors())
            .unwrap();
        (root, stage.tree().clone(), cursor)
    };
    let (mut fs, mut coordinator, _) = ordinary(scope(), "packed-scope");
    {
        let mut disjoint = coordinator.reducer_and_index_maintenance().unwrap();
        let mut maintenance = disjoint
            .indexes
            .packed_indexes(&mut fs, certificate_limits())
            .unwrap();
        fs.arm(FaultPlan::default()).unwrap();
        assert!(maintenance.admit(&mut fs, &root, 1, validation()).is_err());
        assert!(
            maintenance
                .get(&mut fs, &tree, b"missing", reads())
                .is_err()
        );
        assert!(
            maintenance
                .stage(&mut fs, [7; 32], 1, Some(&tree), &[], batches())
                .is_err()
        );
        assert!(maintenance.cursor(&tree, b"", None, cursors()).is_err());
        assert!(maintenance.next(&mut fs, &mut cursor).is_err());
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    }
    let mut disjoint = other.reducer_and_index_maintenance().unwrap();
    let maintenance = disjoint
        .indexes
        .packed_indexes(&mut other_fs, certificate_limits())
        .unwrap();
    assert!(matches!(
        maintenance.next(&mut other_fs, &mut cursor),
        Err(TransactionError::Storage(StorageError::NeedsRecovery))
    ));
}

#[test]
fn packed_scoped_recovery_publishes_only_terminal_and_cold_discovers_exact_family() {
    let (mut fs, name, outcomes, _) = fixture();
    let (mut recovery, frontier) = open_disk(&mut fs, &name);
    let mut cursor = recovery
        .open_transaction_cursor(
            CommitRevision::FIRST,
            outcomes[2].revision,
            3,
            RANGE_BYTES + 6 * 4161,
        )
        .unwrap();
    let first = recovery
        .next_recovered_transaction(&mut fs, &mut cursor)
        .unwrap()
        .unwrap();
    let stage = recovery
        .packed_indexes_with_io(&mut fs, &first, certificate_limits())
        .unwrap()
        .stage(
            &mut fs,
            [7; 32],
            1,
            None,
            &[IndexDelta::new(b"a".to_vec(), None, Some(b"value".to_vec())).unwrap()],
            batches(),
        )
        .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        recovery
            .publish_recovered_packed_root(
                &mut fs,
                [7; 32],
                claims(&first),
                &[stage.tree().family_descriptor()],
                2
            )
            .is_err()
    );
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    assert!(
        recovery
            .discover_packed_roots_at_revision(
                &mut fs,
                [7; 32],
                first.revision(),
                certificate_limits(),
                discovery()
            )
            .unwrap()
            .0
            .is_empty()
    );
    let terminal = recovery
        .packed_indexes_with_io(&mut fs, &frontier, certificate_limits())
        .unwrap()
        .stage(&mut fs, [7; 32], 1, Some(stage.tree()), &[], batches())
        .unwrap();
    let root = recovery
        .publish_recovered_packed_root(
            &mut fs,
            [7; 32],
            claims(&frontier),
            &[terminal.tree().family_descriptor()],
            2,
        )
        .unwrap();
    let family = root.manifest().families()[0];
    let admitted = recovery
        .packed_indexes_with_io(&mut fs, &frontier, certificate_limits())
        .unwrap()
        .admit(&mut fs, &root, 1, validation())
        .unwrap()
        .0;
    drop(recovery);
    let (mut recovery, frontier) = open_disk(&mut fs, &name);
    let (roots, report) = recovery
        .discover_packed_roots_at_revision(
            &mut fs,
            [7; 32],
            frontier.revision(),
            certificate_limits(),
            discovery(),
        )
        .unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(report.encoded_bytes, 4177);
    assert!(roots[0].manifest().families()[0] == family);
    let maintenance = recovery
        .packed_indexes_with_io(&mut fs, &frontier, certificate_limits())
        .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert!(maintenance.get(&mut fs, &admitted, b"a", reads()).is_err());
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    let admitted = maintenance
        .admit(&mut fs, &roots[0], 1, validation())
        .unwrap()
        .0;
    assert_eq!(
        maintenance
            .get(&mut fs, &admitted, b"a", reads())
            .unwrap()
            .value
            .unwrap()
            .as_slice(),
        b"value"
    );
}
