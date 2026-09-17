use sha2::{Digest, Sha256};
use std::cell::Cell;
use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
use uste_storage::{
    AdapterErrorKind, BLOB_CHUNK_BYTES, BlobInventory, ClockObservation, EntryName, FileSystem,
    fault::{FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation, ScriptedClock},
    journal::{CommitInput, CreationOptions, DurableKeyEnvelope, JournalStore},
    memory::MemoryFileSystem,
};
use uste_txn::{
    ApplyError, Cancellation, CommitCoordinator, NeverCancel, PrincipalDigest, RetentionDays,
    TransactionError, TransactionRequest, TransactionState,
};
use uste_types::{
    DatabaseId, IdempotencyKey, NamespaceId, NamespaceRef, TransactionId, UtcInstant,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct CounterState(i64);

impl TransactionState for CounterState {
    type Prepared = i64;
    type Snapshot = CounterState;

    fn prepare(
        &self,
        canonical_request: &[u8],
        _blob_inventory: Option<&BlobInventory>,
        _revision: uste_types::CommitRevision,
    ) -> Result<Self::Prepared, ApplyError> {
        if canonical_request.len() != 16 {
            return Err(ApplyError::InvalidRequest);
        }
        let expected = i64::from_be_bytes(canonical_request[..8].try_into().unwrap());
        let delta = i64::from_be_bytes(canonical_request[8..].try_into().unwrap());
        if self.0 != expected {
            return Err(ApplyError::Conflict);
        }
        self.0.checked_add(delta).ok_or(ApplyError::ResourceLimit)
    }

    fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
        let mut digest = [0_u8; 32];
        digest[..8].copy_from_slice(&prepared.to_be_bytes());
        digest
    }

    fn publish(&mut self, prepared: Self::Prepared) {
        self.0 = prepared;
    }

    fn snapshot(&self) -> Self::Snapshot {
        self.clone()
    }
}

fn mutation(expected: i64, delta: i64) -> [u8; 16] {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&expected.to_be_bytes());
    bytes[8..].copy_from_slice(&delta.to_be_bytes());
    bytes
}

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([1; 16]),
        NamespaceId::from_bytes([2; 16]),
    )
}

fn instant(days: i64) -> UtcInstant {
    UtcInstant::new(days * 86_400, 123).unwrap()
}

fn request<'a>(key: u8, transaction: u8, bytes: &'a [u8]) -> TransactionRequest<'a> {
    TransactionRequest {
        principal: PrincipalDigest::from_bytes([3; 32]),
        idempotency_key: IdempotencyKey::from_bytes([key; 16]),
        transaction_id: TransactionId::from_bytes([transaction; 16]),
        canonical_request: bytes,
        blob_inventory: None,
    }
}

fn clock(day: i64) -> ScriptedClock {
    ScriptedClock::new([Ok(ClockObservation {
        wall_utc: instant(day),
        monotonic_ticks: u64::try_from(day).unwrap_or_default(),
    })])
}

#[test]
fn equal_and_rolling_back_wall_samples_never_order_or_merge_commits() {
    let scope = scope();
    let mut filesystem = MemoryFileSystem::default();
    let name = EntryName::new("wall-ordering").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope,
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope.database(), 9),
        CounterEntropy(90),
        CounterState::default(),
    )
    .unwrap();
    for (index, wall_day) in [10_i64, 10, -10].into_iter().enumerate() {
        let expected = i64::try_from(index).unwrap();
        let bytes = mutation(expected, 1);
        let outcome = coordinator
            .commit(
                &mut filesystem,
                request(
                    u8::try_from(index + 1).unwrap(),
                    u8::try_from(index + 11).unwrap(),
                    &bytes,
                ),
                &mut clock(wall_day),
                &NeverCancel,
            )
            .unwrap();
        assert_eq!(outcome.revision.get(), u64::try_from(index + 1).unwrap());
    }
    assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(3));
    drop(coordinator);
    filesystem.restart().unwrap();
    let (coordinator, report) = CommitCoordinator::open(
        &mut filesystem,
        &name,
        scope,
        RetentionDays::new(30).unwrap(),
        CounterEntropy(91),
        CounterEntropy(92),
        &mut TestKeyAdapter,
        CounterState::default(),
    )
    .unwrap();
    assert_eq!(report.frontier.unwrap().get(), 3);
    assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(3));
}

