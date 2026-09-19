use super::*;
use uste_storage::journal::{JournalRangeReadReport, StorageError};
use uste_txn::{CoordinatorRecoveryLimits, TransactionOutcome};

type Fs = FaultFileSystem<MemoryFileSystem>;
type Recovery = AuthenticatedIndexRecovery<Fs, TestEnvelope, CounterEntropy, CounterEntropy>;
const RANGE_BYTES: u64 = 3 * (4_161 + 4_161);

#[path = "transaction_cursor/packed.rs"]
mod packed;
#[path = "transaction_cursor/window.rs"]
mod window;

fn fixture() -> (Fs, EntryName, [TransactionOutcome; 3], BlobInventory) {
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let name = EntryName::new("transaction-cursor").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 910),
        CounterEntropy(911),
        CounterState::default(),
    )
    .unwrap();
    let mut upload = coordinator.start_blob_upload(scope()).unwrap();
    coordinator
        .write_blob_upload(&mut fs, &mut upload, b"synthetic cursor source")
        .unwrap();
    let reference = coordinator
        .finish_blob_upload(&mut fs, &mut upload)
        .unwrap();
    let inventory = BlobInventory::new(scope(), [reference]).unwrap();
    let outcomes = [(0, 2), (2, 3), (5, 4)].map(|(expected, delta)| {
        let key = match expected {
            0 => 1,
            2 => 2,
            _ => 3,
        };
        let bytes = mutation(expected, delta);
        let request = TransactionRequest {
            blob_inventory: (key == 3).then_some(&inventory),
            ..request(key, key + 10, &bytes)
        };
        coordinator
            .commit(&mut fs, request, &mut clock(i64::from(key)), &NeverCancel)
            .unwrap()
    });
    drop(coordinator);
    (fs, name, outcomes, inventory)
}

fn open(fs: &mut Fs, name: &EntryName, scope: NamespaceRef) -> Recovery {
    AuthenticatedIndexRecovery::open(
        fs,
        name,
        scope,
        CounterEntropy(912),
        CounterEntropy(913),
        &mut TestKeyAdapter,
    )
    .unwrap()
    .0
}

fn open_disk(fs: &mut Fs, name: &EntryName) -> (Recovery, uste_txn::RecoveredFrontierTransaction) {
    let (recovery, _, frontier) = AuthenticatedIndexRecovery::open_with_disk_certificate_anchors(
        fs,
        name,
        scope(),
        CounterEntropy(912),
        CounterEntropy(913),
        &mut TestKeyAdapter,
        uste_storage::journal::CertificateAnchorReadLimits::new(3, 3 * 4161).unwrap(),
    )
    .unwrap();
    (recovery, frontier.unwrap())
}

#[test]
fn cursor_retained_proofs_remove_stage_reads_without_authorizing_another_owner() {
    let (mut fs, name, outcomes, _) = fixture();
    let (mut recovery, unbound_frontier) = open_disk(&mut fs, &name);
    let mut cursor = recovery
        .open_transaction_cursor(
            CommitRevision::FIRST,
            outcomes[2].revision,
            3,
            RANGE_BYTES + 6 * 4161,
        )
        .unwrap();
    let mut retained = Vec::new();
    while let Some(transaction) = recovery
        .next_recovered_transaction(&mut fs, &mut cursor)
        .unwrap()
    {
        fs.arm(FaultPlan::default()).unwrap();
        for _ in 0..3 {
            let stage = recovery
                .stage_indexes_with_io(&mut fs, &transaction)
                .unwrap();
            assert_eq!(
                stage.anchor().unwrap(),
                (transaction.revision(), *transaction.certificate_digest())
            );
        }
        recovery.stage_indexes(&transaction).unwrap();
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
        retained.push(transaction);
    }
    let report = recovery.finish_transaction_cursor(cursor).unwrap();
    assert_eq!(report.encoded_bytes, RANGE_BYTES + 6 * 4161);
    assert_eq!(retained[2], unbound_frontier); // transient proof is not transaction content
    fs.arm(FaultPlan::default()).unwrap();
    recovery
        .stage_indexes_with_io(&mut fs, &unbound_frontier)
        .unwrap();
    assert_eq!(fs.operation_count(Operation::ReadAt), 1); // explicit fallback still reauthenticates

    let (mut foreign_fs, foreign_name, _, _) = fixture();
    for disk in [false, true] {
        let mut foreign = if disk {
            open_disk(&mut foreign_fs, &foreign_name).0
        } else {
            open(&mut foreign_fs, &foreign_name, scope())
        };
        foreign_fs.arm(FaultPlan::default()).unwrap();
        for transaction in &retained {
            assert!(
                foreign
                    .stage_indexes_with_io(&mut foreign_fs, transaction)
                    .is_err()
            );
            assert!(foreign.stage_indexes(transaction).is_err());
        }
        assert_eq!(foreign_fs.operation_count(Operation::ReadAt), 0);
        assert_eq!(foreign_fs.operation_count(Operation::CreateNew), 0);
    }
    // Existing receipts bind admitted content, not continuous rereads. Fresh cursors still fail.
    let directory = fs.open_directory(&fs.root(), &name).unwrap();
    let certificates = fs
        .open_existing(&directory, &EntryName::new("CERTIFICATES").unwrap())
        .unwrap();
    let offset = 3 * 4161 + 127;
    let mut byte = [0];
    fs.read_at(&certificates, offset, &mut byte).unwrap();
    fs.write_at(&certificates, offset, &[byte[0] ^ 1]).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    recovery
        .stage_indexes_with_io(&mut fs, &retained[0])
        .unwrap();
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    let mut fresh = recovery
        .open_transaction_cursor(CommitRevision::FIRST, outcomes[2].revision, 3, 100_000)
        .unwrap();
    assert!(
        recovery
            .next_recovered_transaction(&mut fs, &mut fresh)
            .is_err()
    );
    assert!(recovery.finish_transaction_cursor(fresh).is_err());
    fs.write_at(&certificates, offset, &byte).unwrap();
    drop(recovery);
    let (mut reopened, _) = open_disk(&mut fs, &name);
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        reopened
            .stage_indexes_with_io(&mut fs, &retained[0])
            .is_err()
    );
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
}

