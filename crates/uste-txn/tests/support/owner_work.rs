use super::*;

struct NoSnapshot(CounterState);
impl TransactionState for NoSnapshot {
    type Prepared = i64;
    type Snapshot = ();
    fn prepare(
        &self,
        request: &[u8],
        inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<i64, ApplyError> {
        self.0.prepare(request, inventory, revision)
    }
    fn result_digest(prepared: &i64) -> [u8; 32] {
        CounterState::result_digest(prepared)
    }
    fn publish(&mut self, prepared: i64) {
        self.0.publish(prepared);
    }
    fn snapshot(&self) {
        panic!("owner diagnostics must not construct a reducer snapshot");
    }
}

#[test]
fn owner_work_ordinary_reports_are_cumulative_read_only_and_never_snapshot() {
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let name = EntryName::new("owner-work").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 6_100_000),
        CounterEntropy(6_200_000),
        NoSnapshot(CounterState::default()),
    )
    .unwrap();
    let initial = coordinator.vault_decrypt_report().unwrap();
    assert_eq!(initial, uste_crypto::VaultDecryptReport::default());
    let nonces = coordinator.vault_nonce_report().unwrap();
    assert_eq!(nonces.issued_nonces, 3); // Creation manifest, certificate header, segment header.
    assert_eq!(
        nonces.issued_nonces + nonces.remaining_nonces,
        nonces.nonce_limit
    );
    let bytes = mutation(0, 7);
    let outcome = coordinator
        .commit(&mut fs, request(1, 11, &bytes), &mut clock(1), &NeverCancel)
        .unwrap();
    let after = coordinator.vault_decrypt_report().unwrap();
    let after_nonce = coordinator.vault_nonce_report().unwrap();
    assert_eq!(after_nonce.issued_nonces, nonces.issued_nonces + 2);
    fs.arm(
        FaultPlan::new([FaultPoint {
            operation: Operation::ReadAt,
            occurrence: 1,
            action: FaultAction::Error(AdapterErrorKind::Io),
        }])
        .unwrap(),
    )
    .unwrap();
    for _ in 0..3 {
        assert_eq!(coordinator.vault_decrypt_report().unwrap(), after);
        assert_eq!(coordinator.vault_nonce_report().unwrap(), after_nonce);
    }
    assert_eq!(fs.pending_faults(), 1);
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    drop(coordinator);
    // Consume the sentinel on a real storage read; restart deliberately retains pending faults.
    assert!(
        JournalStore::open(
            &mut fs,
            &name,
            scope().database(),
            CounterEntropy(6_250_000),
            CounterEntropy(6_260_000),
            &mut TestKeyAdapter,
            |_| Ok(()),
        )
        .is_err()
    );
    assert_eq!(fs.pending_faults(), 0);
    assert_eq!(fs.operation_count(Operation::ReadAt), 1);
    let (journal, _) = JournalStore::open(
        &mut fs,
        &name,
        scope().database(),
        CounterEntropy(6_300_000),
        CounterEntropy(6_400_000),
        &mut TestKeyAdapter,
        |_| Ok(()),
    )
    .unwrap();
    let storage_report = journal.vault_decrypt_report().unwrap();
    let storage_nonce = journal.vault_nonce_report().unwrap();
    assert!(storage_report.successful_calls > 0);
    assert_eq!(storage_nonce.issued_nonces, 0);
    drop(journal);
    let (mut coordinator, _) = CommitCoordinator::open(
        &mut fs,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(6_500_000),
        CounterEntropy(6_600_000),
        &mut TestKeyAdapter,
        NoSnapshot(CounterState::default()),
    )
    .unwrap();
    assert_eq!(coordinator.vault_decrypt_report().unwrap(), storage_report);
    assert_eq!(coordinator.vault_nonce_report().unwrap(), storage_nonce);
    assert_eq!(
        coordinator
            .commit(&mut fs, request(1, 11, &bytes), &mut clock(2), &NeverCancel)
            .unwrap(),
        outcome
    );
    assert_eq!(coordinator.vault_nonce_report().unwrap(), storage_nonce);
}

#[test]
fn owner_work_recovery_reports_include_open_and_cursor_work_without_reset() {
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let name = EntryName::new("recovery-owner-work").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 6_700_000),
        CounterEntropy(6_800_000),
        CounterState::default(),
    )
    .unwrap();
    coordinator
        .commit(
            &mut fs,
            request(1, 11, &mutation(0, 7)),
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();
    drop(coordinator);
    let (journal, _) = JournalStore::open(
        &mut fs,
        &name,
        scope().database(),
        CounterEntropy(6_900_000),
        CounterEntropy(7_000_000),
        &mut TestKeyAdapter,
        |_| Ok(()),
    )
    .unwrap();
    let expected = journal.vault_decrypt_report().unwrap();
    drop(journal);
    let (recovery, _) = AuthenticatedIndexRecovery::open(
        &mut fs,
        &name,
        scope(),
        CounterEntropy(7_100_000),
        CounterEntropy(7_200_000),
        &mut TestKeyAdapter,
    )
    .unwrap();
    assert_eq!(recovery.vault_decrypt_report().unwrap(), expected);
    let nonces = recovery.vault_nonce_report().unwrap();
    assert_eq!(nonces.issued_nonces, 0);
    fs.arm(
        FaultPlan::new([FaultPoint {
            operation: Operation::ReadAt,
            occurrence: 1,
            action: FaultAction::Error(AdapterErrorKind::Io),
        }])
        .unwrap(),
    )
    .unwrap();
    let mut failed = recovery
        .open_transaction_cursor(CommitRevision::FIRST, CommitRevision::FIRST, 1, 32 * 4161)
        .unwrap();
    assert!(
        recovery
            .next_recovered_transaction(&mut fs, &mut failed)
            .is_err()
    );
    assert!(recovery.finish_transaction_cursor(failed).is_err());
    assert_eq!(fs.pending_faults(), 0);
    // An adapter failure before a decrypt is not invented cryptographic work.
    assert_eq!(recovery.vault_decrypt_report().unwrap(), expected);
    assert_eq!(recovery.vault_nonce_report().unwrap(), nonces);
    fs.arm(FaultPlan::default()).unwrap();
    let mut cursor = recovery
        .open_transaction_cursor(CommitRevision::FIRST, CommitRevision::FIRST, 1, 32 * 4161)
        .unwrap();
    assert!(
        recovery
            .next_recovered_transaction(&mut fs, &mut cursor)
            .unwrap()
            .is_some()
    );
    assert!(
        recovery
            .next_recovered_transaction(&mut fs, &mut cursor)
            .unwrap()
            .is_none()
    );
    recovery.finish_transaction_cursor(cursor).unwrap();
    let after = recovery.vault_decrypt_report().unwrap();
    let reads = fs.operation_count(Operation::ReadAt);
    assert!(reads > 0);
    assert_eq!(after.failed_calls, expected.failed_calls);
    assert_eq!(after.successful_calls - expected.successful_calls, reads);
    assert_eq!(
        after.authenticated_encoded_bytes - expected.authenticated_encoded_bytes,
        reads * 4161
    );
    assert!(after.returned_plaintext_bytes > expected.returned_plaintext_bytes);
    assert_eq!(recovery.vault_nonce_report().unwrap(), nonces);
    for _ in 0..3 {
        assert_eq!(recovery.vault_decrypt_report().unwrap(), after);
    }
    assert_eq!(fs.operation_count(Operation::ReadAt), reads);
}
