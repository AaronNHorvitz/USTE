use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
use uste_policy::{
    Action, AuthenticationError, AuthorizationRequirement, AuthorizationRequirements,
    NamespaceGrant, NamespacePolicy, PermissionSet, PolicyKernel, PolicyVersion, PrincipalDigest,
    QuotaLimits, Target, TrustedPrincipalAdapter,
};
use uste_storage::{
    BLOB_CHUNK_BYTES, BlobInventory, BlobUploadToken, ClockObservation, EntryName,
    fault::{FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation, ScriptedClock},
    journal::DurableKeyEnvelope,
    memory::MemoryFileSystem,
};
use uste_txn::{
    ApplyError, AuthorizedCoordinator, AuthorizedError, AuthorizedReadError, AuthorizedReadState,
    AuthorizedTransactionRequest, AuthorizedTransactionState, CommitCoordinator,
    MAX_STAGED_UPLOAD_RESERVATIONS, NeverCancel, RetentionDays, TransactionError, TransactionState,
    open_authorized,
};
use uste_types::{
    CommitRevision, DatabaseId, IdempotencyKey, NamespaceId, NamespaceRef, RecordId, RecordRef,
    TransactionId, UtcInstant,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct CounterState(i64);

impl TransactionState for CounterState {
    type Prepared = i64;
    type Snapshot = Self;

    fn prepare(
        &self,
        canonical_request: &[u8],
        _blob_inventory: Option<&BlobInventory>,
        _revision: CommitRevision,
    ) -> Result<Self::Prepared, ApplyError> {
        if canonical_request.len() != 16 {
            return Err(ApplyError::InvalidRequest);
        }
        let expected = i64::from_be_bytes(canonical_request[..8].try_into().unwrap());
        let delta = i64::from_be_bytes(canonical_request[8..].try_into().unwrap());
        if expected != self.0 {
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

impl AuthorizedTransactionState for CounterState {
    fn authorization_requirements(
        _canonical_request: &[u8],
        _blob_inventory: Option<&BlobInventory>,
    ) -> Result<AuthorizationRequirements, ApplyError> {
        Ok(AuthorizationRequirements::default())
    }
}

impl AuthorizedReadState for CounterState {
    type ReadRequest = ();
    type ReadOutput = i64;
    type ReadError = core::convert::Infallible;

    fn read_authorization_requirements(
        _request: &Self::ReadRequest,
    ) -> Result<AuthorizationRequirements, ApplyError> {
        Ok(AuthorizationRequirements::default())
    }

    fn read_authorized(
        snapshot: &Self::Snapshot,
        _request: &Self::ReadRequest,
        _authorize_candidate: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Result<Self::ReadOutput, Self::ReadError> {
        Ok(snapshot.0)
    }
}

#[derive(Clone)]
struct GuardedState(Arc<AtomicUsize>);

impl TransactionState for GuardedState {
    type Prepared = ();
    type Snapshot = ();

    fn prepare(
        &self,
        _canonical_request: &[u8],
        _blob_inventory: Option<&BlobInventory>,
        _revision: CommitRevision,
    ) -> Result<Self::Prepared, ApplyError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn result_digest(_prepared: &Self::Prepared) -> [u8; 32] {
        [0; 32]
    }

    fn publish(&mut self, _prepared: Self::Prepared) {}

    fn snapshot(&self) -> Self::Snapshot {}
}

impl AuthorizedTransactionState for GuardedState {
    fn authorization_requirements(
        _canonical_request: &[u8],
        _blob_inventory: Option<&BlobInventory>,
    ) -> Result<AuthorizationRequirements, ApplyError> {
        AuthorizationRequirements::new([AuthorizationRequirement {
            action: Action::ReadRecord,
            target: Target::Record(RecordRef::new(
                scope(7).database(),
                scope(7).namespace(),
                RecordId::from_bytes([9; 16]),
            )),
        }])
        .map_err(|_| ApplyError::ResourceLimit)
    }
}

fn mutation(expected: i64, delta: i64) -> [u8; 16] {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&expected.to_be_bytes());
    bytes[8..].copy_from_slice(&delta.to_be_bytes());
    bytes
}

fn scope(namespace: u8) -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([1; 16]),
        NamespaceId::from_bytes([namespace; 16]),
    )
}

fn clock(day: i64) -> ScriptedClock {
    ScriptedClock::new([Ok(ClockObservation {
        wall_utc: UtcInstant::new(day * 86_400, 0).unwrap(),
        monotonic_ticks: u64::try_from(day).unwrap_or_default(),
    })])
}

struct AuthAdapter;

impl TrustedPrincipalAdapter for AuthAdapter {
    type Credential = u8;

    fn authenticate(
        &mut self,
        credential: &Self::Credential,
    ) -> Result<PrincipalDigest, AuthenticationError> {
        Ok(PrincipalDigest::from_bytes([*credential; 32]))
    }
}

fn limits(bytes: u64) -> QuotaLimits {
    QuotaLimits::new(16, bytes, bytes, 2, u32::try_from(bytes).unwrap()).unwrap()
}

fn policy(
    scope: NamespaceRef,
    version: u64,
    grants: &[(u8, &[Action])],
    bytes: u64,
) -> PolicyKernel {
    let mut namespace =
        NamespacePolicy::new(scope, PolicyVersion::new(version).unwrap(), limits(bytes));
    for (principal, actions) in grants {
        namespace
            .grant(
                PrincipalDigest::from_bytes([*principal; 32]),
                NamespaceGrant::new(
                    PermissionSet::from_actions(actions.iter().copied()),
                    limits(bytes),
                ),
            )
            .unwrap();
    }
    let mut kernel = PolicyKernel::new();
    kernel.install_initial_policy(namespace).unwrap();
    kernel
}

fn authenticated(kernel: &PolicyKernel, principal: u8) -> uste_policy::AuthenticatedPrincipal {
    kernel.authenticate(&mut AuthAdapter, &principal).unwrap()
}

const ALL_DATA_ACTIONS: &[Action] = &[
    Action::ReadRecord,
    Action::ReadBlob,
    Action::Commit,
    Action::StartUpload,
    Action::ResumeUpload,
    Action::WriteUpload,
    Action::FinishUpload,
    Action::AbortUpload,
    Action::ReadOwnOutcome,
    Action::InspectQuota,
    Action::ManagePolicy,
];

#[test]
fn outcome_unknown_invalidates_existing_authorized_views() {
    let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(14),
        RetentionDays::new(30).unwrap(),
        EntryName::new("uncertain-authorized-view").unwrap(),
        create_vault(scope(14).database(), 17),
        CounterEntropy(170),
        CounterState::default(),
    )
    .unwrap();
    let kernel = policy(scope(14), 1, &[(3, ALL_DATA_ACTIONS)], 1024);
    let alice = authenticated(&kernel, 3);
    let mut coordinator = AuthorizedCoordinator::new(raw, kernel).unwrap();
    let view = coordinator.read_view(&alice).unwrap();
    filesystem
        .arm(
            FaultPlan::new([FaultPoint {
                operation: Operation::SyncData,
                occurrence: 2,
                action: FaultAction::CrashAfter,
            }])
            .unwrap(),
        )
        .unwrap();
    let bytes = mutation(0, 1);
    assert_eq!(
        coordinator.commit(
            &mut filesystem,
            &alice,
            AuthorizedTransactionRequest {
                idempotency_key: IdempotencyKey::from_bytes([1; 16]),
                transaction_id: TransactionId::from_bytes([2; 16]),
                canonical_request: &bytes,
                blob_inventory: None,
            },
            &mut clock(0),
            &NeverCancel,
        ),
        Err(AuthorizedError::Transaction(
            TransactionError::OutcomeUnknown
        ))
    );
    let uncertain = AuthorizedError::Transaction(TransactionError::OutcomeUnknown);
    assert_eq!(coordinator.read_view_revision(&view), Err(uncertain));
    assert_eq!(
        coordinator.read(&alice, &view, &()),
        Err(AuthorizedReadError::Authorization(uncertain))
    );
}

#[test]
fn default_deny_and_principal_derived_outcomes_conceal_existence() {
    let mut filesystem = MemoryFileSystem::default();
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(2),
        RetentionDays::new(30).unwrap(),
        EntryName::new("deny").unwrap(),
        create_vault(scope(2).database(), 10),
        CounterEntropy(100),
        CounterState::default(),
    )
    .unwrap();
    let kernel = PolicyKernel::new();
    let alice = authenticated(&kernel, 3);
    let mut coordinator = AuthorizedCoordinator::new(raw, kernel).unwrap();
    assert!(matches!(
        coordinator.read_view(&alice),
        Err(AuthorizedError::Unauthorized)
    ));
    assert_eq!(
        coordinator.start_blob_upload(&alice).unwrap_err(),
        AuthorizedError::Unauthorized
    );
    assert_eq!(
        coordinator.transaction_outcome(&alice, TransactionId::from_bytes([8; 16]), &mut clock(0),),
        Err(AuthorizedError::Unauthorized)
    );
}