#[test]
fn disk_cursor_retained_proofs_preserve_every_read_failure_and_sticky_refusal() {
    let (mut fs, name, outcomes, _) = fixture();
    let (recovery, _) = open_disk(&mut fs, &name);
    let bytes = RANGE_BYTES + 6 * 4161;
    fs.arm(FaultPlan::default()).unwrap();
    recovery
        .visit_transactions(
            &mut fs,
            CommitRevision::FIRST,
            outcomes[2].revision,
            3,
            bytes,
            |_, _| Ok(()),
        )
        .unwrap();
    let reads = fs.operation_count(Operation::ReadAt);
    assert!(reads > 6);
    for occurrence in 1..=reads {
        fs.arm(
            FaultPlan::new([FaultPoint {
                operation: Operation::ReadAt,
                occurrence,
                action: FaultAction::Error(AdapterErrorKind::Io),
            }])
            .unwrap(),
        )
        .unwrap();
        let mut cursor = recovery
            .open_transaction_cursor(CommitRevision::FIRST, outcomes[2].revision, 3, bytes)
            .unwrap();
        loop {
            match recovery.next_recovered_transaction(&mut fs, &mut cursor) {
                Ok(Some(_)) => {}
                Ok(None) => panic!("selected read fault must reject the range"),
                Err(_) => break,
            }
        }
        assert_eq!(fs.pending_faults(), 0);
        fs.arm(FaultPlan::default()).unwrap();
        assert!(
            recovery
                .next_recovered_transaction(&mut fs, &mut cursor)
                .is_err()
        );
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        assert!(recovery.finish_transaction_cursor(cursor).is_err());
    }
    recovery
        .visit_transactions(
            &mut fs,
            CommitRevision::FIRST,
            outcomes[2].revision,
            3,
            bytes,
            |_, _| Ok(()),
        )
        .unwrap();
}