#[test]
fn retry_conflict_cancellation_reader_and_restart_are_coherent() {
    let scope = scope();
    let mut filesystem = MemoryFileSystem::default();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope,
        RetentionDays::new(30).unwrap(),
        EntryName::new("database").unwrap(),
        create_vault(scope.database(), 10),
        CounterEntropy(100),
        CounterState::default(),
    )
    .unwrap();
    assert_eq!(coordinator.read_view().unwrap().revision(), None);
    assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(0));

    let bytes = mutation(0, 5);
    let mut failed_clock = ScriptedClock::new([Err(AdapterErrorKind::Io)]);
    assert_eq!(
        coordinator
            .commit(
                &mut filesystem,
                request(1, 2, &bytes),
                &mut failed_clock,
                &NeverCancel,
            )
            .unwrap_err(),
        TransactionError::RetryableUnavailable
    );
    assert_eq!(coordinator.read_view().unwrap().revision(), None);
    let first = coordinator
        .commit(
            &mut filesystem,
            request(4, 5, &bytes),
            &mut clock(0),
            &NeverCancel,
        )
        .unwrap();
    assert_eq!(first.revision.get(), 1);
    assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(5));
    let pinned_first = coordinator.read_view().unwrap();
    assert_eq!(
        coordinator
            .commit(
                &mut filesystem,
                request(4, 5, &bytes),
                &mut clock(1),
                &NeverCancel,
            )
            .unwrap(),
        first
    );
    assert_eq!(
        coordinator.read_view().unwrap().revision().unwrap().get(),
        1
    );

    let changed = mutation(5, 1);
    assert_eq!(
        coordinator
            .commit(
                &mut filesystem,
                request(4, 5, &changed),
                &mut clock(1),
                &NeverCancel,
            )
            .unwrap_err(),
        TransactionError::Conflict
    );
    let stale = mutation(0, 9);
    assert_eq!(
        coordinator
            .commit(
                &mut filesystem,
                request(6, 7, &stale),
                &mut clock(1),
                &NeverCancel,
            )
            .unwrap_err(),
        TransactionError::Conflict
    );
    assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(5));
    assert_eq!(
        coordinator
            .commit(
                &mut filesystem,
                request(8, 9, &changed),
                &mut clock(1),
                &AlwaysCancel,
            )
            .unwrap_err(),
        TransactionError::Cancelled
    );
    let second = coordinator
        .commit(
            &mut filesystem,
            request(13, 14, &changed),
            &mut clock(-1),
            &NeverCancel,
        )
        .unwrap();
    assert_eq!(second.revision.get(), 2);
    assert_eq!(pinned_first.revision().unwrap().get(), 1);
    assert_eq!(pinned_first.state(), &CounterState(5));
    assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(6));
    drop(coordinator);
    filesystem.restart().unwrap();

    let (mut coordinator, report) = CommitCoordinator::open(
        &mut filesystem,
        &EntryName::new("database").unwrap(),
        scope,
        RetentionDays::new(31).unwrap(),
        CounterEntropy(20),
        CounterEntropy(200),
        &mut TestKeyAdapter,
        CounterState::default(),
    )
    .unwrap();
    assert_eq!(report.frontier.unwrap().get(), 2);
    assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(6));
    assert_eq!(
        coordinator
            .outcome(
                PrincipalDigest::from_bytes([3; 32]),
                IdempotencyKey::from_bytes([4; 16]),
                &mut clock(29)
            )
            .unwrap(),
        Some(first)
    );
    assert_eq!(
        coordinator
            .transaction_outcome(TransactionId::from_bytes([5; 16]), &mut clock(29))
            .unwrap(),
        Some(first)
    );
    assert_eq!(
        coordinator
            .outcome(
                PrincipalDigest::from_bytes([3; 32]),
                IdempotencyKey::from_bytes([4; 16]),
                &mut clock(30)
            )
            .unwrap_err(),
        TransactionError::IdempotencyExpired
    );
    assert_eq!(
        coordinator
            .commit(
                &mut filesystem,
                request(10, 5, &changed),
                &mut clock(2),
                &NeverCancel,
            )
            .unwrap_err(),
        TransactionError::Conflict
    );
}