#[test]
fn reducer_targets_are_denied_before_state_prepare() {
    let mut filesystem = MemoryFileSystem::default();
    let prepares = Arc::new(AtomicUsize::new(0));
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(7),
        RetentionDays::new(30).unwrap(),
        EntryName::new("record-deny").unwrap(),
        create_vault(scope(7).database(), 15),
        CounterEntropy(150),
        GuardedState(Arc::clone(&prepares)),
    )
    .unwrap();
    let mut grant = NamespaceGrant::new(
        PermissionSet::from_actions([Action::Commit, Action::ReadRecord, Action::ReadOwnOutcome]),
        limits(20),
    );
    grant
        .deny_record(
            RecordId::from_bytes([9; 16]),
            PermissionSet::from_actions([Action::ReadRecord]),
        )
        .unwrap();
    let mut namespace = NamespacePolicy::new(scope(7), PolicyVersion::new(1).unwrap(), limits(20));
    namespace
        .grant(PrincipalDigest::from_bytes([3; 32]), grant)
        .unwrap();
    let mut kernel = PolicyKernel::new();
    kernel.install_initial_policy(namespace).unwrap();
    let alice = authenticated(&kernel, 3);
    let mut coordinator = AuthorizedCoordinator::new(raw, kernel).unwrap();
    assert_eq!(
        coordinator.commit(
            &mut filesystem,
            &alice,
            AuthorizedTransactionRequest {
                idempotency_key: IdempotencyKey::from_bytes([1; 16]),
                transaction_id: TransactionId::from_bytes([2; 16]),
                canonical_request: b"x",
                blob_inventory: None,
            },
            &mut clock(0),
            &NeverCancel,
        ),
        Err(AuthorizedError::Unauthorized)
    );
    assert_eq!(prepares.load(Ordering::SeqCst), 0);
}

