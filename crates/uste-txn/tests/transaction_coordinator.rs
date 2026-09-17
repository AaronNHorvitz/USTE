use sha2::{Digest, Sha256};
use std::cell::Cell;
use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
use uste_storage::{
    AdapterErrorKind, ClockObservation, EntryName,
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
    }
}

fn clock(day: i64) -> ScriptedClock {
    ScriptedClock::new([Ok(ClockObservation {
        wall_utc: instant(day),
        monotonic_ticks: u64::try_from(day).unwrap_or_default(),
    })])
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
    assert_eq!(
        coordinator
            .commit(
                &mut filesystem,
                request(11, 12, &bytes),
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
    let retry = coordinator
        .commit(
            &mut filesystem,
            request(11, 12, &bytes),
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