#[test]
fn lost_response_is_outcome_unknown_then_durable_retry_after_restart() {
    let scope = scope();
    let plan = FaultPlan::new([FaultPoint {
        operation: Operation::SyncData,
        occurrence: 2,
        action: FaultAction::CrashAfter,
    }])
    .unwrap();
    let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope,
        RetentionDays::new(30).unwrap(),
        EntryName::new("lost-response").unwrap(),
        create_vault(scope.database(), 30),
        CounterEntropy(300),
        CounterState::default(),
    )
    .unwrap();
    let bytes = mutation(0, 7);
    let blob_bytes = b"lost response keeps the exact committed blob inventory";
    let mut upload = coordinator.start_blob_upload(scope).unwrap();
    coordinator
        .write_blob_upload(&mut filesystem, &mut upload, blob_bytes)
        .unwrap();
    let reference = coordinator
        .finish_blob_upload(&mut filesystem, &mut upload)
        .unwrap();
    let inventory = BlobInventory::new(scope, [reference]).unwrap();
    assert_eq!(
        coordinator
            .commit(
                &mut filesystem,
                TransactionRequest {
                    blob_inventory: Some(&inventory),
                    ..request(11, 12, &bytes)
                },
                &mut clock(0),
                &NeverCancel,
            )
            .unwrap_err(),
        TransactionError::OutcomeUnknown
    );
    assert_eq!(
        coordinator.read_view().unwrap_err(),
        TransactionError::OutcomeUnknown
    );
    assert_eq!(
        coordinator
            .outcome(
                PrincipalDigest::from_bytes([3; 32]),
                IdempotencyKey::from_bytes([11; 16]),
                &mut clock(1)
            )
            .unwrap_err(),
        TransactionError::OutcomeUnknown
    );
    drop(coordinator);
    filesystem.restart().unwrap();
    let (mut coordinator, report) = CommitCoordinator::open(
        &mut filesystem,
        &EntryName::new("lost-response").unwrap(),
        scope,
        RetentionDays::new(30).unwrap(),
        CounterEntropy(40),
        CounterEntropy(400),
        &mut TestKeyAdapter,
        CounterState::default(),
    )
    .unwrap();
    assert_eq!(report.frontier.unwrap().get(), 1);
    assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(7));
    assert_blob_equals(&coordinator, &mut filesystem, reference, blob_bytes);
    let retry = coordinator
        .commit(
            &mut filesystem,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(11, 12, &bytes)
            },
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();
    assert_eq!(retry.revision.get(), 1);
}

#[test]
fn every_initial_publication_fault_preserves_atomic_visibility() {
    let boundaries = [
        (Operation::WriteAt, 5_u64),
        (Operation::SyncData, 1),
        (Operation::WriteAt, 6),
        (Operation::SyncData, 2),
    ];
    let actions = [
        FaultAction::Error(AdapterErrorKind::Io),
        FaultAction::CrashBefore,
        FaultAction::CrashAfter,
    ];
    for (case, (operation, occurrence, action)) in boundaries
        .into_iter()
        .flat_map(|(operation, occurrence)| {
            actions
                .into_iter()
                .map(move |action| (operation, occurrence, action))
        })
        .enumerate()
    {
        let plan = FaultPlan::new([FaultPoint {
            operation,
            occurrence,
            action,
        }])
        .unwrap();
        let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
        let mut coordinator = CommitCoordinator::create(
            &mut filesystem,
            scope(),
            RetentionDays::new(30).unwrap(),
            EntryName::new("fault-matrix").unwrap(),
            create_vault(scope().database(), 1_000 + u64::try_from(case).unwrap()),
            CounterEntropy(2_000 + u64::try_from(case).unwrap()),
            CounterState::default(),
        )
        .unwrap();
        let bytes = mutation(0, 5);
        assert_eq!(
            coordinator
                .commit(
                    &mut filesystem,
                    request(1, 2, &bytes),
                    &mut clock(0),
                    &NeverCancel,
                )
                .unwrap_err(),
            TransactionError::OutcomeUnknown,
            "operation={operation:?} occurrence={occurrence} action={action:?}"
        );
        assert_eq!(
            coordinator.read_view().unwrap_err(),
            TransactionError::OutcomeUnknown
        );
        drop(coordinator);
        filesystem.restart().unwrap();

        let (coordinator, report) = CommitCoordinator::open(
            &mut filesystem,
            &EntryName::new("fault-matrix").unwrap(),
            scope(),
            RetentionDays::new(30).unwrap(),
            CounterEntropy(3_000),
            CounterEntropy(4_000),
            &mut TestKeyAdapter,
            CounterState::default(),
        )
        .unwrap();
        let committed = operation == Operation::SyncData
            && occurrence == 2
            && action == FaultAction::CrashAfter;
        assert_eq!(report.frontier.is_some(), committed);
        assert_eq!(
            coordinator.read_view().unwrap().state().0,
            if committed { 5 } else { 0 }
        );
    }
}

