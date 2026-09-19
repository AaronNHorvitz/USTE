use super::*;
use std::rc::Rc;
use uste_txn::CoordinatorRecoveryLimits;

struct ProbeState {
    value: CounterState,
    preparations: Rc<Cell<usize>>,
    wrong_digest: bool,
}

impl TransactionState for ProbeState {
    type Prepared = i64;
    type Snapshot = CounterState;

    fn prepare(
        &self,
        bytes: &[u8],
        inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<i64, ApplyError> {
        self.preparations.set(self.preparations.get() + 1);
        let value = self.value.prepare(bytes, inventory, revision)?;
        Ok(value + i64::from(self.wrong_digest))
    }

    fn result_digest(value: &i64) -> [u8; 32] {
        CounterState::result_digest(value)
    }
    fn publish(&mut self, value: i64) {
        self.value.publish(value);
    }
    fn snapshot(&self) -> CounterState {
        self.value.clone()
    }
}

#[test]
fn bounded_bootstrap_admission_retry_owner_and_exclusive_handoff() {
    let mut fs = MemoryFileSystem::default();
    let name = EntryName::new("bounded-bootstrap").unwrap();
    let retention = RetentionDays::new(30).unwrap();
    let coordinator = CommitCoordinator::create(
        &mut fs,
        scope(),
        retention,
        name.clone(),
        create_vault(scope().database(), 810),
        CounterEntropy(811),
        CounterState::default(),
    )
    .unwrap();
    drop(coordinator);
    let open = |fs: &mut MemoryFileSystem| {
        AuthenticatedIndexRecovery::open(
            fs,
            &name,
            scope(),
            CounterEntropy(812),
            CounterEntropy(813),
            &mut TestKeyAdapter,
        )
    };
    let (recovery, _) = open(&mut fs).unwrap();
    let mut coordinator = recovery
        .into_bounded_coordinator(
            &mut fs,
            CounterState::default(),
            retention,
            CoordinatorRecoveryLimits::new(0, 0).unwrap(),
            0,
        )
        .unwrap();
    assert!(open(&mut fs).is_err());
    let mut upload = coordinator.start_blob_upload(scope()).unwrap();
    coordinator
        .write_blob_upload(&mut fs, &mut upload, b"bootstrap evidence")
        .unwrap();
    let reference = coordinator
        .finish_blob_upload(&mut fs, &mut upload)
        .unwrap();
    let inventory = BlobInventory::new(scope(), [reference]).unwrap();
    let bytes = mutation(0, 5);
    let transaction = TransactionRequest {
        blob_inventory: Some(&inventory),
        ..request(1, 2, &bytes)
    };
    let outcome = coordinator
        .commit(&mut fs, transaction, &mut clock(1), &NeverCancel)
        .unwrap();
    drop(coordinator);
    fs.restart().unwrap();

    for (outcomes, owners, bytes, wrong, expected_preparations) in [
        (0, 1, 1_000_000, false, 0),
        (1, 0, 1_000_000, false, 0),
        (1, 1, 1, false, 0),
        (1, 1, 1_000_000, true, 1),
    ] {
        let (recovery, _) = open(&mut fs).unwrap();
        assert!(open(&mut fs).is_err());
        let preparations = Rc::new(Cell::new(0));
        let result = recovery.into_bounded_coordinator(
            &mut fs,
            ProbeState {
                value: CounterState::default(),
                preparations: preparations.clone(),
                wrong_digest: wrong,
            },
            retention,
            CoordinatorRecoveryLimits::new(outcomes, owners).unwrap(),
            bytes,
        );
        let expected = if wrong {
            TransactionError::IntegrityFailure
        } else {
            TransactionError::Storage(uste_storage::journal::StorageError::ResourceLimit)
        };
        assert_eq!(result.err(), Some(expected));
        assert_eq!(preparations.get(), expected_preparations);
    }
    let (recovery, _) = open(&mut fs).unwrap();
    let mut coordinator = recovery
        .into_bounded_coordinator(
            &mut fs,
            CounterState::default(),
            retention,
            CoordinatorRecoveryLimits::new(1, 1).unwrap(),
            1_000_000,
        )
        .unwrap();
    assert!(open(&mut fs).is_err());
    assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(5));
    assert_eq!(
        coordinator.committed_blob_owner(reference),
        Some(transaction.principal)
    );
    assert_eq!(
        coordinator
            .commit(&mut fs, transaction, &mut clock(2), &NeverCancel)
            .unwrap(),
        outcome
    );
    let next_bytes = mutation(5, 2);
    coordinator
        .commit(
            &mut fs,
            request(3, 4, &next_bytes),
            &mut clock(2),
            &NeverCancel,
        )
        .unwrap();
    drop(coordinator);
    let (recovery, _) = open(&mut fs).unwrap();
    let preparations = Rc::new(Cell::new(0));
    assert!(
        recovery
            .into_bounded_coordinator(
                &mut fs,
                ProbeState {
                    value: CounterState::default(),
                    preparations: preparations.clone(),
                    wrong_digest: false,
                },
                retention,
                CoordinatorRecoveryLimits::new(1, 1).unwrap(),
                1_000_000
            )
            .is_err()
    );
    assert_eq!(preparations.get(), 0);
    assert!(open(&mut fs).is_ok());
}

