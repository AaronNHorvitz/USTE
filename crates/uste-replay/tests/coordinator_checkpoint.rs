use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
use uste_replay::{ReplayError, capture_coordinator_checkpoint, decode_coordinator_checkpoint};
use uste_storage::{
    BlobInventory, CheckpointInput, Clock, ClockObservation, EntryName,
    journal::DurableKeyEnvelope, memory::MemoryFileSystem,
};
use uste_txn::{
    ApplyError, CheckpointState, CheckpointStateError, CommitCoordinator, NeverCancel,
    PrincipalDigest, RetentionDays, TransactionRequest, TransactionState,
    load_verified_checkpoint_candidates,
};
use uste_types::{
    CommitRevision, DatabaseId, IdempotencyKey, NamespaceId, NamespaceRef, TransactionId,
    UtcInstant,
};

#[derive(Clone, Debug, Eq, PartialEq)]
struct CounterState {
    scope: NamespaceRef,
    revision: Option<CommitRevision>,
    value: u64,
    prepare_calls_since_decode: u64,
}

impl CounterState {
    const fn new(scope: NamespaceRef) -> Self {
        Self {
            scope,
            revision: None,
            value: 0,
            prepare_calls_since_decode: 0,
        }
    }
}

impl TransactionState for CounterState {
    type Prepared = Self;
    type Snapshot = Self;

    fn prepare(
        &self,
        canonical_request: &[u8],
        blob_inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<Self::Prepared, ApplyError> {
        if blob_inventory.is_some() || canonical_request.len() != 8 {
            return Err(ApplyError::InvalidRequest);
        }
        Ok(Self {
            scope: self.scope,
            revision: Some(revision),
            value: self
                .value
                .checked_add(u64::from_be_bytes(canonical_request.try_into().unwrap()))
                .ok_or(ApplyError::ResourceLimit)?,
            prepare_calls_since_decode: self
                .prepare_calls_since_decode
                .checked_add(1)
                .ok_or(ApplyError::ResourceLimit)?,
        })
    }

    fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
        let mut digest = [0_u8; 32];
        digest[..8].copy_from_slice(&prepared.value.to_be_bytes());
        digest[8..16].copy_from_slice(
            &prepared
                .revision
                .expect("prepared revision")
                .get()
                .to_be_bytes(),
        );
        digest
    }

    fn publish(&mut self, prepared: Self::Prepared) {
        *self = prepared;
    }

    fn snapshot(&self) -> Self::Snapshot {
        self.clone()
    }
}

impl CheckpointState for CounterState {
    const REDUCER_PROFILE: [u8; 32] = [0x91; 32];

    fn checkpoint_scope(snapshot: &Self::Snapshot) -> NamespaceRef {
        snapshot.scope
    }

    fn checkpoint_revision(snapshot: &Self::Snapshot) -> Option<CommitRevision> {
        snapshot.revision
    }

    fn logical_state_digest(snapshot: &Self::Snapshot) -> Result<[u8; 32], CheckpointStateError> {
        let mut digest = [0_u8; 32];
        digest[..8].copy_from_slice(&snapshot.value.to_be_bytes());
        digest[8..16].copy_from_slice(
            &snapshot
                .revision
                .map_or(0, CommitRevision::get)
                .to_be_bytes(),
        );
        Ok(digest)
    }

    fn encode_checkpoint(snapshot: &Self::Snapshot) -> Result<Vec<u8>, CheckpointStateError> {
        let mut encoded = Vec::with_capacity(16);
        encoded.extend_from_slice(
            &snapshot
                .revision
                .ok_or(CheckpointStateError::Invalid)?
                .get()
                .to_be_bytes(),
        );
        encoded.extend_from_slice(&snapshot.value.to_be_bytes());
        Ok(encoded)
    }

    fn decode_checkpoint(
        scope: NamespaceRef,
        revision: CommitRevision,
        encoded: &[u8],
    ) -> Result<Self, CheckpointStateError> {
        if encoded.len() != 16
            || u64::from_be_bytes(encoded[..8].try_into().unwrap()) != revision.get()
        {
            return Err(CheckpointStateError::Invalid);
        }
        Ok(Self {
            scope,
            revision: Some(revision),
            value: u64::from_be_bytes(encoded[8..].try_into().unwrap()),
            prepare_calls_since_decode: 0,
        })
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

#[derive(Debug, Default)]
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

impl CounterEntropy {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }
}

impl EntropySource for CounterEntropy {
    fn fill(&mut self, output: &mut [u8]) -> Result<(), EntropyFailure> {
        self.0 = self.0.checked_add(1).ok_or(EntropyFailure)?;
        let mut block = self.0;
        for chunk in output.chunks_mut(8) {
            let bytes = block.to_be_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
            block = block.checked_add(1).ok_or(EntropyFailure)?;
        }
        Ok(())
    }
}

struct TestClock(i64);

impl Clock for TestClock {
    fn observe(&mut self) -> Result<ClockObservation, uste_storage::AdapterError> {
        let now = self.0;
        self.0 += 1;
        Ok(ClockObservation {
            wall_utc: UtcInstant::new(now, 0).unwrap(),
            monotonic_ticks: u64::try_from(now).unwrap(),
        })
    }
}

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([0x81; 16]),
        NamespaceId::from_bytes([0x82; 16]),
    )
}

