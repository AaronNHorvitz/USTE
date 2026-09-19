use super::*;
#[path = "packed_roots.rs"]
mod roots;
use uste_storage::{
    IndexDelta, journal::CertificateAnchorReadLimits, packed_index_pack::PackWriteLimits,
    packed_tree_batch::TreeBatchLimits, packed_tree_cursor::TreeCursorLimits,
    packed_tree_lookup::TreeLookupLimits,
};

fn certificate_limits() -> CertificateAnchorReadLimits {
    CertificateAnchorReadLimits::new(3, 3 * 4161).unwrap()
}
fn batches() -> TreeBatchLimits {
    TreeBatchLimits {
        maximum_deltas: 16,
        maximum_input_bytes: 64_000,
        maximum_dirty_nodes: 128,
        maximum_path_branches: 128,
        maximum_read_pages: 128,
        maximum_read_bytes: 128 * 20545,
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
        maximum_encoded_bytes: 128 * 20545,
        maximum_value_bytes: 32_000,
    }
}
fn cursors() -> TreeCursorLimits {
    TreeCursorLimits {
        maximum_path_branches: 128,
        maximum_candidates: 128,
        maximum_returned_bytes: 64_000,
        maximum_pages: 128,
        maximum_encoded_bytes: 128 * 20545,
    }
}

#[test]
fn packed_ordinary_maintenance_checks_frontier_budget_and_keeps_retry_semantics() {
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let mut coordinator = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).unwrap(),
        EntryName::new("packed-ordinary").unwrap(),
        create_vault(scope().database(), 1001),
        CounterEntropy(1002),
        CounterState::default(),
    )
    .unwrap();
    assert!(
        coordinator
            .reducer_and_index_maintenance()
            .unwrap()
            .indexes
            .packed_indexes(&mut fs, certificate_limits())
            .is_err()
    );
    let bytes = mutation(0, 2);
    let outcome = coordinator
        .commit(&mut fs, request(1, 11, &bytes), &mut clock(1), &NeverCancel)
        .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    {
        let mut disjoint = coordinator.reducer_and_index_maintenance().unwrap();
        assert_eq!(disjoint.reducer.0, 2);
        assert!(
            disjoint
                .indexes
                .packed_indexes(&mut fs, CertificateAnchorReadLimits::new(1, 4160).unwrap())
                .is_err()
        );
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        let mut maintenance = disjoint
            .indexes
            .packed_indexes(&mut fs, certificate_limits())
            .unwrap();
        assert_eq!(maintenance.anchor().0, outcome.revision);
        assert_eq!(fs.operation_count(Operation::ReadAt), 1);
        let staged = maintenance
            .stage(
                &mut fs,
                [7; 32],
                1,
                None,
                &[IndexDelta::new(b"key".to_vec(), None, Some(b"value".to_vec())).unwrap()],
                batches(),
            )
            .unwrap();
        let mut cursor = maintenance
            .cursor(staged.tree(), b"", None, cursors())
            .unwrap();
        let entry = maintenance.next(&mut fs, &mut cursor).unwrap().unwrap();
        assert_eq!(entry.key(), b"key");
        assert_eq!(entry.value(), b"value");
        assert!(maintenance.next(&mut fs, &mut cursor).unwrap().is_none());
        assert_eq!(cursor.report().returned_entries, 1);
    }
    assert_eq!(
        coordinator
            .commit(&mut fs, request(1, 11, &bytes), &mut clock(2), &NeverCancel)
            .unwrap(),
        outcome
    );
    assert!(
        coordinator
            .commit(
                &mut fs,
                request(2, 11, &mutation(2, 1)),
                &mut clock(2),
                &NeverCancel
            )
            .is_err()
    );
}

#[test]
fn packed_recovery_unbound_authentication_faults_never_issue_a_handle() {
    // The live owner retains the certificate handle: authentication performs only ReadAt.
    for operation in [Operation::ReadAt] {
        for action in [
            FaultAction::Error(AdapterErrorKind::Io),
            FaultAction::CrashBefore,
            FaultAction::CrashAfter,
        ] {
            let (mut fs, name, _, _) = fixture();
            let (mut recovery, frontier) = open_disk(&mut fs, &name);
            fs.arm(
                FaultPlan::new([FaultPoint {
                    operation,
                    occurrence: 1,
                    action,
                }])
                .unwrap(),
            )
            .unwrap();
            assert!(
                recovery
                    .packed_indexes_with_io(&mut fs, &frontier, certificate_limits())
                    .is_err()
            );
            assert_eq!(fs.pending_faults(), 0);
            assert_eq!(fs.operation_count(Operation::CreateNew), 0);
        }
    }
}