#[test]
fn bounded_bootstrap_reauthenticates_every_read_before_returning_a_coordinator() {
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let name = EntryName::new("bounded-bootstrap-fault").unwrap();
    let retention = RetentionDays::new(30).unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut fs,
        scope(),
        retention,
        name.clone(),
        create_vault(scope().database(), 820),
        CounterEntropy(821),
        CounterState::default(),
    )
    .unwrap();
    coordinator
        .commit(
            &mut fs,
            request(1, 2, &mutation(0, 5)),
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();
    coordinator
        .commit(
            &mut fs,
            request(3, 4, &mutation(5, 2)),
            &mut clock(2),
            &NeverCancel,
        )
        .unwrap();
    drop(coordinator);
    let open = |fs: &mut FaultFileSystem<MemoryFileSystem>| {
        AuthenticatedIndexRecovery::open(
            fs,
            &name,
            scope(),
            CounterEntropy(822),
            CounterEntropy(823),
            &mut TestKeyAdapter,
        )
    };
    let limits = CoordinatorRecoveryLimits::new(2, 0).unwrap();
    let (recovery, _) = open(&mut fs).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    let coordinator = recovery
        .into_bounded_coordinator(
            &mut fs,
            CounterState::default(),
            retention,
            limits,
            1_000_000,
        )
        .unwrap();
    let reads = fs.operation_count(Operation::ReadAt);
    assert!(reads > 0);
    drop(coordinator);
    for occurrence in 1..=reads {
        let (recovery, _) = open(&mut fs).unwrap();
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
                .into_bounded_coordinator(
                    &mut fs,
                    CounterState::default(),
                    retention,
                    limits,
                    1_000_000
                )
                .is_err()
        );
        assert_eq!(fs.pending_faults(), 0);
    }
    let (recovery, _) = open(&mut fs).unwrap();
    let coordinator = recovery
        .into_bounded_coordinator(
            &mut fs,
            CounterState::default(),
            retention,
            limits,
            1_000_000,
        )
        .unwrap();
    assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(7));
    drop(coordinator);
    let (recovery, _) = open(&mut fs).unwrap();
    // Corruption after initial authentication cannot be trusted through the ownership handoff.
    let directory = fs.open_directory(&fs.root(), &name).unwrap();
    let certificates = fs
        .open_existing(&directory, &EntryName::new("CERTIFICATES").unwrap())
        .unwrap();
    let mut byte = [0];
    // Format 1.0 has a 4,161-byte log header before revision one's certificate.
    let offset = 4_161 + 127;
    assert_eq!(fs.read_at(&certificates, offset, &mut byte).unwrap(), 1);
    byte[0] ^= 1;
    assert_eq!(fs.write_at(&certificates, offset, &byte).unwrap(), 1);
    assert_eq!(
        recovery
            .into_bounded_coordinator(
                &mut fs,
                CounterState::default(),
                retention,
                limits,
                1_000_000
            )
            .err(),
        Some(TransactionError::IntegrityFailure)
    );
}