#[test]
fn exact_short_writes_and_both_cancellation_boundaries_are_safe() {
    for occurrence in [5_u64, 6] {
        let plan = FaultPlan::new([FaultPoint {
            operation: Operation::WriteAt,
            occurrence,
            action: FaultAction::ShortWrite { maximum: 1 },
        }])
        .unwrap();
        let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
        let mut coordinator = CommitCoordinator::create(
            &mut filesystem,
            scope(),
            RetentionDays::new(30).unwrap(),
            EntryName::new("short-write").unwrap(),
            create_vault(scope().database(), 5_000 + occurrence),
            CounterEntropy(6_000 + occurrence),
            CounterState::default(),
        )
        .unwrap();
        coordinator
            .commit(
                &mut filesystem,
                request(1, 2, &mutation(0, 5)),
                &mut clock(0),
                &NeverCancel,
            )
            .unwrap();
        assert_eq!(filesystem.pending_faults(), 0);
        assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(5));
    }

    for cancel_on_call in [1_u8, 2] {
        let mut filesystem = MemoryFileSystem::default();
        let mut coordinator = CommitCoordinator::create(
            &mut filesystem,
            scope(),
            RetentionDays::new(30).unwrap(),
            EntryName::new("cancel-boundary").unwrap(),
            create_vault(scope().database(), 7_000 + u64::from(cancel_on_call)),
            CounterEntropy(8_000 + u64::from(cancel_on_call)),
            CounterState::default(),
        )
        .unwrap();
        assert_eq!(
            coordinator
                .commit(
                    &mut filesystem,
                    request(1, 2, &mutation(0, 5)),
                    &mut clock(0),
                    &CancelOnCall::new(cancel_on_call),
                )
                .unwrap_err(),
            TransactionError::Cancelled
        );
        assert_eq!(coordinator.read_view().unwrap().revision(), None);
        assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(0));
    }
}

#[test]
fn authenticated_but_malformed_groups_are_rejected_during_recovery() {
    let valid = decode_hex(include_str!("../../../acceptance/r1/txn-group-v1.hex"));
    for (case, mut malformed) in [valid.clone(), valid].into_iter().enumerate() {
        if case == 0 {
            malformed[184] = 1;
        } else {
            malformed[152] ^= 1;
        }
        let mut filesystem = MemoryFileSystem::default();
        let mut store = JournalStore::create(
            &mut filesystem,
            CreationOptions {
                database: scope().database(),
                final_name: EntryName::new("malformed").unwrap(),
            },
            create_vault(scope().database(), 9_000 + u64::try_from(case).unwrap()),
            CounterEntropy(10_000 + u64::try_from(case).unwrap()),
        )
        .unwrap();
        store
            .append_group(
                &mut filesystem,
                CommitInput {
                    encoded_group: &malformed,
                    logical_event_digest: Sha256::digest(&malformed).into(),
                },
            )
            .unwrap();
        drop(store);
        filesystem.restart().unwrap();
        let result = CommitCoordinator::open(
            &mut filesystem,
            &EntryName::new("malformed").unwrap(),
            scope(),
            RetentionDays::new(30).unwrap(),
            CounterEntropy(11_000),
            CounterEntropy(12_000),
            &mut TestKeyAdapter,
            CounterState::default(),
        );
        assert!(matches!(result, Err(TransactionError::IntegrityFailure)));
    }
}