#[test]
fn reducer_targets_cannot_escape_the_coordinator_scope() {
    let mut filesystem = MemoryFileSystem::default();
    let prepares = Arc::new(AtomicUsize::new(0));
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(13),
        RetentionDays::new(30).unwrap(),
        EntryName::new("cross-scope-record").unwrap(),
        create_vault(scope(13).database(), 16),
        CounterEntropy(160),
        GuardedState(Arc::clone(&prepares)),
    )
    .unwrap();
    let mut local_policy =
        NamespacePolicy::new(scope(13), PolicyVersion::new(1).unwrap(), limits(20));
    local_policy
        .grant(
            PrincipalDigest::from_bytes([3; 32]),
            NamespaceGrant::new(PermissionSet::from_actions([Action::Commit]), limits(20)),
        )
        .unwrap();
    let mut foreign_policy =
        NamespacePolicy::new(scope(7), PolicyVersion::new(1).unwrap(), limits(20));
    foreign_policy
        .grant(
            PrincipalDigest::from_bytes([3; 32]),
            NamespaceGrant::new(
                PermissionSet::from_actions([Action::ReadRecord]),
                limits(20),
            ),
        )
        .unwrap();
    let mut kernel = PolicyKernel::new();
    kernel.install_initial_policy(local_policy).unwrap();
    kernel.install_initial_policy(foreign_policy).unwrap();
    let alice = authenticated(&kernel, 3);
    let mut coordinator = AuthorizedCoordinator::new(raw, kernel).unwrap();

    assert_eq!(
        coordinator.commit(
            &mut filesystem,
            &alice,
            AuthorizedTransactionRequest {
                idempotency_key: IdempotencyKey::from_bytes([11; 16]),
                transaction_id: TransactionId::from_bytes([12; 16]),
                canonical_request: b"x",
                blob_inventory: None,
            },
            &mut clock(0),
            &NeverCancel,
        ),
        Err(AuthorizedError::Unauthorized)
    );
    assert_eq!(prepares.load(Ordering::SeqCst), 0);
}