fn request<'a>(value: u8, bytes: &'a [u8; 8]) -> TransactionRequest<'a> {
    TransactionRequest {
        principal: PrincipalDigest::from_bytes([value; 32]),
        idempotency_key: IdempotencyKey::from_bytes([value; 16]),
        transaction_id: TransactionId::from_bytes([value.wrapping_add(32); 16]),
        canonical_request: bytes,
        blob_inventory: None,
    }
}

fn assert_authenticated_malformed_candidate_rejected(
    label: &str,
    mutate: impl FnOnce(&mut Vec<u8>),
) {
    let scope = scope();
    let name = EntryName::new("malformed-coordinator-checkpoint").unwrap();
    let mut filesystem = MemoryFileSystem::default();
    let vault = KeyVault::create(
        scope.database(),
        &mut TestKeyAdapter,
        CounterEntropy::new(10_000),
    )
    .unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope,
        RetentionDays::new(30).unwrap(),
        name.clone(),
        vault,
        CounterEntropy::new(11_000),
        CounterState::new(scope),
    )
    .unwrap();
    let mut clock = TestClock(10);
    let first_bytes = 5_u64.to_be_bytes();
    coordinator
        .commit(
            &mut filesystem,
            request(1, &first_bytes),
            &mut clock,
            &NeverCancel,
        )
        .unwrap();
    let valid = capture_coordinator_checkpoint(
        coordinator.reducer_state_for_checkpoint().unwrap(),
        coordinator.checkpoint_anchor().unwrap().unwrap(),
        coordinator.checkpoint_outcomes(),
        coordinator.committed_blob_owners(),
    )
    .unwrap();
    coordinator
        .publish_checkpoint(&mut filesystem, valid.storage_input())
        .unwrap();
    let second_bytes = 7_u64.to_be_bytes();
    coordinator
        .commit(
            &mut filesystem,
            request(2, &second_bytes),
            &mut clock,
            &NeverCancel,
        )
        .unwrap();
    let malformed = capture_coordinator_checkpoint(
        coordinator.reducer_state_for_checkpoint().unwrap(),
        coordinator.checkpoint_anchor().unwrap().unwrap(),
        coordinator.checkpoint_outcomes(),
        coordinator.committed_blob_owners(),
    )
    .unwrap();
    let input = malformed.storage_input();
    let mut payload = input.payload.to_vec();
    assert_eq!(payload.len(), 480);
    mutate(&mut payload);
    coordinator
        .publish_checkpoint(
            &mut filesystem,
            CheckpointInput {
                scope: input.scope,
                revision: input.revision,
                certificate_digest: input.certificate_digest,
                reducer_profile: input.reducer_profile,
                logical_state_digest: input.logical_state_digest,
                payload: &payload,
            },
        )
        .unwrap();
    drop(coordinator);
    filesystem.restart().unwrap();

    let (candidates, _) = load_verified_checkpoint_candidates::<_, TestEnvelope, _, _, _>(
        &mut filesystem,
        &name,
        scope,
        CounterEntropy::new(12_000),
        CounterEntropy::new(13_000),
        &mut TestKeyAdapter,
    )
    .unwrap();
    assert_eq!(candidates.len(), 2, "case={label}");
    assert!(
        decode_coordinator_checkpoint::<CounterState>(&candidates[0]).is_err(),
        "case={label}"
    );
    assert!(
        decode_coordinator_checkpoint::<CounterState>(&candidates[1]).is_ok(),
        "case={label}"
    );
}

#[test]
fn authenticated_malformed_coordinator_payloads_fail_closed_with_older_fallback() {
    for cut in [1, 7, 39, 143, 151, 167, 175, 323, 479] {
        assert_authenticated_malformed_candidate_rejected(&format!("truncate-{cut}"), |payload| {
            payload.truncate(cut);
        });
    }
    for (label, offset) in [
        ("scope", 8),
        ("revision", 40),
        ("certificate", 48),
        ("profile", 80),
        ("logical-digest", 112),
    ] {
        assert_authenticated_malformed_candidate_rejected(label, |payload| payload[offset] ^= 1);
    }
    assert_authenticated_malformed_candidate_rejected("oversized-reducer-frame", |payload| {
        payload[144..152].copy_from_slice(&u64::MAX.to_be_bytes());
    });
    assert_authenticated_malformed_candidate_rejected("duplicate-retry-key", |payload| {
        let first_key = payload[176..224].to_vec();
        payload[324..372].copy_from_slice(&first_key);
    });
    assert_authenticated_malformed_candidate_rejected("reversed-retry-order", |payload| {
        payload[324..356].fill(0);
    });
    assert_authenticated_malformed_candidate_rejected("invalid-utc-nanoseconds", |payload| {
        payload[320..324].copy_from_slice(&1_000_000_000_u32.to_be_bytes());
    });
    assert_authenticated_malformed_candidate_rejected("impossible-owner-count", |payload| {
        payload[472..480].copy_from_slice(&u64::MAX.to_be_bytes());
    });
    assert_authenticated_malformed_candidate_rejected("invalid-blob-shape", |payload| {
        payload[472..480].copy_from_slice(&1_u64.to_be_bytes());
        payload.extend_from_slice(&[1; 16]);
        payload.extend_from_slice(&0_u64.to_be_bytes());
        payload.extend_from_slice(&1_u32.to_be_bytes());
        payload.extend_from_slice(&[2; 32]);
        payload.extend_from_slice(&[3; 32]);
    });
    assert_authenticated_malformed_candidate_rejected("trailing-byte", |payload| {
        payload.push(0);
    });
}