#[test]
fn packed_recovery_stages_bind_target_and_reject_future_and_foreign_receipts_before_io() {
    let (mut fs, name, outcomes, _) = fixture();
    let (mut recovery, _) = open_disk(&mut fs, &name);
    let mut cursor = recovery
        .open_transaction_cursor(
            CommitRevision::FIRST,
            outcomes[2].revision,
            3,
            RANGE_BYTES + 6 * 4161,
        )
        .unwrap();
    let mut transactions = Vec::new();
    while let Some(transaction) = recovery
        .next_recovered_transaction(&mut fs, &mut cursor)
        .unwrap()
    {
        transactions.push(transaction);
    }
    recovery.finish_transaction_cursor(cursor).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    let first = {
        let mut maintenance = recovery
            .packed_indexes_with_io(&mut fs, &transactions[0], certificate_limits())
            .unwrap();
        assert_eq!(maintenance.anchor().0, outcomes[0].revision);
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        maintenance
            .stage(
                &mut fs,
                [7; 32],
                1,
                None,
                &[IndexDelta::new(b"a".to_vec(), None, Some(b"first".to_vec())).unwrap()],
                batches(),
            )
            .unwrap()
    };
    let second = {
        let mut maintenance = recovery
            .packed_indexes_with_io(&mut fs, &transactions[1], certificate_limits())
            .unwrap();
        maintenance
            .stage(
                &mut fs,
                [7; 32],
                1,
                Some(first.tree()),
                &[IndexDelta::new(
                    b"a".to_vec(),
                    Some(b"first".to_vec()),
                    Some(b"second".to_vec()),
                )
                .unwrap()],
                batches(),
            )
            .unwrap()
    };
    let mut future_cursor = {
        let maintenance = recovery
            .packed_indexes_with_io(&mut fs, &transactions[1], certificate_limits())
            .unwrap();
        assert_eq!(
            maintenance
                .get(&mut fs, second.tree(), b"a", reads())
                .unwrap()
                .value
                .as_ref()
                .map(|value| value.as_slice()),
            Some(b"second".as_slice())
        );
        maintenance
            .cursor(second.tree(), b"", None, cursors())
            .unwrap()
    };
    fs.arm(FaultPlan::default()).unwrap();
    {
        let mut maintenance = recovery
            .packed_indexes_with_io(&mut fs, &transactions[0], certificate_limits())
            .unwrap();
        assert!(
            maintenance
                .get(&mut fs, second.tree(), b"a", reads())
                .is_err()
        );
        assert!(
            maintenance
                .stage(&mut fs, [7; 32], 1, Some(second.tree()), &[], batches())
                .is_err()
        );
        assert!(
            maintenance
                .cursor(second.tree(), b"", None, cursors())
                .is_err()
        );
        assert!(maintenance.next(&mut fs, &mut future_cursor).is_err());
    }
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    {
        let maintenance = recovery
            .packed_indexes_with_io(&mut fs, &transactions[1], certificate_limits())
            .unwrap();
        assert!(matches!(
            maintenance.next(&mut fs, &mut future_cursor),
            Err(TransactionError::Storage(StorageError::NeedsRecovery))
        ));
        assert_eq!(
            maintenance
                .get(&mut fs, first.tree(), b"a", reads())
                .unwrap()
                .value
                .as_ref()
                .map(|value| value.as_slice()),
            Some(b"first".as_slice())
        );
    }
    let (mut foreign_fs, foreign_name, _, _) = fixture();
    let (mut foreign, _) = open_disk(&mut foreign_fs, &foreign_name);
    foreign_fs.arm(FaultPlan::default()).unwrap();
    for transaction in &transactions {
        assert!(
            foreign
                .packed_indexes_with_io(&mut foreign_fs, transaction, certificate_limits())
                .is_err()
        );
    }
    assert_eq!(foreign_fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(foreign_fs.operation_count(Operation::CreateNew), 0);
}