#[test]
fn exact_byte_quota_commit_read_restart_and_outcome_isolation() {
    let mut filesystem = MemoryFileSystem::default();
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(3),
        RetentionDays::new(30).unwrap(),
        EntryName::new("quota").unwrap(),
        create_vault(scope(3).database(), 20),
        CounterEntropy(200),
        CounterState::default(),
    )
    .unwrap();
    let grants = [(3, ALL_DATA_ACTIONS), (4, &[Action::ReadOwnOutcome][..])];
    let kernel = policy(scope(3), 1, &grants, 5);
    let alice = authenticated(&kernel, 3);
    let bob = authenticated(&kernel, 4);
    let mut coordinator = AuthorizedCoordinator::new(raw, kernel).unwrap();

    let mut upload = coordinator.start_blob_upload(&alice).unwrap();
    coordinator
        .write_blob_upload(&mut filesystem, &alice, &mut upload, b"12")
        .unwrap();
    coordinator
        .write_blob_upload(&mut filesystem, &alice, &mut upload, b"345")
        .unwrap();
    assert_eq!(
        coordinator.write_blob_upload(&mut filesystem, &alice, &mut upload, b"6"),
        Err(AuthorizedError::ResourceLimit)
    );
    assert_eq!(upload.accepted_bytes(), 5);
    let reference = coordinator
        .finish_blob_upload(&mut filesystem, &alice, &mut upload)
        .unwrap();
    let mut concealed = [0_u8; 1];
    assert_eq!(
        coordinator.read_blob_range(&mut filesystem, &bob, reference, 0, &mut concealed),
        Err(AuthorizedError::Unauthorized)
    );
    let staged = coordinator.quota_usage(&alice).unwrap();
    assert_eq!(staged.namespace_staged_bytes, 5);
    assert_eq!(staged.namespace_committed_bytes, 0);

    let inventory = BlobInventory::new(scope(3), [reference]).unwrap();
    let request_bytes = mutation(0, 7);
    let transaction = TransactionId::from_bytes([7; 16]);
    coordinator
        .commit(
            &mut filesystem,
            &alice,
            AuthorizedTransactionRequest {
                idempotency_key: IdempotencyKey::from_bytes([6; 16]),
                transaction_id: transaction,
                canonical_request: &request_bytes,
                blob_inventory: Some(&inventory),
            },
            &mut clock(0),
            &NeverCancel,
        )
        .unwrap();
    let committed = coordinator.quota_usage(&alice).unwrap();
    assert_eq!(committed.namespace_staged_bytes, 0);
    assert_eq!(committed.namespace_committed_bytes, 5);
    assert_eq!(
        coordinator.read_blob_range(&mut filesystem, &bob, reference, 0, &mut concealed),
        Err(AuthorizedError::Unauthorized)
    );
    assert_eq!(
        coordinator
            .transaction_outcome(&bob, transaction, &mut clock(1))
            .unwrap(),
        None
    );
    let mut output = [0_u8; 5];
    assert_eq!(
        coordinator
            .read_blob_range(&mut filesystem, &alice, reference, 0, &mut output)
            .unwrap(),
        5
    );
    assert_eq!(&output, b"12345");
    drop(upload);
    drop(coordinator);
    filesystem.restart().unwrap();

    let reopened_policy = policy(scope(3), 1, &grants, 5);
    let alice = authenticated(&reopened_policy, 3);
    let (mut coordinator, report) = open_authorized(
        &mut filesystem,
        &EntryName::new("quota").unwrap(),
        scope(3),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(300),
        CounterEntropy(400),
        &mut TestKeyAdapter,
        CounterState::default(),
        reopened_policy,
    )
    .unwrap();
    assert_eq!(report.frontier.map(CommitRevision::get), Some(1));
    let usage = coordinator.quota_usage(&alice).unwrap();
    assert_eq!(usage.namespace_committed_bytes, 5);
    assert_eq!(usage.principal_committed_bytes, 5);
    assert_eq!(
        coordinator.start_blob_upload(&alice).unwrap_err(),
        AuthorizedError::ResourceLimit
    );
    assert_eq!(
        coordinator
            .resume_blob_upload(
                &mut filesystem,
                &alice,
                BlobUploadToken::from_upload_id(scope(8), [0xee; 16]),
            )
            .unwrap_err(),
        AuthorizedError::Unauthorized
    );
}

