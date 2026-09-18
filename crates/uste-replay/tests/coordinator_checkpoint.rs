use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
use uste_replay::{ReplayError, capture_coordinator_checkpoint, decode_coordinator_checkpoint};
use uste_storage::{
    BlobInventory, CheckpointInput, Clock, ClockObservation, EntryName, IndexEntry, IndexRootInput,
    journal::DurableKeyEnvelope, memory::MemoryFileSystem,
};
use uste_txn::{
    ApplyError, COORDINATOR_METADATA_PROFILE_V1, CheckpointState, CheckpointStateError,
    CommitCoordinator, CoordinatorMetadataLoadLimits, NeverCancel, PrincipalDigest, RetentionDays,
    TransactionError, TransactionRequest, TransactionState, load_coordinator_metadata_candidates,
    load_verified_checkpoint_candidates, publish_coordinator_metadata_root,
    reconstruct_coordinator_metadata_seed, stream_verified_checkpoint_candidate,
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

struct AlwaysCancel;
impl uste_txn::Cancellation for AlwaysCancel {
    fn is_cancelled(&self) -> bool {
        true
    }
}

type FaultDiskCounter = uste_txn::DiskCommitCoordinator<
    CounterState,
    uste_storage::fault::FaultFileSystem<MemoryFileSystem>,
    TestEnvelope,
    CounterEntropy,
    CounterEntropy,
>;

fn reopen_fault_disk_counter(
    filesystem: &mut uste_storage::fault::FaultFileSystem<MemoryFileSystem>,
    name: &EntryName,
    state: CounterState,
) -> (FaultDiskCounter, u64) {
    let base_revision = state.revision.unwrap();
    // Recovery must get fresh entropy after each simulated process restart, including when a
    // previous process left immutable scratch objects behind.
    static NEXT_ENTROPY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(100_000);
    let entropy = NEXT_ENTROPY.fetch_add(10_000, std::sync::atomic::Ordering::Relaxed);
    let (recovery, report) = uste_txn::AuthenticatedIndexRecovery::open(
        filesystem,
        name,
        state.scope,
        CounterEntropy::new(entropy),
        CounterEntropy::new(entropy + 5_000),
        &mut TestKeyAdapter,
    )
    .unwrap();
    let mut cache = uste_storage::PageCache::new(64 * 1024).unwrap();
    let lookup = uste_storage::IndexGetLimits::new(16, 136).unwrap();
    let transaction_root = recovery
        .load_index_root_manifests(filesystem, uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1)
        .unwrap()
        .into_iter()
        .find(|root| root.revision() == base_revision)
        .unwrap();
    let transactions = uste_txn::admit_coordinator_transaction_index_for_recovery(
        &recovery,
        filesystem,
        transaction_root,
        uste_txn::CoordinatorTransactionAdmissionLimits {
            run: uste_storage::IndexRunReadLimits::new(16, 10, 4096).unwrap(),
            lookup,
            maximum_groups: base_revision.get(),
            maximum_encoded_bytes: 1_000_000,
        },
        &mut cache,
    )
    .unwrap();
    let candidate =
        uste_txn::load_coordinator_metadata_candidates_for_recovery::<CounterState, _, _, _, _>(
            &recovery, filesystem,
        )
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.revision() == base_revision)
        .unwrap();
    let base = uste_txn::admit_coordinator_disk_base(
        &recovery,
        filesystem,
        candidate,
        transactions,
        uste_txn::CoordinatorDiskAdmissionLimits {
            metadata: CoordinatorMetadataLoadLimits::new(
                base_revision.get(),
                0,
                base_revision.get() + 1,
                16,
                4096,
            )
            .unwrap(),
            lookup,
            maximum_total_journal_groups: base_revision.get(),
            maximum_encoded_bytes_per_pass: 1_000_000,
        },
        &mut cache,
    )
    .unwrap();
    let coordinator = uste_txn::DiskCommitCoordinator::recover_from_admitted_base(
        recovery,
        filesystem,
        base,
        state,
        RetentionDays::new(30).unwrap(),
        uste_txn::DiskCoordinatorRecoveryLimits {
            overlay: uste_txn::CoordinatorRecoveryLimits::new(2, 0).unwrap(),
            lookup,
            maximum_encoded_bytes: 1_000_000,
        },
        &mut cache,
    )
    .unwrap();
    (coordinator, report.frontier.unwrap().get())
}