#[test]
fn arbitrary_blob_round_trip_is_commit_gated_and_survives_restart() {
    let mut filesystem = MemoryFileSystem::default();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        EntryName::new("blob-round-trip").unwrap(),
        create_vault(scope().database(), 17_000),
        CounterEntropy(18_000),
        CounterState::default(),
    )
    .unwrap();
    let empty_inventory = BlobInventory::new(scope(), []).unwrap();
    assert_eq!(
        coordinator
            .commit(
                &mut filesystem,
                TransactionRequest {
                    blob_inventory: Some(&empty_inventory),
                    ..request(19, 20, &mutation(0, 1))
                },
                &mut clock(0),
                &NeverCancel,
            )
            .unwrap_err(),
        TransactionError::InvalidRequest
    );
    let bytes: Vec<u8> = (0..(BLOB_CHUNK_BYTES * 2 + 37))
        .map(|index| u8::try_from(index % 251).unwrap())
        .collect();
    let mut upload = coordinator.start_blob_upload(scope()).unwrap();
    for part in bytes.chunks(333_333) {
        coordinator
            .write_blob_upload(&mut filesystem, &mut upload, part)
            .unwrap();
    }
    let reference = coordinator
        .finish_blob_upload(&mut filesystem, &mut upload)
        .unwrap();
    assert_eq!(reference.byte_len(), u64::try_from(bytes.len()).unwrap());
    assert_eq!(reference.chunk_count(), 3);
    assert!(matches!(
        coordinator.read_blob_range(&mut filesystem, reference, 0, &mut [0_u8; 1]),
        Err(TransactionError::Storage(
            uste_storage::journal::StorageError::InvalidState
        ))
    ));
    let inventory = BlobInventory::new(scope(), [reference]).unwrap();
    let initial_mutation = mutation(0, 5);
    let transaction = TransactionRequest {
        blob_inventory: Some(&inventory),
        ..request(21, 22, &initial_mutation)
    };
    let committed = coordinator
        .commit(&mut filesystem, transaction, &mut clock(0), &NeverCancel)
        .unwrap();
    assert_blob_equals(&coordinator, &mut filesystem, reference, &bytes);
    let cross_offset = u64::try_from(BLOB_CHUNK_BYTES - 9).unwrap();
    let mut cross = [0_u8; 37];
    assert_eq!(
        coordinator
            .read_blob_range(&mut filesystem, reference, cross_offset, &mut cross)
            .unwrap(),
        cross.len()
    );
    assert_eq!(
        cross.as_slice(),
        &bytes[BLOB_CHUNK_BYTES - 9..BLOB_CHUNK_BYTES - 9 + cross.len()]
    );
    assert_eq!(
        coordinator
            .read_blob_range(&mut filesystem, reference, reference.byte_len(), &mut [],)
            .unwrap(),
        0
    );
    assert!(matches!(
        coordinator.read_blob_range(
            &mut filesystem,
            reference,
            reference.byte_len() + 1,
            &mut [],
        ),
        Err(TransactionError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
    assert!(matches!(
        coordinator.read_blob_range(
            &mut filesystem,
            reference,
            0,
            &mut vec![0_u8; BLOB_CHUNK_BYTES + 1],
        ),
        Err(TransactionError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
    drop(coordinator);
    filesystem.restart().unwrap();

    let (mut coordinator, report) = CommitCoordinator::open(
        &mut filesystem,
        &EntryName::new("blob-round-trip").unwrap(),
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(19_000),
        CounterEntropy(20_000),
        &mut TestKeyAdapter,
        CounterState::default(),
    )
    .unwrap();
    assert_eq!(report.frontier, Some(committed.revision));
    assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(5));
    assert_blob_equals(&coordinator, &mut filesystem, reference, &bytes);
    let retry = coordinator
        .commit(
            &mut filesystem,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(21, 22, &initial_mutation)
            },
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();
    assert_eq!(retry, committed);

    let mut other_upload = coordinator.start_blob_upload(scope()).unwrap();
    let other_reference = coordinator
        .finish_blob_upload(&mut filesystem, &mut other_upload)
        .unwrap();
    let changed_inventory = BlobInventory::new(scope(), [other_reference]).unwrap();
    assert_eq!(
        coordinator
            .commit(
                &mut filesystem,
                TransactionRequest {
                    blob_inventory: Some(&changed_inventory),
                    ..request(21, 22, &initial_mutation)
                },
                &mut clock(1),
                &NeverCancel,
            )
            .unwrap_err(),
        TransactionError::Conflict
    );

    coordinator
        .commit(
            &mut filesystem,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(27, 28, &mutation(5, 1))
            },
            &mut clock(2),
            &NeverCancel,
        )
        .unwrap();
    drop(coordinator);
    filesystem.restart().unwrap();
    let (coordinator, report) = CommitCoordinator::open(
        &mut filesystem,
        &EntryName::new("blob-round-trip").unwrap(),
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(29_000),
        CounterEntropy(30_000),
        &mut TestKeyAdapter,
        CounterState::default(),
    )
    .unwrap();
    assert_eq!(report.frontier.map(|revision| revision.get()), Some(2));
    assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(6));
}

#[test]
fn upload_resume_abort_and_zero_byte_content_are_explicit() {
    let mut filesystem = MemoryFileSystem::default();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        EntryName::new("blob-resume").unwrap(),
        create_vault(scope().database(), 21_000),
        CounterEntropy(22_000),
        CounterState::default(),
    )
    .unwrap();
    let full = vec![0x5a; BLOB_CHUNK_BYTES];
    let tail = b"resumed tail";
    let mut upload = coordinator.start_blob_upload(scope()).unwrap();
    coordinator
        .write_blob_upload(&mut filesystem, &mut upload, &full)
        .unwrap();
    coordinator
        .write_blob_upload(&mut filesystem, &mut upload, tail)
        .unwrap();
    let token = upload.token();
    assert_eq!(
        upload.durable_bytes(),
        u64::try_from(BLOB_CHUNK_BYTES).unwrap()
    );
    drop(upload);
    drop(coordinator);
    filesystem.restart().unwrap();

    let (mut coordinator, _) = CommitCoordinator::open(
        &mut filesystem,
        &EntryName::new("blob-resume").unwrap(),
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(23_000),
        CounterEntropy(24_000),
        &mut TestKeyAdapter,
        CounterState::default(),
    )
    .unwrap();
    let mut upload = coordinator
        .resume_blob_upload(&mut filesystem, token)
        .unwrap();
    assert_eq!(
        upload.durable_bytes(),
        u64::try_from(BLOB_CHUNK_BYTES).unwrap()
    );
    coordinator
        .write_blob_upload(&mut filesystem, &mut upload, tail)
        .unwrap();
    let resumed_reference = coordinator
        .finish_blob_upload(&mut filesystem, &mut upload)
        .unwrap();
    let mut expected = full;
    expected.extend_from_slice(tail);

    let mut aborted = coordinator.start_blob_upload(scope()).unwrap();
    coordinator
        .write_blob_upload(&mut filesystem, &mut aborted, &vec![7; BLOB_CHUNK_BYTES])
        .unwrap();
    let aborted_token = aborted.token();
    coordinator
        .abort_blob_upload(&mut filesystem, &mut aborted)
        .unwrap();
    assert!(matches!(
        coordinator.write_blob_upload(&mut filesystem, &mut aborted, b"resurrect"),
        Err(TransactionError::Storage(
            uste_storage::journal::StorageError::InvalidState
        ))
    ));
    assert!(matches!(
        coordinator.resume_blob_upload(&mut filesystem, aborted_token),
        Err(TransactionError::Storage(
            uste_storage::journal::StorageError::InvalidState
        ))
    ));

    let mut zero_upload = coordinator.start_blob_upload(scope()).unwrap();
    let zero_token = zero_upload.token();
    let zero_reference = coordinator
        .finish_blob_upload(&mut filesystem, &mut zero_upload)
        .unwrap();
    assert_eq!(
        coordinator
            .finish_blob_upload(&mut filesystem, &mut zero_upload)
            .unwrap(),
        zero_reference
    );
    let mut resumed_zero = coordinator
        .resume_blob_upload(&mut filesystem, zero_token)
        .unwrap();
    assert!(matches!(
        coordinator.write_blob_upload(&mut filesystem, &mut resumed_zero, b"not empty"),
        Err(TransactionError::Storage(
            uste_storage::journal::StorageError::InvalidState
        ))
    ));
    assert_eq!(zero_reference.byte_len(), 0);
    assert_eq!(zero_reference.chunk_count(), 0);
    let inventory = BlobInventory::new(scope(), [resumed_reference, zero_reference]).unwrap();
    let mutation = mutation(0, 1);
    coordinator
        .commit(
            &mut filesystem,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(23, 24, &mutation)
            },
            &mut clock(0),
            &NeverCancel,
        )
        .unwrap();
    assert_blob_equals(&coordinator, &mut filesystem, resumed_reference, &expected);
    assert_eq!(
        coordinator
            .read_blob_range(&mut filesystem, zero_reference, 0, &mut [])
            .unwrap(),
        0
    );
}