#[test]
fn views_and_uploads_are_bound_to_the_issuing_coordinator() {
    let mut filesystem = MemoryFileSystem::default();
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(10),
        RetentionDays::new(30).unwrap(),
        EntryName::new("instance-bound").unwrap(),
        create_vault(scope(10).database(), 80),
        CounterEntropy(1_200),
        CounterState::default(),
    )
    .unwrap();
    let grants = [(3, ALL_DATA_ACTIONS)];
    let kernel = policy(scope(10), 1, &grants, 20);
    let reopen_kernel = kernel.clone();
    let alice = authenticated(&kernel, 3);
    let coordinator = AuthorizedCoordinator::new(raw, kernel).unwrap();
    let view = coordinator.read_view(&alice).unwrap();
    let mut coordinator = coordinator;
    let mut upload = coordinator.start_blob_upload(&alice).unwrap();
    drop(coordinator);
    filesystem.restart().unwrap();

    let (mut reopened, _) = open_authorized(
        &mut filesystem,
        &EntryName::new("instance-bound").unwrap(),
        scope(10),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(1_300),
        CounterEntropy(1_400),
        &mut TestKeyAdapter,
        CounterState::default(),
        reopen_kernel,
    )
    .unwrap();
    assert_eq!(
        reopened.read_view_revision(&view),
        Err(AuthorizedError::Unauthorized)
    );
    assert_eq!(
        reopened.write_blob_upload(&mut filesystem, &alice, &mut upload, b"x"),
        Err(AuthorizedError::Unauthorized)
    );
    assert_eq!(upload.accepted_bytes(), 0);
}

#[test]
fn resume_only_permission_cannot_turn_a_fabricated_same_scope_token_into_an_upload() {
    let mut filesystem = MemoryFileSystem::default();
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(12),
        RetentionDays::new(30).unwrap(),
        EntryName::new("resume-capability").unwrap(),
        create_vault(scope(12).database(), 100),
        CounterEntropy(1_800),
        CounterState::default(),
    )
    .unwrap();
    let grants = [(4, &[Action::ResumeUpload][..])];
    let kernel = policy(scope(12), 1, &grants, 20);
    let bob = authenticated(&kernel, 4);
    let mut coordinator = AuthorizedCoordinator::new(raw, kernel).unwrap();

    assert_eq!(
        coordinator
            .resume_blob_upload(
                &mut filesystem,
                &bob,
                BlobUploadToken::from_upload_id(scope(12), [0xee; 16]),
            )
            .unwrap_err(),
        AuthorizedError::Unauthorized
    );
}

#[test]
fn recovered_zero_byte_final_marker_is_valid_resume_evidence() {
    let mut filesystem = MemoryFileSystem::default();
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(11),
        RetentionDays::new(30).unwrap(),
        EntryName::new("zero-final-resume").unwrap(),
        create_vault(scope(11).database(), 90),
        CounterEntropy(1_500),
        CounterState::default(),
    )
    .unwrap();
    let grants = [(3, ALL_DATA_ACTIONS)];
    let kernel = policy(scope(11), 1, &grants, 20);
    let alice = authenticated(&kernel, 3);
    let mut coordinator = AuthorizedCoordinator::new(raw, kernel).unwrap();
    let mut upload = coordinator.start_blob_upload(&alice).unwrap();
    let token = upload.token();
    let reference = coordinator
        .finish_blob_upload(&mut filesystem, &alice, &mut upload)
        .unwrap();
    assert_eq!(reference.byte_len(), 0);
    drop(upload);
    drop(coordinator);
    filesystem.restart().unwrap();

    let reopened_policy = policy(scope(11), 1, &grants, 20);
    let alice = authenticated(&reopened_policy, 3);
    let (mut coordinator, _) = open_authorized(
        &mut filesystem,
        &EntryName::new("zero-final-resume").unwrap(),
        scope(11),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(1_600),
        CounterEntropy(1_700),
        &mut TestKeyAdapter,
        CounterState::default(),
        reopened_policy,
    )
    .unwrap();
    let mut resumed = coordinator
        .resume_blob_upload(&mut filesystem, &alice, token)
        .unwrap();
    assert_eq!(
        coordinator
            .finish_blob_upload(&mut filesystem, &alice, &mut resumed)
            .unwrap(),
        reference
    );
    let inventory = BlobInventory::new(scope(11), [reference]).unwrap();
    coordinator
        .commit(
            &mut filesystem,
            &alice,
            AuthorizedTransactionRequest {
                idempotency_key: IdempotencyKey::from_bytes([3; 16]),
                transaction_id: TransactionId::from_bytes([4; 16]),
                canonical_request: &mutation(0, 1),
                blob_inventory: Some(&inventory),
            },
            &mut clock(0),
            &NeverCancel,
        )
        .unwrap();
}