#[test]
fn inventory_free_genesis_preserves_terminal_authority_and_reauthenticates_reads() {
    let (mut fs, name, outcomes, _) = fixture();
    let recovery = open(&mut fs, &name, scope());
    assert!(
        recovery
            .recover_inventory_free_genesis(&mut fs, CounterState::default(), 1,)
            .is_err()
    );
    assert!(
        recovery
            .recover_inventory_free_genesis(&mut fs, CounterState(1), RANGE_BYTES,)
            .is_err()
    );
    fs.arm(FaultPlan::default()).unwrap();
    let genesis = recovery
        .recover_inventory_free_genesis(&mut fs, CounterState::default(), RANGE_BYTES)
        .unwrap();
    assert_eq!(genesis.state(), &CounterState(2));
    assert_eq!(genesis.transaction().outcome(), outcomes[0]);
    assert_eq!(genesis.transaction().revision(), CommitRevision::FIRST);
    let reads = fs.operation_count(Operation::ReadAt);
    assert!(reads > 0);
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
    for occurrence in 1..=reads {
        fs.arm(
            FaultPlan::new([FaultPoint {
                operation: Operation::ReadAt,
                occurrence,
                action: FaultAction::Error(AdapterErrorKind::Io),
            }])
            .unwrap(),
        )
        .unwrap();
        assert!(
            recovery
                .recover_inventory_free_genesis(&mut fs, CounterState::default(), RANGE_BYTES,)
                .is_err()
        );
        assert_eq!(fs.pending_faults(), 0);
        assert_eq!(fs.operation_count(Operation::WriteAt), 0);
    }
    fs.arm(FaultPlan::default()).unwrap();
    let mut cursor = recovery
        .open_transaction_cursor(outcomes[2].revision, outcomes[2].revision, 1, RANGE_BYTES)
        .unwrap();
    assert_eq!(
        recovery
            .next_recovered_transaction(&mut fs, &mut cursor)
            .unwrap()
            .unwrap()
            .outcome(),
        outcomes[2]
    );
    recovery.finish_transaction_cursor(cursor).unwrap();
    drop(recovery);
    let recovery = open(&mut fs, &name, scope());
    let coordinator = recovery
        .into_bounded_coordinator(
            &mut fs,
            CounterState::default(),
            RetentionDays::new(30).unwrap(),
            CoordinatorRecoveryLimits::new(3, 1).unwrap(),
            RANGE_BYTES,
        )
        .unwrap();
    assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(9));
}