#[test]
fn missing_blob_named_by_committed_inventory_fails_recovery() {
    let mut filesystem = MemoryFileSystem::default();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        EntryName::new("missing-blob").unwrap(),
        create_vault(scope().database(), 25_000),
        CounterEntropy(26_000),
        CounterState::default(),
    )
    .unwrap();
    let mut upload = coordinator.start_blob_upload(scope()).unwrap();
    coordinator
        .write_blob_upload(&mut filesystem, &mut upload, b"committed content")
        .unwrap();
    let reference = coordinator
        .finish_blob_upload(&mut filesystem, &mut upload)
        .unwrap();
    let inventory = BlobInventory::new(scope(), [reference]).unwrap();
    coordinator
        .commit(
            &mut filesystem,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(25, 26, &mutation(0, 1))
            },
            &mut clock(0),
            &NeverCancel,
        )
        .unwrap();
    drop(coordinator);
    filesystem.restart().unwrap();
    let root = filesystem.root();
    let directory = filesystem
        .open_directory(&root, &EntryName::new("missing-blob").unwrap())
        .unwrap();
    let name = EntryName::new(format!("b-{}-00000000", hex(reference.id().as_bytes()))).unwrap();
    filesystem.remove_file(&directory, &name).unwrap();
    filesystem.sync_directory(&directory).unwrap();
    filesystem.restart().unwrap();
    let result = CommitCoordinator::open(
        &mut filesystem,
        &EntryName::new("missing-blob").unwrap(),
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(27_000),
        CounterEntropy(28_000),
        &mut TestKeyAdapter,
        CounterState::default(),
    );
    assert!(matches!(result, Err(TransactionError::IntegrityFailure)));
}