#[test]
fn live_upload_slots_and_durable_abort_release_only_their_exact_reservations() {
    let mut filesystem = MemoryFileSystem::default();
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(5),
        RetentionDays::new(30).unwrap(),
        EntryName::new("abort-quota").unwrap(),
        create_vault(scope(5).database(), 40),
        CounterEntropy(600),
        CounterState::default(),
    )
    .unwrap();
    let grants = [(3, ALL_DATA_ACTIONS), (4, &[Action::ResumeUpload][..])];
    let kernel = policy(scope(5), 1, &grants, 20);
    let alice = authenticated(&kernel, 3);
    let bob = authenticated(&kernel, 4);
    let mut coordinator = AuthorizedCoordinator::new(raw, kernel).unwrap();
    let first = coordinator.start_blob_upload(&alice).unwrap();
    let first_token = first.token();
    let second = coordinator.start_blob_upload(&alice).unwrap();
    assert_eq!(
        coordinator.start_blob_upload(&alice).unwrap_err(),
        AuthorizedError::ResourceLimit
    );
    assert_eq!(
        coordinator
            .resume_blob_upload(&mut filesystem, &alice, first_token)
            .unwrap_err(),
        AuthorizedError::ResourceLimit
    );
    assert_eq!(
        coordinator
            .resume_blob_upload(&mut filesystem, &bob, first_token)
            .unwrap_err(),
        AuthorizedError::Unauthorized
    );
    drop(first);
    let resumed = coordinator
        .resume_blob_upload(&mut filesystem, &alice, first_token)
        .unwrap();
    drop(resumed);
    let mut third = coordinator.start_blob_upload(&alice).unwrap();
    coordinator
        .write_blob_upload(&mut filesystem, &alice, &mut third, b"seven!!")
        .unwrap();
    assert_eq!(
        coordinator
            .quota_usage(&alice)
            .unwrap()
            .principal_staged_bytes,
        7
    );
    coordinator
        .abort_blob_upload(&mut filesystem, &alice, &mut third)
        .unwrap();
    assert_eq!(
        coordinator
            .quota_usage(&alice)
            .unwrap()
            .principal_staged_bytes,
        0
    );
    drop(second);
}

#[test]
fn failed_chunk_publication_reconciles_the_actual_accepted_byte_delta() {
    let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(6),
        RetentionDays::new(30).unwrap(),
        EntryName::new("fault-quota").unwrap(),
        create_vault(scope(6).database(), 50),
        CounterEntropy(700),
        CounterState::default(),
    )
    .unwrap();
    let grants = [(3, ALL_DATA_ACTIONS)];
    let quota = u64::try_from(BLOB_CHUNK_BYTES).unwrap();
    let kernel = policy(scope(6), 1, &grants, quota);
    let alice = authenticated(&kernel, 3);
    let mut coordinator = AuthorizedCoordinator::new(raw, kernel).unwrap();
    let mut upload = coordinator.start_blob_upload(&alice).unwrap();
    filesystem
        .arm(
            FaultPlan::new([FaultPoint {
                operation: Operation::SyncAll,
                occurrence: 1,
                action: FaultAction::CrashAfter,
            }])
            .unwrap(),
        )
        .unwrap();
    let bytes = vec![0xa5; BLOB_CHUNK_BYTES];
    assert!(matches!(
        coordinator.write_blob_upload(&mut filesystem, &alice, &mut upload, &bytes),
        Err(AuthorizedError::Transaction(_))
    ));
    assert_eq!(upload.accepted_bytes(), quota);
    assert_eq!(
        coordinator
            .quota_usage(&alice)
            .unwrap()
            .principal_staged_bytes,
        quota
    );
    assert!(filesystem.is_crashed());
    assert_eq!(filesystem.pending_faults(), 0);
}