#[test]
fn disk_coordinator_commit_faults_preserve_base_and_exact_suffix() {
    use uste_storage::fault::{FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation};
    let mut cases = Vec::new();
    for operation in [Operation::WriteAt, Operation::SyncData] {
        for occurrence in [1, 2] {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                cases.push((operation, occurrence, action));
            }
        }
    }
    cases.push((
        Operation::ReadAt,
        1,
        FaultAction::Error(uste_storage::AdapterErrorKind::Io),
    ));
    for (operation, occurrence, action) in cases {
        let scope = scope();
        let name = EntryName::new("disk-coordinator-faults").unwrap();
        let mut filesystem =
            FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
        let vault = KeyVault::create(
            scope.database(),
            &mut TestKeyAdapter,
            CounterEntropy::new(80_000),
        )
        .unwrap();
        let mut coordinator = CommitCoordinator::create(
            &mut filesystem,
            scope,
            RetentionDays::new(30).unwrap(),
            name.clone(),
            vault,
            CounterEntropy::new(81_000),
            CounterState::new(scope),
        )
        .unwrap();
        let first = 5_u64.to_be_bytes();
        let second = 7_u64.to_be_bytes();
        let mut clock = TestClock(20);
        coordinator
            .commit(
                &mut filesystem,
                request(1, &first),
                &mut clock,
                &NeverCancel,
            )
            .unwrap();
        let base_state = coordinator.read_view().unwrap().state().clone();
        publish_coordinator_metadata_root(&mut coordinator, &mut filesystem).unwrap();
        uste_txn::publish_coordinator_transaction_index(&mut coordinator, &mut filesystem).unwrap();
        drop(coordinator);
        filesystem.restart().unwrap();
        let (mut disk, frontier) =
            reopen_fault_disk_counter(&mut filesystem, &name, base_state.clone());
        assert_eq!(frontier, 1);
        filesystem
            .arm(
                FaultPlan::new([FaultPoint {
                    operation,
                    occurrence,
                    action,
                }])
                .unwrap(),
            )
            .unwrap();
        let lookup = uste_storage::IndexGetLimits::new(16, 136).unwrap();
        let mut cache = uste_storage::PageCache::new(64 * 1024).unwrap();
        let error = disk
            .commit(
                &mut filesystem,
                request(2, &second),
                &mut clock,
                &NeverCancel,
                lookup,
                &mut cache,
            )
            .unwrap_err();
        assert_eq!(filesystem.pending_faults(), 0, "fault must actually fire");
        if operation == Operation::ReadAt {
            assert!(matches!(error, TransactionError::Storage(_)));
            assert_eq!(disk.state().unwrap().value, 5);
        } else {
            assert_eq!(error, TransactionError::OutcomeUnknown);
            assert!(matches!(
                disk.state(),
                Err(TransactionError::OutcomeUnknown)
            ));
            assert!(matches!(
                disk.outcome(
                    &mut filesystem,
                    PrincipalDigest::from_bytes([1; 32]),
                    IdempotencyKey::from_bytes([1; 16]),
                    UtcInstant::new(20, 0).unwrap(),
                    lookup,
                    &mut cache
                ),
                Err(TransactionError::OutcomeUnknown)
            ));
        }
        assert_eq!(disk.overlay_counts(), (0, 0));
        drop(disk);
        filesystem.restart().unwrap();
        let (mut disk, frontier) = reopen_fault_disk_counter(&mut filesystem, &name, base_state);
        let committed = operation == Operation::SyncData
            && occurrence == 2
            && action == FaultAction::CrashAfter;
        assert_eq!(
            frontier,
            if committed { 2 } else { 1 },
            "{operation:?}/{occurrence}/{action:?}"
        );
        assert_eq!(disk.state().unwrap().value, if committed { 12 } else { 5 });
        let retry = disk
            .commit(
                &mut filesystem,
                request(2, &second),
                &mut clock,
                &NeverCancel,
                lookup,
                &mut cache,
            )
            .unwrap();
        assert_eq!(retry.revision.get(), 2);
        assert_eq!(disk.state().unwrap().value, 12);
        assert_eq!(disk.overlay_counts(), (1, 0));
        let base_state = disk.state().unwrap().clone();
        if operation == Operation::ReadAt {
            let mut limited = metadata_rebase_limits();
            limited.merge =
                uste_storage::IndexRunMergeLimits::new(limited.reuse, 10, 4096, 1, 4096).unwrap();
            assert!(matches!(
                disk.rebase_metadata(&mut filesystem, limited),
                Err(TransactionError::Storage(
                    uste_storage::journal::StorageError::ResourceLimit
                ))
            ));
            assert_eq!(disk.overlay_counts(), (1, 0));
            assert!(disk.rebase_required());
        }
        disk.rebase_metadata(&mut filesystem, metadata_rebase_limits())
            .unwrap();
        assert_eq!(disk.overlay_counts(), (0, 0));
        assert!(!disk.rebase_required());
        assert_eq!(
            disk.commit(
                &mut filesystem,
                request(2, &second),
                &mut clock,
                &NeverCancel,
                lookup,
                &mut cache
            )
            .unwrap(),
            retry
        );
        drop(disk);
        filesystem.restart().unwrap();
        let (mut disk, frontier) = reopen_fault_disk_counter(&mut filesystem, &name, base_state);
        assert_eq!(frontier, 2);
        assert_eq!(disk.overlay_counts(), (0, 0));
        disk.commit(
            &mut filesystem,
            request(3, &first),
            &mut clock,
            &NeverCancel,
            lookup,
            &mut cache,
        )
        .unwrap();
        assert_eq!(disk.state().unwrap().value, 17);
    }
}

fn metadata_rebase_limits() -> uste_txn::CoordinatorMetadataRebaseLimits {
    let read = uste_storage::IndexRunReadLimits::new(16, 10, 4096).unwrap();
    uste_txn::CoordinatorMetadataRebaseLimits {
        merge: uste_storage::IndexRunMergeLimits::new(read, 10, 4096, 10, 4096).unwrap(),
        reuse: read,
    }
}