#[test]
fn disk_blob_recovery_captures_exact_transaction_inventory_and_streams_without_history_maps() {
    use uste_storage::journal::{
        BlobCatalogRecovery, BlobMetadataAdmissionLimits, BlobMetadataRebuildLimits,
        BlobRecoveryLimits, CertificateAnchorReadLimits,
    };
    use uste_storage::{IndexGetLimits, IndexRunMergeLimits, IndexRunReadLimits, PageCache};
    let (mut fs, name, outcomes, inventory) = fixture();
    let run = IndexRunReadLimits::new(8, 8, 1024).unwrap();
    let limits = BlobRecoveryLimits {
        catalog: BlobMetadataRebuildLimits {
            admission: BlobMetadataAdmissionLimits {
                maximum_blobs: 1,
                maximum_namespaces: 1,
                maximum_inventories: 1,
                maximum_reference_bindings: 1,
                run,
                lookup: IndexGetLimits::new(8, 136).unwrap(),
                certificates: CertificateAnchorReadLimits::new(3, 3 * 4161).unwrap(),
                maximum_journal_groups: 3,
                maximum_journal_encoded_bytes: 1_000_000,
            },
            merge: IndexRunMergeLimits::new(run, 8, 1024, 8, 1024).unwrap(),
            maximum_merge_output_bytes: 1024,
        },
        catalog_recovery: BlobCatalogRecovery::AdmitOrRebuild,
        maximum_verified_blob_bytes_per_pass: inventory.references()[0].byte_len(),
        maximum_uncommitted_segment_tails: 3,
    };
    let (recovery, report, frontier) = AuthenticatedIndexRecovery::open_with_disk_blob_metadata(
        &mut fs,
        &name,
        scope(),
        CounterEntropy(914),
        CounterEntropy(915),
        &mut TestKeyAdapter,
        limits,
        &mut PageCache::new(64 * 1024).unwrap(),
    )
    .unwrap();
    let frontier = frontier.unwrap();
    assert_eq!(report.frontier, Some(outcomes[2].revision));
    assert_eq!(frontier.outcome(), outcomes[2]);
    assert_eq!(frontier.blob_inventory(), Some(&inventory));
    let bytes = RANGE_BYTES + 6 * 4161;
    let mut cursor = recovery
        .open_transaction_cursor(CommitRevision::FIRST, outcomes[2].revision, 3, bytes)
        .unwrap();
    for (index, outcome) in outcomes.iter().enumerate() {
        let transaction = recovery
            .next_recovered_transaction(&mut fs, &mut cursor)
            .unwrap()
            .unwrap();
        assert_eq!(transaction.outcome(), *outcome);
        assert_eq!(
            transaction.blob_inventory(),
            (index == 2).then_some(&inventory)
        );
    }
    assert!(
        recovery
            .next_recovered_transaction(&mut fs, &mut cursor)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        recovery.finish_transaction_cursor(cursor).unwrap(),
        JournalRangeReadReport {
            groups: 3,
            encoded_bytes: bytes
        }
    );
    let mut reverse = Vec::new();
    recovery
        .visit_transactions_reverse(
            &mut fs,
            CommitRevision::FIRST,
            outcomes[2].revision,
            3,
            RANGE_BYTES + 4161,
            |_, transaction| {
                assert_eq!(
                    transaction.blob_inventory(),
                    (transaction.revision().get() == 3).then_some(&inventory)
                );
                reverse.push(transaction.outcome());
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(reverse, outcomes.into_iter().rev().collect::<Vec<_>>());
    assert!(
        recovery
            .visit_transactions_reverse(
                &mut fs,
                CommitRevision::FIRST,
                outcomes[2].revision,
                3,
                RANGE_BYTES + 4160,
                |_, _| Ok(()),
            )
            .is_err()
    );
}

#[test]
fn transaction_cursor_exact_budgets_owned_inventory_and_terminal_report() {
    let (mut fs, name, outcomes, inventory) = fixture();
    let recovery = open(&mut fs, &name, scope());
    let last = CommitRevision::new(3).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        recovery
            .open_transaction_cursor(CommitRevision::FIRST, last, 2, RANGE_BYTES)
            .is_err()
    );
    assert!(
        recovery
            .open_transaction_cursor(last, CommitRevision::FIRST, 3, RANGE_BYTES)
            .is_err()
    );
    assert!(
        recovery
            .open_transaction_cursor(
                CommitRevision::FIRST,
                CommitRevision::new(4).unwrap(),
                4,
                RANGE_BYTES
            )
            .is_err()
    );
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    let unfinished = recovery
        .open_transaction_cursor(CommitRevision::FIRST, last, 3, RANGE_BYTES)
        .unwrap();
    assert!(recovery.finish_transaction_cursor(unfinished).is_err());
    let mut cursor = recovery
        .open_transaction_cursor(CommitRevision::FIRST, last, 3, RANGE_BYTES)
        .unwrap();
    for (i, outcome) in outcomes.iter().enumerate() {
        let transaction = recovery
            .next_recovered_transaction(&mut fs, &mut cursor)
            .unwrap()
            .unwrap();
        assert_eq!(transaction.revision(), outcome.revision);
        assert_eq!(transaction.outcome(), *outcome);
        assert_eq!(
            transaction.canonical_request(),
            &[(0, 2), (2, 3), (5, 4)].map(|(expected, delta)| mutation(expected, delta))[i]
        );
        assert_eq!(transaction.blob_inventory(), (i == 2).then_some(&inventory));
        assert!(!format!("{cursor:?}").contains("canonical_request"));
    }
    assert!(
        recovery
            .next_recovered_transaction(&mut fs, &mut cursor)
            .unwrap()
            .is_none()
    );
    let report = recovery.finish_transaction_cursor(cursor).unwrap();
    assert_eq!(
        report,
        JournalRangeReadReport {
            groups: 3,
            encoded_bytes: RANGE_BYTES
        }
    );
    let mut short = recovery
        .open_transaction_cursor(CommitRevision::FIRST, last, 3, RANGE_BYTES - 1)
        .unwrap();
    for _ in 0..2 {
        recovery
            .next_recovered_transaction(&mut fs, &mut short)
            .unwrap()
            .unwrap();
    }
    assert_eq!(
        recovery
            .next_recovered_transaction(&mut fs, &mut short)
            .unwrap_err(),
        TransactionError::Storage(StorageError::ResourceLimit)
    );
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        recovery
            .next_recovered_transaction(&mut fs, &mut short)
            .is_err()
    );
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert!(recovery.finish_transaction_cursor(short).is_err());
    let mut suffix = recovery
        .open_transaction_cursor(CommitRevision::new(2).unwrap(), last, 2, RANGE_BYTES)
        .unwrap();
    while recovery
        .next_recovered_transaction(&mut fs, &mut suffix)
        .unwrap()
        .is_some()
    {}
    assert_eq!(
        recovery.finish_transaction_cursor(suffix).unwrap(),
        JournalRangeReadReport {
            groups: 2,
            encoded_bytes: 2 * (4_161 + 4_161)
        }
    );
}