#[test]
fn finalized_and_aborted_upload_markers_survive_restart() {
    let mut filesystem = MemoryFileSystem::default();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        EntryName::new("blob-markers").unwrap(),
        create_vault(scope().database(), 29_000),
        CounterEntropy(30_000),
        CounterState::default(),
    )
    .unwrap();
    let mut finalized = coordinator.start_blob_upload(scope()).unwrap();
    let finalized_token = finalized.token();
    let finalized_reference = coordinator
        .finish_blob_upload(&mut filesystem, &mut finalized)
        .unwrap();
    let mut aborted = coordinator.start_blob_upload(scope()).unwrap();
    let aborted_token = aborted.token();
    coordinator
        .abort_blob_upload(&mut filesystem, &mut aborted)
        .unwrap();
    drop(coordinator);
    filesystem.restart().unwrap();

    let (mut coordinator, _) = CommitCoordinator::open(
        &mut filesystem,
        &EntryName::new("blob-markers").unwrap(),
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(31_000),
        CounterEntropy(32_000),
        &mut TestKeyAdapter,
        CounterState::default(),
    )
    .unwrap();
    let mut resumed_final = coordinator
        .resume_blob_upload(&mut filesystem, finalized_token)
        .unwrap();
    assert!(matches!(
        coordinator.write_blob_upload(&mut filesystem, &mut resumed_final, b"mutation"),
        Err(TransactionError::Storage(
            uste_storage::journal::StorageError::InvalidState
        ))
    ));
    assert_eq!(
        coordinator
            .finish_blob_upload(&mut filesystem, &mut resumed_final)
            .unwrap(),
        finalized_reference
    );
    assert!(matches!(
        coordinator.resume_blob_upload(&mut filesystem, aborted_token),
        Err(TransactionError::Storage(
            uste_storage::journal::StorageError::InvalidState
        ))
    ));
}

fn assert_blob_equals<F>(
    coordinator: &CommitCoordinator<CounterState, F, TestEnvelope, CounterEntropy, CounterEntropy>,
    filesystem: &mut F,
    reference: uste_storage::BlobReference,
    expected: &[u8],
) where
    F: uste_storage::OwnershipFileSystem,
{
    let mut actual = Vec::new();
    let mut offset = 0_u64;
    let mut buffer = vec![0_u8; 700_001];
    loop {
        let count = coordinator
            .read_blob_range(filesystem, reference, offset, &mut buffer)
            .unwrap();
        if count == 0 {
            break;
        }
        actual.extend_from_slice(&buffer[..count]);
        offset += u64::try_from(count).unwrap();
    }
    assert_eq!(actual, expected);
}