#[test]
fn disk_metadata_rebase_failures_preserve_pinned_pair_and_resynchronize_retries() {
    use uste_storage::fault::{FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation};
    let boundaries = (1..=5)
        .map(|n| (Operation::SyncAll, n))
        .chain((1..=7).map(|n| (Operation::SyncDirectory, n)))
        .chain((4..=7).map(|n| (Operation::WriteAt, n)));
    for (operation, occurrence) in boundaries {
        for action in [
            FaultAction::Error(uste_storage::AdapterErrorKind::Io),
            FaultAction::CrashBefore,
            FaultAction::CrashAfter,
        ] {
            let scope = scope();
            let name = EntryName::new("disk-metadata-rebase-faults").unwrap();
            let mut filesystem =
                FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
            let vault = KeyVault::create(
                scope.database(),
                &mut TestKeyAdapter,
                CounterEntropy::new(82_000),
            )
            .unwrap();
            let mut coordinator = CommitCoordinator::create(
                &mut filesystem,
                scope,
                RetentionDays::new(30).unwrap(),
                name.clone(),
                vault,
                CounterEntropy::new(83_000),
                CounterState::new(scope),
            )
            .unwrap();
            let first = 5_u64.to_be_bytes();
            let second = 7_u64.to_be_bytes();
            let mut clock = TestClock(20);
            coordinator
                .commit(
                    &mut filesystem,
                    request(1, &first),
                    &mut clock,
                    &NeverCancel,
                )
                .unwrap();
            let base_state = coordinator.read_view().unwrap().state().clone();
            publish_coordinator_metadata_root(&mut coordinator, &mut filesystem).unwrap();
            uste_txn::publish_coordinator_transaction_index(&mut coordinator, &mut filesystem)
                .unwrap();
            drop(coordinator);
            filesystem.restart().unwrap();
            let (mut disk, _) =
                reopen_fault_disk_counter(&mut filesystem, &name, base_state.clone());
            let lookup = uste_storage::IndexGetLimits::new(16, 136).unwrap();
            let mut cache = uste_storage::PageCache::new(64 * 1024).unwrap();
            let outcome = disk
                .commit(
                    &mut filesystem,
                    request(2, &second),
                    &mut clock,
                    &NeverCancel,
                    lookup,
                    &mut cache,
                )
                .unwrap();
            let current_state = disk.state().unwrap().clone();
            filesystem
                .arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
            assert!(
                disk.rebase_metadata(&mut filesystem, metadata_rebase_limits())
                    .is_err(),
                "{operation:?}/{occurrence}/{action:?}"
            );
            assert_eq!(filesystem.pending_faults(), 0);
            assert!(disk.rebase_required());
            assert_eq!(disk.overlay_counts(), (1, 0));
            assert_eq!(
                disk.state().unwrap().value,
                12,
                "cache failure does not invalidate certified state"
            );
            if !filesystem.is_crashed() {
                assert!(matches!(
                    disk.commit(
                        &mut filesystem,
                        request(3, &first),
                        &mut clock,
                        &NeverCancel,
                        lookup,
                        &mut cache
                    ),
                    Err(TransactionError::ResourceLimit)
                ));
                assert_eq!(
                    disk.commit(
                        &mut filesystem,
                        request(2, &second),
                        &mut clock,
                        &NeverCancel,
                        lookup,
                        &mut cache
                    )
                    .unwrap(),
                    outcome
                );
            }
            if operation == Operation::SyncAll
                && occurrence == 5
                && action == FaultAction::Error(uste_storage::AdapterErrorKind::Io)
            {
                // Repeat a partial-pair failure without a restart; the pinned old pair must not
                // be rotated away by publishing another equivalent metadata generation.
                filesystem
                    .arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                assert!(
                    disk.rebase_metadata(&mut filesystem, metadata_rebase_limits())
                        .is_err()
                );
                assert_eq!(filesystem.pending_faults(), 0);
                drop(disk);
                filesystem.restart().unwrap();
                (disk, _) = reopen_fault_disk_counter(&mut filesystem, &name, base_state.clone());
                assert!(disk.rebase_required());
            } else if filesystem.is_crashed() {
                drop(disk);
                filesystem.restart().unwrap();
                (disk, _) = reopen_fault_disk_counter(&mut filesystem, &name, base_state.clone());
            }
            disk.rebase_metadata(&mut filesystem, metadata_rebase_limits())
                .unwrap_or_else(|error| {
                    panic!("rebase retry {operation:?}/{occurrence}/{action:?}: {error:?}")
                });
            assert_eq!(disk.overlay_counts(), (0, 0));
            assert!(!disk.rebase_required());
            drop(disk);
            filesystem.restart().unwrap();
            let (mut disk, frontier) =
                reopen_fault_disk_counter(&mut filesystem, &name, current_state);
            assert_eq!(frontier, 2);
            assert_eq!(disk.overlay_counts(), (0, 0));
            assert_eq!(
                disk.commit(
                    &mut filesystem,
                    request(2, &second),
                    &mut clock,
                    &NeverCancel,
                    lookup,
                    &mut cache
                )
                .unwrap(),
                outcome
            );
            disk.commit(
                &mut filesystem,
                request(3, &first),
                &mut clock,
                &NeverCancel,
                lookup,
                &mut cache,
            )
            .unwrap();
            assert_eq!(disk.state().unwrap().value, 17);
        }
    }
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

impl uste_txn::DiskCoordinatorState for CounterState {
    fn metadata_publication_input(
        &self,
        anchor: (CommitRevision, [u8; 32]),
    ) -> Result<IndexRootInput, ApplyError> {
        if self.revision != Some(anchor.0) {
            return Err(ApplyError::Conflict);
        }
        Ok(IndexRootInput {
            scope: self.scope,
            revision: anchor.0,
            certificate_digest: anchor.1,
            reducer_profile: Self::REDUCER_PROFILE,
            logical_state_digest: Self::logical_state_digest(self)
                .map_err(|_| ApplyError::Conflict)?,
            index_profile: COORDINATOR_METADATA_PROFILE_V1,
        })
    }

    fn validate_metadata_base(
        &self,
        root: &uste_storage::RecoveredIndexRoot,
    ) -> Result<(), ApplyError> {
        if root.scope() != self.scope
            || Some(root.revision()) != self.revision
            || root.reducer_profile() != &Self::REDUCER_PROFILE
            || Self::logical_state_digest(self).map_err(|_| ApplyError::Conflict)?
                != *root.logical_state_digest()
        {
            return Err(ApplyError::Conflict);
        }
        Ok(())
    }
}

impl TransactionState for CounterState {
    type Prepared = Self;
    type Snapshot = Self;

    fn prepare(
        &self,
        canonical_request: &[u8],
        _blob_inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<Self::Prepared, ApplyError> {
        if canonical_request.len() != 8 {
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
fn encrypted_metadata_root_seeds_exact_prefix_and_replays_suffix() {
    let scope = scope();
    let name = EntryName::new("coordinator-metadata-root").unwrap();
    let mut filesystem = MemoryFileSystem::default();
    let vault = KeyVault::create(
        scope.database(),
        &mut TestKeyAdapter,
        CounterEntropy::new(20_000),
    )
    .unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope,
        RetentionDays::new(30).unwrap(),
        name.clone(),
        vault,
        CounterEntropy::new(21_000),
        CounterState::new(scope),
    )
    .unwrap();
    let mut clock = TestClock(20);
    let mut upload = coordinator.start_blob_upload(scope).unwrap();
    coordinator
        .write_blob_upload(
            &mut filesystem,
            &mut upload,
            b"coordinator metadata preserves first-commit blob ownership",
        )
        .unwrap();
    let reference = coordinator
        .finish_blob_upload(&mut filesystem, &mut upload)
        .unwrap();
    let inventory = BlobInventory::new(scope, [reference]).unwrap();
    let first_bytes = 5_u64.to_be_bytes();
    coordinator
        .commit(
            &mut filesystem,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(1, &first_bytes)
            },
            &mut clock,
            &NeverCancel,
        )
        .unwrap();
    let checkpoint_state = coordinator.read_view().unwrap().state().clone();
    let transaction_index =
        uste_txn::publish_coordinator_transaction_index(&mut coordinator, &mut filesystem).unwrap();
    let (principal, _, expected_outcome) = coordinator.checkpoint_outcomes().next().unwrap();
    let mut transaction_cache = uste_storage::PageCache::new(64 * 1024).unwrap();
    let lookup_limits = uste_storage::IndexGetLimits::new(16, 136).unwrap();
    assert_eq!(
        transaction_index
            .lookup(
                &coordinator,
                &mut filesystem,
                expected_outcome.transaction_id,
                lookup_limits,
                &mut transaction_cache,
            )
            .unwrap(),
        Some((principal, expected_outcome))
    );
    assert_eq!(
        transaction_index
            .lookup(
                &coordinator,
                &mut filesystem,
                TransactionId::from_bytes([0xee; 16]),
                lookup_limits,
                &mut transaction_cache,
            )
            .unwrap(),
        None
    );
    assert!(matches!(
        transaction_index.lookup(
            &coordinator,
            &mut filesystem,
            expected_outcome.transaction_id,
            uste_storage::IndexGetLimits::new(16, 135).unwrap(),
            &mut transaction_cache,
        ),
        Err(TransactionError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
    let publication = publish_coordinator_metadata_root(&mut coordinator, &mut filesystem).unwrap();
    let candidates = load_coordinator_metadata_candidates(&coordinator, &mut filesystem).unwrap();
    let candidate = candidates
        .iter()
        .find(|candidate| candidate.generation() == publication.generation)
        .unwrap();
    let limits = CoordinatorMetadataLoadLimits::new(4, 4, 9, 16, 1024 * 1024).unwrap();
    let (seed, report) = reconstruct_coordinator_metadata_seed(
        &coordinator,
        &mut filesystem,
        candidate,
        checkpoint_state.clone(),
        limits,
    )
    .unwrap();
    assert_eq!(report.runs, 3);
    assert_eq!(report.entries, 3);
    assert!(report.logical_bytes > 0);
    assert!(report.pages_read >= report.runs);
    assert!(matches!(
        reconstruct_coordinator_metadata_seed(
            &coordinator,
            &mut filesystem,
            candidate,
            checkpoint_state.clone(),
            CoordinatorMetadataLoadLimits::new(0, 4, 9, 16, 1024 * 1024).unwrap(),
        ),
        Err(TransactionError::ResourceLimit)
    ));
    assert!(matches!(
        reconstruct_coordinator_metadata_seed(
            &coordinator,
            &mut filesystem,
            candidate,
            checkpoint_state.clone(),
            CoordinatorMetadataLoadLimits::new(4, 0, 9, 16, 1024 * 1024).unwrap(),
        ),
        Err(TransactionError::ResourceLimit)
    ));
    let mut wrong_state = checkpoint_state.clone();
    wrong_state.value = 999;
    assert!(matches!(
        reconstruct_coordinator_metadata_seed(
            &coordinator,
            &mut filesystem,
            candidate,
            wrong_state,
            limits,
        ),
        Err(TransactionError::IntegrityFailure)
    ));

    // The carrier run and descriptor are self-consistent and authenticated, but the frozen
    // outcome value has nonzero reserved bytes. Semantic reconstruction must still reject it.
    let canonical_root = coordinator
        .load_index_roots(&mut filesystem, COORDINATOR_METADATA_PROFILE_V1)
        .unwrap()
        .into_iter()
        .find(|root| root.generation() == publication.generation)
        .unwrap();
    let mut runs = canonical_root.runs().copied().collect::<Vec<_>>();
    let (principal, idempotency_key, outcome) = coordinator.checkpoint_outcomes().next().unwrap();
    let mut outcome_key = Vec::with_capacity(48);
    outcome_key.extend_from_slice(&principal.as_bytes());
    outcome_key.extend_from_slice(idempotency_key.as_bytes());
    let mut outcome_value = Vec::with_capacity(104);
    outcome_value.extend_from_slice(outcome.transaction_id.as_bytes());
    outcome_value.extend_from_slice(&outcome.revision.get().to_be_bytes());
    outcome_value.extend_from_slice(&outcome.request_digest);
    outcome_value.extend_from_slice(&outcome.result_digest);
    outcome_value.extend_from_slice(&outcome.expires_at.seconds().to_be_bytes());
    outcome_value.extend_from_slice(&outcome.expires_at.nanoseconds().to_be_bytes());
    outcome_value.extend_from_slice(&[1, 0, 0, 0]);
    let malformed_run = coordinator
        .publish_index_run(
            &mut filesystem,
            publication.revision,
            COORDINATOR_METADATA_PROFILE_V1,
            2,
            [IndexEntry {
                key: outcome_key,
                value: outcome_value,
            }],
        )
        .unwrap();
    *runs.iter_mut().find(|run| run.family() == 2).unwrap() = malformed_run;
    let (revision, certificate_digest) = coordinator.checkpoint_anchor().unwrap().unwrap();
    let malformed_root = coordinator
        .publish_index_root(
            &mut filesystem,
            IndexRootInput {
                scope,
                revision,
                certificate_digest,
                reducer_profile: CounterState::REDUCER_PROFILE,
                logical_state_digest: CounterState::logical_state_digest(&checkpoint_state)
                    .unwrap(),
                index_profile: COORDINATOR_METADATA_PROFILE_V1,
            },
            &runs,
        )
        .unwrap();
    let malformed_candidates =
        load_coordinator_metadata_candidates(&coordinator, &mut filesystem).unwrap();
    let malformed_candidate = malformed_candidates
        .iter()
        .find(|candidate| candidate.generation() == malformed_root.generation)
        .unwrap();
    assert!(matches!(
        reconstruct_coordinator_metadata_seed(
            &coordinator,
            &mut filesystem,
            malformed_candidate,
            checkpoint_state,
            limits,
        ),
        Err(TransactionError::Storage(
            uste_storage::journal::StorageError::IntegrityFailure
        ))
    ));

    let second_bytes = 7_u64.to_be_bytes();
    coordinator
        .commit(
            &mut filesystem,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(2, &second_bytes)
            },
            &mut clock,
            &NeverCancel,
        )
        .unwrap();
    assert!(matches!(
        transaction_index.lookup(
            &coordinator,
            &mut filesystem,
            expected_outcome.transaction_id,
            lookup_limits,
            &mut transaction_cache,
        ),
        Err(TransactionError::IntegrityFailure)
    ));
    drop(coordinator);
    filesystem.restart().unwrap();
    let (recovered, recovery) = CommitCoordinator::open_seeded(
        &mut filesystem,
        &name,
        scope,
        RetentionDays::new(30).unwrap(),
        CounterEntropy::new(22_000),
        CounterEntropy::new(23_000),
        &mut TestKeyAdapter,
        seed,
    )
    .unwrap();
    assert_eq!(recovery.frontier.unwrap().get(), 2);
    let state = recovered.read_view().unwrap().state().clone();
    assert_eq!(state.value, 12);
    assert_eq!(state.prepare_calls_since_decode, 2);
    assert_eq!(recovered.checkpoint_outcomes().len(), 2);
    assert_eq!(recovered.committed_blob_owners().count(), 1);
    // The retained root is at revision one: admission must not present it as revision two.
    assert!(
        uste_txn::load_coordinator_transaction_indexes(
            &recovered,
            &mut filesystem,
            uste_storage::IndexRunReadLimits::new(16, 10, 4096).unwrap(),
        )
        .unwrap()
        .is_empty()
    );
    let mut recovered = recovered;
    let good_metadata = publish_coordinator_metadata_root(&mut recovered, &mut filesystem).unwrap();
    uste_txn::publish_coordinator_transaction_index(&mut recovered, &mut filesystem).unwrap();
    let indexes = uste_txn::load_coordinator_transaction_indexes(
        &recovered,
        &mut filesystem,
        uste_storage::IndexRunReadLimits::new(16, 10, 4096).unwrap(),
    )
    .unwrap();
    assert_eq!(indexes.len(), 1);
    assert!(matches!(
        uste_txn::load_coordinator_transaction_indexes(
            &recovered,
            &mut filesystem,
            uste_storage::IndexRunReadLimits::new(16, 1, 4096).unwrap(),
        ),
        Err(TransactionError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
    assert_eq!(
        indexes[0]
            .lookup(
                &recovered,
                &mut filesystem,
                expected_outcome.transaction_id,
                lookup_limits,
                &mut transaction_cache,
            )
            .unwrap(),
        Some((principal, expected_outcome))
    );
    let root = recovered
        .load_index_root_manifests(
            &mut filesystem,
            uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
        )
        .unwrap()
        .remove(0);
    let mut entries = Vec::new();
    recovered
        .visit_index_run(
            &mut filesystem,
            &root,
            1,
            uste_storage::IndexRunReadLimits::new(16, 10, 4096).unwrap(),
            &mut |key, value| {
                entries.push(IndexEntry {
                    key: key.to_vec(),
                    value: value.to_vec(),
                });
                Ok(())
            },
        )
        .unwrap();
    entries[0].value[0] ^= 1;
    let wrong = recovered
        .publish_index_run(
            &mut filesystem,
            root.revision(),
            uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
            1,
            entries,
        )
        .unwrap();
    recovered
        .publish_index_root(
            &mut filesystem,
            IndexRootInput {
                scope,
                revision: root.revision(),
                certificate_digest: *root.certificate_digest(),
                reducer_profile: *root.reducer_profile(),
                logical_state_digest: *root.logical_state_digest(),
                index_profile: uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
            },
            &[wrong],
        )
        .unwrap();
    assert!(
        uste_txn::load_coordinator_transaction_indexes(
            &recovered,
            &mut filesystem,
            uste_storage::IndexRunReadLimits::new(16, 10, 4096).unwrap(),
        )
        .is_err(),
        "authenticated but wrong principal must not be admitted"
    );
    let good_generation = root.generation();
    let metadata_root = recovered
        .load_index_root_manifests(&mut filesystem, COORDINATOR_METADATA_PROFILE_V1)
        .unwrap()
        .into_iter()
        .find(|root| root.generation() == good_metadata.generation)
        .unwrap();
    let mut owner_entries = Vec::new();
    recovered
        .visit_index_run(
            &mut filesystem,
            &metadata_root,
            3,
            uste_storage::IndexRunReadLimits::new(16, 10, 4096).unwrap(),
            &mut |key, value| {
                owner_entries.push(IndexEntry {
                    key: key.to_vec(),
                    value: value.to_vec(),
                });
                Ok(())
            },
        )
        .unwrap();
    // The second principal also references the blob, but must never become its first owner.
    owner_entries[0].value[48..].copy_from_slice(&[2; 32]);
    let wrong_owner_run = recovered
        .publish_index_run(
            &mut filesystem,
            metadata_root.revision(),
            COORDINATOR_METADATA_PROFILE_V1,
            3,
            owner_entries,
        )
        .unwrap();
    let owner_runs = metadata_root
        .runs()
        .map(|run| {
            if run.family() == 3 {
                wrong_owner_run
            } else {
                *run
            }
        })
        .collect::<Vec<_>>();
    recovered
        .publish_index_root(
            &mut filesystem,
            IndexRootInput {
                scope,
                revision: metadata_root.revision(),
                certificate_digest: *metadata_root.certificate_digest(),
                reducer_profile: *metadata_root.reducer_profile(),
                logical_state_digest: *metadata_root.logical_state_digest(),
                index_profile: COORDINATOR_METADATA_PROFILE_V1,
            },
            &owner_runs,
        )
        .unwrap();
    drop(recovered);
    filesystem.restart().unwrap();
    let (recovery, _) = uste_txn::AuthenticatedIndexRecovery::open(
        &mut filesystem,
        &name,
        scope,
        CounterEntropy::new(24_000),
        CounterEntropy::new(25_000),
        &mut TestKeyAdapter,
    )
    .unwrap();
    let admission_limits = uste_txn::CoordinatorTransactionAdmissionLimits {
        run: uste_storage::IndexRunReadLimits::new(16, 10, 4096).unwrap(),
        lookup: lookup_limits,
        maximum_groups: 2,
        maximum_encoded_bytes: 1_000_000,
    };
    let candidates = recovery
        .load_index_root_manifests(
            &mut filesystem,
            uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
        )
        .unwrap();
    assert_eq!(candidates.len(), 2);
    for candidate in candidates {
        let good = candidate.generation() == good_generation;
        let result = uste_txn::admit_coordinator_transaction_index_for_recovery(
            &recovery,
            &mut filesystem,
            candidate,
            admission_limits,
            &mut transaction_cache,
        );
        assert_eq!(
            result.is_ok(),
            good,
            "journal correspondence is independent of coordinator maps"
        );
    }
    for limited in [
        uste_txn::CoordinatorTransactionAdmissionLimits {
            maximum_groups: 1,
            ..admission_limits
        },
        uste_txn::CoordinatorTransactionAdmissionLimits {
            maximum_encoded_bytes: 1,
            ..admission_limits
        },
        uste_txn::CoordinatorTransactionAdmissionLimits {
            lookup: uste_storage::IndexGetLimits::new(16, 135).unwrap(),
            ..admission_limits
        },
    ] {
        let candidate = recovery
            .load_index_root_manifests(
                &mut filesystem,
                uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
            )
            .unwrap()
            .into_iter()
            .find(|root| root.generation() == good_generation)
            .unwrap();
        assert!(
            uste_txn::admit_coordinator_transaction_index_for_recovery(
                &recovery,
                &mut filesystem,
                candidate,
                limited,
                &mut transaction_cache,
            )
            .is_err()
        );
    }
    let base_limits = uste_txn::CoordinatorDiskAdmissionLimits {
        metadata: CoordinatorMetadataLoadLimits::new(2, 1, 4, 32, 4096).unwrap(),
        lookup: lookup_limits,
        maximum_total_journal_groups: 4,
        maximum_encoded_bytes_per_pass: 1_000_000,
    };
    let mismatched_candidate =
        uste_txn::load_coordinator_metadata_candidates_for_recovery::<CounterState, _, _, _, _>(
            &recovery,
            &mut filesystem,
        )
        .unwrap()
        .remove(0);
    assert!(matches!(
        uste_txn::admit_coordinator_disk_base(
            &recovery,
            &mut filesystem,
            mismatched_candidate,
            transaction_index,
            base_limits,
            &mut transaction_cache,
        ),
        Err(TransactionError::IntegrityFailure)
    ));
    let mut admitted_base = None;
    for maximum_total_journal_groups in [3, 4] {
        let candidates = uste_txn::load_coordinator_metadata_candidates_for_recovery::<
            CounterState,
            _,
            _,
            _,
            _,
        >(&recovery, &mut filesystem)
        .unwrap();
        assert_eq!(candidates.len(), 2);
        for candidate in candidates {
            let good = candidate.generation() == good_metadata.generation;
            let transaction_root = recovery
                .load_index_root_manifests(
                    &mut filesystem,
                    uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
                )
                .unwrap()
                .into_iter()
                .find(|root| root.generation() == good_generation)
                .unwrap();
            let transaction_index = uste_txn::admit_coordinator_transaction_index_for_recovery(
                &recovery,
                &mut filesystem,
                transaction_root,
                admission_limits,
                &mut transaction_cache,
            )
            .unwrap();
            let result = uste_txn::admit_coordinator_disk_base(
                &recovery,
                &mut filesystem,
                candidate,
                transaction_index,
                uste_txn::CoordinatorDiskAdmissionLimits {
                    maximum_total_journal_groups,
                    ..base_limits
                },
                &mut transaction_cache,
            );
            assert_eq!(result.is_ok(), good && maximum_total_journal_groups == 4);
            if let Ok(base) = result {
                assert_eq!(
                    base.retry_at_base(
                        &recovery,
                        &mut filesystem,
                        principal,
                        idempotency_key,
                        lookup_limits,
                        &mut transaction_cache,
                    )
                    .unwrap(),
                    Some(expected_outcome)
                );
                assert_eq!(
                    base.retry_at_base(
                        &recovery,
                        &mut filesystem,
                        PrincipalDigest::from_bytes([0xee; 32]),
                        idempotency_key,
                        lookup_limits,
                        &mut transaction_cache,
                    )
                    .unwrap(),
                    None
                );
                assert_eq!(
                    base.owner_at_base(
                        &recovery,
                        &mut filesystem,
                        reference.id(),
                        lookup_limits,
                        &mut transaction_cache,
                    )
                    .unwrap(),
                    Some((reference, PrincipalDigest::from_bytes([1; 32])))
                );
                admitted_base = Some(base);
            }
        }
    }
    let mut disk = uste_txn::DiskCommitCoordinator::from_admitted_base(
        recovery,
        admitted_base.unwrap(),
        state.clone(),
        RetentionDays::new(30).unwrap(),
        uste_txn::CoordinatorRecoveryLimits::new(1, 0).unwrap(),
    )
    .unwrap();
    assert_eq!(disk.overlay_counts(), (0, 0));
    assert_eq!(
        disk.commit(
            &mut filesystem,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(1, &first_bytes)
            },
            &mut clock,
            &NeverCancel,
            lookup_limits,
            &mut transaction_cache,
        )
        .unwrap(),
        expected_outcome
    );
    assert_eq!(disk.overlay_counts(), (0, 0));
    assert!(matches!(
        disk.commit(
            &mut filesystem,
            TransactionRequest {
                transaction_id: expected_outcome.transaction_id,
                ..request(3, &first_bytes)
            },
            &mut clock,
            &NeverCancel,
            lookup_limits,
            &mut transaction_cache,
        ),
        Err(TransactionError::Conflict)
    ));
    let third_bytes = 9_u64.to_be_bytes();
    let mut new_upload = disk.start_blob_upload(scope).unwrap();
    disk.write_blob_upload(
        &mut filesystem,
        &mut new_upload,
        b"new owner needs an admitted overlay slot",
    )
    .unwrap();
    let new_reference = disk
        .finish_blob_upload(&mut filesystem, &mut new_upload)
        .unwrap();
    let new_inventory = BlobInventory::new(scope, [new_reference]).unwrap();
    assert!(matches!(
        disk.commit(
            &mut filesystem,
            TransactionRequest {
                blob_inventory: Some(&new_inventory),
                ..request(3, &third_bytes)
            },
            &mut clock,
            &NeverCancel,
            lookup_limits,
            &mut transaction_cache,
        ),
        Err(TransactionError::ResourceLimit)
    ));
    assert_eq!(disk.overlay_counts(), (0, 0));
    assert_eq!(disk.checkpoint_anchor().unwrap().unwrap().0.get(), 2);
    assert!(matches!(
        disk.commit(
            &mut filesystem,
            request(3, &third_bytes),
            &mut clock,
            &AlwaysCancel,
            lookup_limits,
            &mut transaction_cache,
        ),
        Err(TransactionError::Cancelled)
    ));
    let third = disk
        .commit(
            &mut filesystem,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(3, &third_bytes)
            },
            &mut clock,
            &NeverCancel,
            lookup_limits,
            &mut transaction_cache,
        )
        .unwrap();
    assert_eq!(third.revision.get(), 3);
    assert_eq!(
        disk.overlay_counts(),
        (1, 0),
        "existing disk owner does not consume overlay capacity"
    );
    assert_eq!(
        disk.commit(
            &mut filesystem,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(3, &third_bytes)
            },
            &mut clock,
            &NeverCancel,
            lookup_limits,
            &mut transaction_cache,
        )
        .unwrap(),
        third
    );
    assert!(matches!(
        disk.commit(
            &mut filesystem,
            request(4, &third_bytes),
            &mut clock,
            &NeverCancel,
            lookup_limits,
            &mut transaction_cache,
        ),
        Err(TransactionError::ResourceLimit)
    ));
    assert!(matches!(
        disk.outcome(
            &mut filesystem,
            principal,
            idempotency_key,
            expected_outcome.expires_at,
            lookup_limits,
            &mut transaction_cache,
        ),
        Err(TransactionError::IdempotencyExpired)
    ));
    assert_eq!(disk.state().unwrap().value, 21);
    assert_eq!(
        disk.transaction_outcome(
            &mut filesystem,
            principal,
            expected_outcome.transaction_id,
            UtcInstant::new(20, 0).unwrap(),
            lookup_limits,
            &mut transaction_cache,
        )
        .unwrap(),
        Some(expected_outcome)
    );
    assert_eq!(
        disk.transaction_outcome(
            &mut filesystem,
            PrincipalDigest::from_bytes([0xee; 32]),
            expected_outcome.transaction_id,
            UtcInstant::new(20, 0).unwrap(),
            lookup_limits,
            &mut transaction_cache,
        )
        .unwrap(),
        None
    );
    assert_eq!(
        disk.committed_blob_owner(
            &mut filesystem,
            reference,
            lookup_limits,
            &mut transaction_cache,
        )
        .unwrap(),
        Some(principal)
    );
    drop(disk);
    filesystem.restart().unwrap();
    let (recovered, _) = CommitCoordinator::open(
        &mut filesystem,
        &name,
        scope,
        RetentionDays::new(30).unwrap(),
        CounterEntropy::new(26_000),
        CounterEntropy::new(27_000),
        &mut TestKeyAdapter,
        CounterState::new(scope),
    )
    .unwrap();
    assert_eq!(recovered.read_view().unwrap().state().value, 21);
    assert_eq!(
        recovered.committed_blob_owner(reference),
        Some(PrincipalDigest::from_bytes([1; 32]))
    );
    let mut recovered = recovered;
    let fourth = recovered
        .commit(
            &mut filesystem,
            TransactionRequest {
                blob_inventory: Some(&new_inventory),
                ..request(4, &third_bytes)
            },
            &mut clock,
            &NeverCancel,
        )
        .unwrap();
    drop(recovered);
    filesystem.restart().unwrap();
    for (maximum_outcomes, maximum_owners, maximum_encoded_bytes) in [
        (1, 1, 1_000_000),
        (2, 0, 1_000_000),
        (2, 1, 1),
        (2, 1, 10_000),
        (2, 1, 1_000_000),
    ] {
        let (recovery, _) = uste_txn::AuthenticatedIndexRecovery::open(
            &mut filesystem,
            &name,
            scope,
            CounterEntropy::new(28_000),
            CounterEntropy::new(29_000),
            &mut TestKeyAdapter,
        )
        .unwrap();
        let transaction_root = recovery
            .load_index_root_manifests(
                &mut filesystem,
                uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
            )
            .unwrap()
            .into_iter()
            .find(|root| root.generation() == good_generation)
            .unwrap();
        let transaction_index = uste_txn::admit_coordinator_transaction_index_for_recovery(
            &recovery,
            &mut filesystem,
            transaction_root,
            admission_limits,
            &mut transaction_cache,
        )
        .unwrap();
        let candidate = uste_txn::load_coordinator_metadata_candidates_for_recovery::<
            CounterState,
            _,
            _,
            _,
            _,
        >(&recovery, &mut filesystem)
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.generation() == good_metadata.generation)
        .unwrap();
        let base = uste_txn::admit_coordinator_disk_base(
            &recovery,
            &mut filesystem,
            candidate,
            transaction_index,
            base_limits,
            &mut transaction_cache,
        )
        .unwrap();
        let result = uste_txn::DiskCommitCoordinator::recover_from_admitted_base(
            recovery,
            &mut filesystem,
            base,
            state.clone(),
            RetentionDays::new(30).unwrap(),
            uste_txn::DiskCoordinatorRecoveryLimits {
                overlay: uste_txn::CoordinatorRecoveryLimits::new(maximum_outcomes, maximum_owners)
                    .unwrap(),
                lookup: lookup_limits,
                maximum_encoded_bytes,
            },
            &mut transaction_cache,
        );
        if maximum_outcomes == 1 || maximum_owners == 0 || maximum_encoded_bytes < 1_000_000 {
            assert!(matches!(
                result,
                Err(TransactionError::ResourceLimit
                    | TransactionError::Storage(
                        uste_storage::journal::StorageError::ResourceLimit
                    ))
            ));
        } else {
            let mut recovered = result.unwrap();
            assert_eq!(recovered.state().unwrap().value, 30);
            assert_eq!(recovered.overlay_counts(), (2, 1));
            assert_eq!(
                recovered
                    .commit(
                        &mut filesystem,
                        TransactionRequest {
                            blob_inventory: Some(&new_inventory),
                            ..request(4, &third_bytes)
                        },
                        &mut clock,
                        &NeverCancel,
                        lookup_limits,
                        &mut transaction_cache,
                    )
                    .unwrap(),
                fourth
            );
            assert_eq!(
                recovered
                    .committed_blob_owner(
                        &mut filesystem,
                        reference,
                        lookup_limits,
                        &mut transaction_cache
                    )
                    .unwrap(),
                Some(principal)
            );
            assert_eq!(
                recovered
                    .committed_blob_owner(
                        &mut filesystem,
                        new_reference,
                        lookup_limits,
                        &mut transaction_cache,
                    )
                    .unwrap(),
                Some(PrincipalDigest::from_bytes([4; 32]))
            );
            recovered
                .rebase_metadata(&mut filesystem, metadata_rebase_limits())
                .unwrap();
            assert_eq!(recovered.overlay_counts(), (0, 0));
            assert_eq!(
                recovered
                    .committed_blob_owner(
                        &mut filesystem,
                        new_reference,
                        lookup_limits,
                        &mut transaction_cache,
                    )
                    .unwrap(),
                Some(PrincipalDigest::from_bytes([4; 32]))
            );
            assert_eq!(
                recovered
                    .commit(
                        &mut filesystem,
                        TransactionRequest {
                            blob_inventory: Some(&new_inventory),
                            ..request(4, &third_bytes)
                        },
                        &mut clock,
                        &NeverCancel,
                        lookup_limits,
                        &mut transaction_cache,
                    )
                    .unwrap(),
                fourth
            );
        }
    }
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

    let mut streamed = Vec::new();
    let (stream_candidate, stream_report) =
        stream_verified_checkpoint_candidate::<_, TestEnvelope, _, _, _>(
            &mut filesystem,
            &name,
            scope,
            0,
            CounterEntropy::new(250),
            CounterEntropy::new(260),
            &mut TestKeyAdapter,
            &mut |bytes| {
                streamed.extend_from_slice(bytes);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(stream_report.frontier.unwrap().get(), 2);
    assert_eq!(stream_candidate.unwrap().revision().get(), 2);
    assert_eq!(streamed, malformed_payload);

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