#[test]
fn transaction_cursor_rejects_every_read_fault_and_late_corruption_without_resume() {
    let (mut fs, name, _, _) = fixture();
    let recovery = open(&mut fs, &name, scope());
    let last = CommitRevision::new(3).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    recovery
        .visit_transactions(
            &mut fs,
            CommitRevision::FIRST,
            last,
            3,
            RANGE_BYTES,
            |_, _| Ok(()),
        )
        .unwrap();
    let reads = fs.operation_count(Operation::ReadAt);
    assert!(reads > 3);
    for occurrence in 1..=reads {
        let mut cursor = recovery
            .open_transaction_cursor(CommitRevision::FIRST, last, 3, RANGE_BYTES)
            .unwrap();
        fs.arm(
            FaultPlan::new([FaultPoint {
                operation: Operation::ReadAt,
                occurrence,
                action: FaultAction::Error(AdapterErrorKind::Io),
            }])
            .unwrap(),
        )
        .unwrap();
        loop {
            match recovery.next_recovered_transaction(&mut fs, &mut cursor) {
                Ok(Some(_)) => {}
                Ok(None) => panic!("read fault must reject the provisional range"),
                Err(_) => break,
            }
        }
        assert_eq!(fs.pending_faults(), 0);
        fs.arm(FaultPlan::default()).unwrap();
        assert!(
            recovery
                .next_recovered_transaction(&mut fs, &mut cursor)
                .is_err()
        );
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        assert!(recovery.finish_transaction_cursor(cursor).is_err());
    }
    let mut cursor = recovery
        .open_transaction_cursor(CommitRevision::FIRST, last, 3, RANGE_BYTES)
        .unwrap();
    recovery
        .next_recovered_transaction(&mut fs, &mut cursor)
        .unwrap()
        .unwrap();
    let directory = fs.open_directory(&fs.root(), &name).unwrap();
    let certificates = fs
        .open_existing(&directory, &EntryName::new("CERTIFICATES").unwrap())
        .unwrap();
    let offset = 3 * 4_161 + 127;
    let mut byte = [0];
    assert_eq!(fs.read_at(&certificates, offset, &mut byte).unwrap(), 1);
    byte[0] ^= 1;
    assert_eq!(fs.write_at(&certificates, offset, &byte).unwrap(), 1);
    recovery
        .next_recovered_transaction(&mut fs, &mut cursor)
        .unwrap()
        .unwrap();
    assert_eq!(
        recovery
            .next_recovered_transaction(&mut fs, &mut cursor)
            .unwrap_err(),
        TransactionError::IntegrityFailure
    );
    byte[0] ^= 1;
    assert_eq!(fs.write_at(&certificates, offset, &byte).unwrap(), 1);
    assert!(
        recovery
            .next_recovered_transaction(&mut fs, &mut cursor)
            .is_err()
    );
    assert!(recovery.finish_transaction_cursor(cursor).is_err());
    recovery
        .visit_transactions(
            &mut fs,
            CommitRevision::FIRST,
            last,
            3,
            RANGE_BYTES,
            |_, _| Ok(()),
        )
        .unwrap();
}

#[test]
fn transaction_cursor_scope_and_frontier_binding_precede_io() {
    let (mut fs, name, _, _) = fixture();
    let recovery = open(&mut fs, &name, scope());
    let last = CommitRevision::new(3).unwrap();
    let cursor = || {
        recovery
            .open_transaction_cursor(CommitRevision::FIRST, last, 3, RANGE_BYTES)
            .unwrap()
    };
    let (mut other_fs, other_name, _, _) = fixture();
    let mut foreign = open(
        &mut other_fs,
        &other_name,
        NamespaceRef::new(scope().database(), NamespaceId::from_bytes([9; 16])),
    );
    other_fs.arm(FaultPlan::default()).unwrap();
    let mut wrong_scope = cursor();
    assert!(
        foreign
            .next_recovered_transaction(&mut other_fs, &mut wrong_scope)
            .is_err()
    );
    assert_eq!(other_fs.operation_count(Operation::ReadAt), 0);
    let mut original_cursor = cursor();
    let transaction = recovery
        .next_recovered_transaction(&mut fs, &mut original_cursor)
        .unwrap()
        .unwrap();
    assert!(foreign.stage_indexes(&transaction).is_err());
    assert_eq!(other_fs.operation_count(Operation::ReadAt), 0);
    drop(foreign);
    let newer = open(&mut other_fs, &other_name, scope());
    let mut coordinator = newer
        .into_bounded_coordinator(
            &mut other_fs,
            CounterState::default(),
            RetentionDays::new(30).unwrap(),
            CoordinatorRecoveryLimits::new(4, 1).unwrap(),
            RANGE_BYTES,
        )
        .unwrap();
    coordinator
        .commit(
            &mut other_fs,
            request(4, 14, &mutation(9, 1)),
            &mut clock(4),
            &NeverCancel,
        )
        .unwrap();
    drop(coordinator);
    let newer = open(&mut other_fs, &other_name, scope());
    other_fs.arm(FaultPlan::default()).unwrap();
    let mut stale = cursor();
    assert!(
        newer
            .next_recovered_transaction(&mut other_fs, &mut stale)
            .is_err()
    );
    assert_eq!(other_fs.operation_count(Operation::ReadAt), 0);
    assert!(newer.finish_transaction_cursor(stale).is_err());
}