fn hex(bytes: [u8; 16]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(text: &str) -> Vec<u8> {
    text.trim()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

struct AlwaysCancel;

impl Cancellation for AlwaysCancel {
    fn is_cancelled(&self) -> bool {
        true
    }
}

struct CancelOnCall {
    calls: Cell<u8>,
    cancel_on: u8,
}

impl CancelOnCall {
    const fn new(cancel_on: u8) -> Self {
        Self {
            calls: Cell::new(0),
            cancel_on,
        }
    }
}

impl Cancellation for CancelOnCall {
    fn is_cancelled(&self) -> bool {
        let calls = self.calls.get().saturating_add(1);
        self.calls.set(calls);
        calls == self.cancel_on
    }
}

#[derive(Debug)]
struct TestEnvelope([u8; 32]);

impl DurableKeyEnvelope for TestEnvelope {
    fn encode_durable(&self) -> Result<Vec<u8>, CryptoError> {
        Ok(self.0.to_vec())
    }

    fn decode_durable(encoded: &[u8]) -> Result<Self, CryptoError> {
        Ok(Self(
            encoded
                .try_into()
                .map_err(|_| CryptoError::InvalidEnvelope)?,
        ))
    }
}

struct TestKeyAdapter;

impl KeyAdapter for TestKeyAdapter {
    type Envelope = TestEnvelope;

    fn wrap(
        &mut self,
        _database: DatabaseId,
        key: &SecretKeyMaterial,
        _entropy: &mut dyn EntropySource,
    ) -> Result<Self::Envelope, CryptoError> {
        Ok(TestEnvelope(*key.expose_to_adapter()))
    }

    fn unwrap(
        &mut self,
        _database: DatabaseId,
        envelope: &Self::Envelope,
    ) -> Result<SecretKeyMaterial, CryptoError> {
        Ok(SecretKeyMaterial::from_adapter_bytes(envelope.0))
    }
}

#[derive(Debug)]
struct CounterEntropy(u64);

impl EntropySource for CounterEntropy {
    fn fill(&mut self, output: &mut [u8]) -> Result<(), EntropyFailure> {
        self.0 = self.0.checked_add(1).ok_or(EntropyFailure)?;
        for (index, chunk) in output.chunks_mut(8).enumerate() {
            let value = self
                .0
                .checked_add(u64::try_from(index).map_err(|_| EntropyFailure)?)
                .ok_or(EntropyFailure)?;
            chunk.copy_from_slice(&value.to_be_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}

fn create_vault(database: DatabaseId, seed: u64) -> KeyVault<TestEnvelope, CounterEntropy> {
    KeyVault::create(database, &mut TestKeyAdapter, CounterEntropy(seed)).unwrap()
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod concurrent_linux {
    use super::*;
    use std::{
        fs::{self, File},
        path::PathBuf,
        sync::{
            Arc, Barrier, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::{SystemTime, UNIX_EPOCH},
    };
    use uste_storage::linux::{LinuxFileSystem, LinuxFilesystemProfile};

    const TEST_ROOT: &str = "USTE_T13_TEST_ROOT";
    const TEST_PROFILE: &str = "USTE_T13_TEST_PROFILE";

    struct Harness {
        filesystem: LinuxFileSystem,
        coordinator: CommitCoordinator<
            CounterState,
            LinuxFileSystem,
            TestEnvelope,
            CounterEntropy,
            CounterEntropy,
        >,
    }

    #[test]
    fn competing_callers_publish_exactly_one_stale_mutation_and_recover_it() {
        const CALLERS: usize = 32;
        let directory = test_directory();
        let mut filesystem = open_filesystem(&directory);
        let coordinator = CommitCoordinator::create(
            &mut filesystem,
            scope(),
            RetentionDays::new(30).unwrap(),
            EntryName::new("concurrent").unwrap(),
            create_vault(scope().database(), 13_000),
            CounterEntropy(14_000),
            CounterState::default(),
        )
        .unwrap();
        let harness = Mutex::new(Harness {
            filesystem,
            coordinator,
        });
        let barrier = Arc::new(Barrier::new(CALLERS));
        let committed = AtomicUsize::new(0);
        let conflicted = AtomicUsize::new(0);

        std::thread::scope(|scope_threads| {
            for caller in 0..CALLERS {
                let barrier = Arc::clone(&barrier);
                let harness = &harness;
                let committed = &committed;
                let conflicted = &conflicted;
                scope_threads.spawn(move || {
                    let bytes = mutation(0, 1);
                    barrier.wait();
                    let mut guard = harness.lock().unwrap();
                    let Harness {
                        filesystem,
                        coordinator,
                    } = &mut *guard;
                    let result = coordinator.commit(
                        filesystem,
                        request(
                            u8::try_from(caller + 1).unwrap(),
                            u8::try_from(caller + 65).unwrap(),
                            &bytes,
                        ),
                        &mut clock(0),
                        &NeverCancel,
                    );
                    match result {
                        Ok(_) => {
                            committed.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(TransactionError::Conflict) => {
                            conflicted.fetch_add(1, Ordering::Relaxed);
                        }
                        other => panic!("unexpected concurrent outcome: {other:?}"),
                    }
                });
            }
        });
        assert_eq!(committed.load(Ordering::Relaxed), 1);
        assert_eq!(conflicted.load(Ordering::Relaxed), CALLERS - 1);

        let Harness {
            mut filesystem,
            coordinator,
        } = harness.into_inner().unwrap();
        assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(1));
        drop(coordinator);
        let (coordinator, report) = CommitCoordinator::open(
            &mut filesystem,
            &EntryName::new("concurrent").unwrap(),
            scope(),
            RetentionDays::new(30).unwrap(),
            CounterEntropy(15_000),
            CounterEntropy(16_000),
            &mut TestKeyAdapter,
            CounterState::default(),
        )
        .unwrap();
        assert_eq!(report.frontier.unwrap().get(), 1);
        assert_eq!(coordinator.read_view().unwrap().state(), &CounterState(1));
        drop(coordinator);
        drop(filesystem);
        fs::remove_dir_all(directory).unwrap();
    }

    fn test_directory() -> PathBuf {
        let root = std::env::var_os(TEST_ROOT)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")));
        fs::create_dir_all(&root).unwrap();
        let discriminator = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = root.join(format!(
            "uste-txn-concurrent-{}-{discriminator}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap();
        directory
    }

    fn open_filesystem(directory: &PathBuf) -> LinuxFileSystem {
        let root: std::os::fd::OwnedFd = File::open(directory).unwrap().into();
        match std::env::var(TEST_PROFILE).as_deref() {
            Ok("ext4") => LinuxFileSystem::from_directory_for_profile(
                root,
                LinuxFilesystemProfile::Ext4Candidate,
            )
            .unwrap(),
            Ok(profile) => panic!("unknown test filesystem profile: {profile}"),
            Err(std::env::VarError::NotPresent) => LinuxFileSystem::from_directory(root).unwrap(),
            Err(error) => panic!("invalid test filesystem profile: {error}"),
        }
    }
}
