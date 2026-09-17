use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
use uste_storage::{
    AdapterErrorKind, ClockObservation, EntryName,
    fault::{FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation, ScriptedClock},
    journal::DurableKeyEnvelope,
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

struct AlwaysCancel;

impl Cancellation for AlwaysCancel {
    fn is_cancelled(&self) -> bool {
        true
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