#[test]
fn encrypted_checkpoint_restores_coordinator_and_replays_only_reducer_suffix() {
    let scope = scope();
    let name = EntryName::new("coordinator-checkpoint").unwrap();
    let mut filesystem = MemoryFileSystem::default();
    let vault = KeyVault::create(
        scope.database(),
        &mut TestKeyAdapter,
        CounterEntropy::new(100),
    )
    .unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope,
        RetentionDays::new(30).unwrap(),
        name.clone(),
        vault,
        CounterEntropy::new(200),
        CounterState::new(scope),
    )
    .unwrap();
    let mut clock = TestClock(10);
    let first_bytes = 5_u64.to_be_bytes();
    coordinator
        .commit(
            &mut filesystem,
            request(1, &first_bytes),
            &mut clock,
            &NeverCancel,
        )
        .unwrap();
    let outcomes = coordinator.checkpoint_outcomes().collect::<Vec<_>>();
    let owners = coordinator.committed_blob_owners().collect::<Vec<_>>();
    let checkpoint = capture_coordinator_checkpoint(
        coordinator.reducer_state_for_checkpoint().unwrap(),
        coordinator.checkpoint_anchor().unwrap().unwrap(),
        outcomes,
        owners,
    )
    .unwrap();
    coordinator
        .publish_checkpoint(&mut filesystem, checkpoint.storage_input())
        .unwrap();

    let second_bytes = 7_u64.to_be_bytes();
    coordinator
        .commit(
            &mut filesystem,
            request(2, &second_bytes),
            &mut clock,
            &NeverCancel,
        )
        .unwrap();

    // Storage authenticates generic checkpoint bytes; the reducer/coordinator codec must still
    // reject a bounded but malformed newest candidate and permit the older complete cache.
    let current_checkpoint = capture_coordinator_checkpoint(
        coordinator.reducer_state_for_checkpoint().unwrap(),
        coordinator.checkpoint_anchor().unwrap().unwrap(),
        coordinator.checkpoint_outcomes(),
        coordinator.committed_blob_owners(),
    )
    .unwrap();
    let current_input = current_checkpoint.storage_input();
    let mut malformed_payload = current_input.payload.to_vec();
    let reducer_length = usize::try_from(u64::from_be_bytes(
        malformed_payload[144..152].try_into().unwrap(),
    ))
    .unwrap();
    let outcome_count_at = 152 + reducer_length;
    malformed_payload[outcome_count_at..outcome_count_at + 8]
        .copy_from_slice(&u64::MAX.to_be_bytes());
    coordinator
        .publish_checkpoint(
            &mut filesystem,
            CheckpointInput {
                scope: current_input.scope,
                revision: current_input.revision,
                certificate_digest: current_input.certificate_digest,
                reducer_profile: current_input.reducer_profile,
                logical_state_digest: current_input.logical_state_digest,
                payload: &malformed_payload,
            },
        )
        .unwrap();
    drop(coordinator);
    filesystem.restart().unwrap();

    let (candidates, verified) = load_verified_checkpoint_candidates::<_, TestEnvelope, _, _, _>(
        &mut filesystem,
        &name,
        scope,
        CounterEntropy::new(300),
        CounterEntropy::new(400),
        &mut TestKeyAdapter,
    )
    .unwrap();
    assert_eq!(verified.frontier.unwrap().get(), 2);
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].revision().get(), 2);
    assert!(matches!(
        decode_coordinator_checkpoint::<CounterState>(&candidates[0]),
        Err(ReplayError::Checkpoint(CheckpointStateError::Invalid))
    ));
    assert_eq!(candidates[1].revision().get(), 1);
    let seed = decode_coordinator_checkpoint::<CounterState>(&candidates[1]).unwrap();
    let (recovered, report) = CommitCoordinator::open_seeded(
        &mut filesystem,
        &name,
        scope,
        RetentionDays::new(30).unwrap(),
        CounterEntropy::new(500),
        CounterEntropy::new(600),
        &mut TestKeyAdapter,
        seed,
    )
    .unwrap();
    assert_eq!(report.frontier.unwrap().get(), 2);
    let state = recovered.read_view().unwrap().state().clone();
    assert_eq!(state.value, 12);
    assert_eq!(state.prepare_calls_since_decode, 1);
    assert_eq!(recovered.checkpoint_outcomes().len(), 2);
}