#[test]
fn restart_denies_new_staging_but_allows_known_token_reconciliation() {
    let mut filesystem = MemoryFileSystem::default();
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(8),
        RetentionDays::new(30).unwrap(),
        EntryName::new("restart-staging").unwrap(),
        create_vault(scope(8).database(), 60),
        CounterEntropy(800),
        CounterState::default(),
    )
    .unwrap();
    let grants = [(3, ALL_DATA_ACTIONS)];
    let quota = u64::try_from(BLOB_CHUNK_BYTES).unwrap();
    let kernel = policy(scope(8), 1, &grants, quota);
    let alice = authenticated(&kernel, 3);
    let mut coordinator = AuthorizedCoordinator::new(raw, kernel).unwrap();
    let mut upload = coordinator.start_blob_upload(&alice).unwrap();
    let bytes = vec![0x5c; BLOB_CHUNK_BYTES];
    coordinator
        .write_blob_upload(&mut filesystem, &alice, &mut upload, &bytes)
        .unwrap();
    let token = upload.token();
    drop(upload);
    drop(coordinator);
    filesystem.restart().unwrap();

    let reopened_policy = policy(scope(8), 1, &grants, quota);
    let alice = authenticated(&reopened_policy, 3);
    let (mut coordinator, _) = open_authorized(
        &mut filesystem,
        &EntryName::new("restart-staging").unwrap(),
        scope(8),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(900),
        CounterEntropy(1_000),
        &mut TestKeyAdapter,
        CounterState::default(),
        reopened_policy,
    )
    .unwrap();
    assert_eq!(
        coordinator.start_blob_upload(&alice).unwrap_err(),
        AuthorizedError::ResourceLimit
    );
    let mut resumed = coordinator
        .resume_blob_upload(&mut filesystem, &alice, token)
        .unwrap();
    assert_eq!(resumed.accepted_bytes(), quota);
    assert_eq!(
        coordinator.write_blob_upload(&mut filesystem, &alice, &mut resumed, b"6"),
        Err(AuthorizedError::ResourceLimit)
    );
    coordinator
        .abort_blob_upload(&mut filesystem, &alice, &mut resumed)
        .unwrap();
    assert_eq!(
        coordinator
            .quota_usage(&alice)
            .unwrap()
            .namespace_staged_bytes,
        0
    );
}

#[test]
fn unresolved_zero_byte_reservations_are_count_bounded() {
    let mut filesystem = MemoryFileSystem::default();
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(9),
        RetentionDays::new(30).unwrap(),
        EntryName::new("reservation-bound").unwrap(),
        create_vault(scope(9).database(), 70),
        CounterEntropy(1_100),
        CounterState::default(),
    )
    .unwrap();
    let grants = [(3, ALL_DATA_ACTIONS)];
    let kernel = policy(scope(9), 1, &grants, 20);
    let alice = authenticated(&kernel, 3);
    let mut coordinator = AuthorizedCoordinator::new(raw, kernel).unwrap();
    for _ in 0..MAX_STAGED_UPLOAD_RESERVATIONS {
        drop(coordinator.start_blob_upload(&alice).unwrap());
    }
    assert_eq!(
        coordinator.start_blob_upload(&alice).unwrap_err(),
        AuthorizedError::ResourceLimit
    );
}

#[test]
fn revocation_invalidates_existing_views_and_upload_handles() {
    let mut filesystem = MemoryFileSystem::default();
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(4),
        RetentionDays::new(30).unwrap(),
        EntryName::new("revoke").unwrap(),
        create_vault(scope(4).database(), 30),
        CounterEntropy(500),
        CounterState::default(),
    )
    .unwrap();
    let grants = [(3, ALL_DATA_ACTIONS)];
    let kernel = policy(scope(4), 1, &grants, 20);
    let alice = authenticated(&kernel, 3);
    let mut coordinator = AuthorizedCoordinator::new(raw, kernel).unwrap();
    let view = coordinator.read_view(&alice).unwrap();
    let mut upload = coordinator.start_blob_upload(&alice).unwrap();
    let replacement = NamespacePolicy::new(scope(4), PolicyVersion::new(2).unwrap(), limits(20));
    coordinator
        .replace_namespace_policy(&alice, PolicyVersion::new(1).unwrap(), replacement)
        .unwrap();
    assert_eq!(
        coordinator.read_view_revision(&view),
        Err(AuthorizedError::StalePolicy)
    );
    assert_eq!(
        coordinator.write_blob_upload(&mut filesystem, &alice, &mut upload, b"hidden"),
        Err(AuthorizedError::StalePolicy)
    );
    assert_eq!(upload.accepted_bytes(), 0);
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
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
