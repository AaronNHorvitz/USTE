use super::*;
use uste_policy::{
    Action, AuthenticatedPrincipal, AuthenticationError, NamespaceGrant, NamespacePolicy,
    PermissionSet, PolicyKernel, PolicyVersion, QuotaLimits, TrustedPrincipalAdapter,
};
use uste_txn::{
    AuthorizedDiskPolicyState, AuthorizedError, AuthorizedPackedMetadata,
    PackedCoordinatorPublicationState, PackedCoordinatorRecoveryState,
};

fn policy(version: u64) -> NamespacePolicy {
    let quota = QuotaLimits::new(1024, 1024, 1024, 4, 1024).unwrap();
    let mut policy = NamespacePolicy::new(scope(), PolicyVersion::new(version).unwrap(), quota);
    for index in [0, 1] {
        let actions = if version == 2 && index == 0 {
            vec![Action::Commit]
        } else {
            vec![Action::ReadOwnOutcome, Action::InspectQuota, Action::Commit]
        };
        policy
            .grant(
                principal(index),
                NamespaceGrant::new(PermissionSet::from_actions(actions), quota),
            )
            .unwrap();
    }
    for (id, action) in [(5, Action::ReadOwnOutcome), (6, Action::InspectQuota)] {
        policy
            .grant(
                PrincipalDigest::from_bytes([id; 32]),
                NamespaceGrant::new(PermissionSet::from_actions([action]), quota),
            )
            .unwrap();
    }
    policy
}
struct Auth;
impl TrustedPrincipalAdapter for Auth {
    type Credential = u8;
    fn authenticate(&mut self, value: &u8) -> Result<PrincipalDigest, AuthenticationError> {
        Ok(PrincipalDigest::from_bytes([*value; 32]))
    }
}
fn kernel(version: u64) -> PolicyKernel {
    let mut kernel = PolicyKernel::default();
    kernel.install_initial_policy(policy(version)).unwrap();
    kernel
}
fn authenticated(kernel: &PolicyKernel, value: u8) -> AuthenticatedPrincipal {
    kernel.authenticate(&mut Auth, &value).unwrap()
}
#[derive(Clone, Default)]
struct PolicyState {
    value: CounterState,
    revision: Option<CommitRevision>,
    anchor: Option<(CommitRevision, [u8; 32])>,
    policy: Option<NamespacePolicy>,
    ready: bool,
}
impl TransactionState for PolicyState {
    type Prepared = Self;
    type Snapshot = Self;
    fn prepare(
        &self,
        bytes: &[u8],
        inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<Self, ApplyError> {
        let mut next = self.clone();
        match bytes {
            b"policy" => {
                if self.policy.is_some() {
                    return Err(ApplyError::Conflict);
                }
                next.policy = Some(policy(1));
                next.ready = true;
            }
            b"revoke" => next.policy = Some(policy(2)),
            b"pending" => next.ready = false,
            b"missing" => next.policy = None,
            _ => next.value = CounterState(self.value.prepare(bytes, inventory, revision)?),
        }
        next.revision = Some(revision);
        Ok(next)
    }
    fn result_digest(prepared: &Self) -> [u8; 32] {
        let mut bytes = prepared.value.0.to_be_bytes().to_vec();
        bytes.extend_from_slice(
            &prepared
                .revision
                .map_or(0, CommitRevision::get)
                .to_be_bytes(),
        );
        bytes.extend_from_slice(
            &prepared
                .policy
                .as_ref()
                .map_or(0, |p| p.version().get())
                .to_be_bytes(),
        );
        bytes.push(u8::from(prepared.ready));
        // Policies are a closed deterministic versioned catalog owned by this synthetic reducer.
        Sha256::digest(bytes).into()
    }
    fn publish(&mut self, prepared: Self) {
        *self = prepared;
    }
    fn snapshot(&self) -> Self {
        panic!("packed metadata must not construct a domain snapshot")
    }
}
impl PackedCoordinatorState for PolicyState {
    fn validate_packed_metadata(
        &self,
        selected: NamespaceRef,
        claims: PackedRootClaims,
    ) -> Result<(), ApplyError> {
        if selected != scope()
            || self.anchor != Some((claims.revision, claims.certificate_digest))
            || self.revision != Some(claims.revision)
            || !self.ready
            || claims.reducer_profile != [0xa5; 32]
            || claims.state_commitment_profile != [0xd5; 32]
            || claims.state_digest != Self::result_digest(self)
        {
            return Err(ApplyError::Conflict);
        }
        Ok(())
    }
}
impl PackedCoordinatorPublicationState for PolicyState {
    fn packed_publication_claims(
        &self,
        selected: NamespaceRef,
        anchor: (CommitRevision, [u8; 32]),
    ) -> Result<PackedRootClaims, ApplyError> {
        if selected != scope() || self.revision != Some(anchor.0) || !self.ready {
            return Err(ApplyError::Conflict);
        }
        Ok(PackedRootClaims {
            revision: anchor.0,
            certificate_digest: anchor.1,
            generation: 1,
            reducer_profile: [0xa5; 32],
            state_commitment_profile: [0xd5; 32],
            state_digest: Self::result_digest(self),
        })
    }
}
impl PackedCoordinatorRecoveryState for PolicyState {
    fn publish_recovered(
        &mut self,
        mut prepared: Self,
        transaction: &uste_txn::RecoveredFrontierTransaction,
    ) -> Result<(), ApplyError> {
        let expected = self
            .revision
            .map_or(Some(CommitRevision::FIRST), |r| r.checked_next().ok());
        if expected != Some(transaction.revision()) || prepared.revision != expected {
            return Err(ApplyError::Conflict);
        }
        prepared.anchor = Some((transaction.revision(), *transaction.certificate_digest()));
        self.publish(prepared);
        Ok(())
    }
}
impl AuthorizedDiskPolicyState for PolicyState {
    fn current_durable_policy(&self) -> Result<&NamespacePolicy, ApplyError> {
        if !self.ready {
            return Err(ApplyError::Conflict);
        }
        self.policy.as_ref().ok_or(ApplyError::Conflict)
    }
}
type PolicyLive =
    PackedCommitCoordinator<PolicyState, Fs, TestEnvelope, CounterEntropy, CounterEntropy>;
fn fixture() -> (
    Fs,
    EntryName,
    PolicyLive,
    BlobInventory,
    [TransactionOutcome; 2],
) {
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let name = EntryName::new("packed-authorized").unwrap();
    let mut raw = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 7101),
        CounterEntropy(7102),
        PolicyState::default(),
    )
    .unwrap();
    let first = raw
        .commit(
            &mut fs,
            request(1, 11, b"policy"),
            &mut clock(0),
            &NeverCancel,
        )
        .unwrap();
    let references = [b"".as_slice(), b"abc".as_slice()].map(|bytes| {
        let mut upload = raw.start_blob_upload(scope()).unwrap();
        if !bytes.is_empty() {
            raw.write_blob_upload(&mut fs, &mut upload, bytes).unwrap();
        }
        raw.finish_blob_upload(&mut fs, &mut upload).unwrap()
    });
    let inventory = BlobInventory::new(scope(), references).unwrap();
    let second = raw
        .commit(
            &mut fs,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(2, 12, &mutation(0, 1))
            },
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();
    drop(raw);
    let (mut recovery, transactions) = transactions_with_entropy(&mut fs, &name, 2, 7202);
    let mut state = PolicyState::default();
    let mut primary = None;
    let mut quota = None;
    for transaction in &transactions {
        let prepared = state
            .prepare(
                transaction.canonical_request(),
                transaction.blob_inventory(),
                transaction.revision(),
            )
            .unwrap();
        assert_eq!(
            PolicyState::result_digest(&prepared),
            transaction.outcome().result_digest
        );
        let next = stage_packed_coordinator_prefix(
            &mut recovery,
            &mut fs,
            primary.as_ref(),
            transaction,
            limits(),
        )
        .unwrap()
        .0;
        quota = Some(
            stage_packed_quota_prefix(
                &mut recovery,
                &mut fs,
                quota.as_ref(),
                &next,
                transaction,
                limits(),
            )
            .unwrap()
            .0,
        );
        primary = Some(next);
        state.publish_recovered(prepared, transaction).unwrap();
    }
    let primary = primary.unwrap();
    let quota = quota.unwrap();
    let claims = state
        .packed_publication_claims(scope(), primary.anchor())
        .unwrap();
    let primary_root = recovery
        .publish_recovered_packed_root(
            &mut fs,
            COORDINATOR_PACKED_PROFILE_V1,
            claims,
            &primary.families(),
            4,
        )
        .unwrap();
    let quota_root = recovery
        .publish_recovered_packed_root(
            &mut fs,
            COORDINATOR_PACKED_USAGE_PROFILE_V1,
            claims,
            &quota.families(),
            4,
        )
        .unwrap();
    let live = PackedCommitCoordinator::from_admitted_prefixes(
        recovery,
        primary,
        quota,
        &primary_root,
        &quota_root,
        state,
        RetentionDays::new(30).unwrap(),
        CoordinatorRecoveryLimits::new(4, 2).unwrap(),
    )
    .unwrap();
    (fs, name, live, inventory, [first, second])
}

#[test]
fn authorized_packed_reads_derive_identity_conceal_foreign_expiry_and_preserve_quota() {
    let (mut fs, _, mut live, inventory, outcomes) = fixture();
    let policy = kernel(1);
    let alice = authenticated(&policy, 3);
    let bob = authenticated(&policy, 4);
    for rebased in [false, true] {
        if rebased {
            live.commit(
                &mut fs,
                TransactionRequest {
                    principal: principal(1),
                    blob_inventory: Some(&inventory),
                    ..request(3, 13, &mutation(1, 1))
                },
                &mut clock(2),
                &NeverCancel,
                commit_limits(),
                &mut PageCache::new(64 * 1024).unwrap(),
            )
            .unwrap();
            live.rebase_metadata(&mut fs, rebase_limits())
                .unwrap()
                .unwrap();
        }
        let facade = AuthorizedPackedMetadata::new(&live, &policy).unwrap();
        assert_eq!(
            facade
                .outcome(
                    &mut fs,
                    &alice,
                    IdempotencyKey::from_bytes([2; 16]),
                    &mut clock(3)
                )
                .unwrap(),
            Some(outcomes[1])
        );
        assert_eq!(
            facade
                .transaction_outcome(
                    &mut fs,
                    &alice,
                    TransactionId::from_bytes([12; 16]),
                    &mut clock(3)
                )
                .unwrap(),
            Some(outcomes[1])
        );
        for id in [12, 99] {
            assert_eq!(
                facade
                    .transaction_outcome(
                        &mut fs,
                        &bob,
                        TransactionId::from_bytes([id; 16]),
                        &mut clock(40)
                    )
                    .unwrap(),
                None
            );
        }
        assert_eq!(
            facade.outcome(
                &mut fs,
                &alice,
                IdempotencyKey::from_bytes([2; 16]),
                &mut clock(31)
            ),
            Err(AuthorizedError::Transaction(
                TransactionError::IdempotencyExpired
            ))
        );
        assert_eq!(
            facade.committed_blob_usage(&mut fs, &alice, 2).unwrap(),
            CommittedBlobUsage {
                namespace_bytes: 3,
                principal_bytes: 3,
                owners: 2
            }
        );
        assert_eq!(
            facade.committed_blob_usage(&mut fs, &bob, 2).unwrap(),
            CommittedBlobUsage {
                namespace_bytes: 3,
                principal_bytes: 0,
                owners: 2
            }
        );
    }
}

#[test]
fn authorized_packed_denied_foreign_and_revoked_reads_precede_clock_and_filesystem() {
    let (mut fs, _, mut live, _, _) = fixture();
    let policy = kernel(1);
    let other = kernel(1);
    let unknown = authenticated(&policy, 99);
    let foreign = authenticated(&other, 3);
    let facade = AuthorizedPackedMetadata::new(&live, &policy).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    for denied in [&unknown, &foreign] {
        assert!(matches!(
            facade.outcome(
                &mut fs,
                denied,
                IdempotencyKey::from_bytes([2; 16]),
                &mut ScriptedClock::new([])
            ),
            Err(AuthorizedError::Unauthorized)
        ));
        assert!(matches!(
            facade.transaction_outcome(
                &mut fs,
                denied,
                TransactionId::from_bytes([12; 16]),
                &mut ScriptedClock::new([])
            ),
            Err(AuthorizedError::Unauthorized)
        ));
        assert!(matches!(
            facade.committed_blob_usage(&mut fs, denied, 0),
            Err(AuthorizedError::Unauthorized)
        ));
    }
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    live.commit(
        &mut fs,
        request(3, 13, b"revoke"),
        &mut clock(2),
        &NeverCancel,
        commit_limits(),
        &mut PageCache::new(64 * 1024).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        AuthorizedPackedMetadata::new(&live, &policy),
        Err(AuthorizedError::InvalidPolicy)
    ));
    let current = kernel(2);
    let alice = authenticated(&current, 3);
    let bob = authenticated(&current, 4);
    let facade = AuthorizedPackedMetadata::new(&live, &current).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert!(matches!(
        facade.outcome(
            &mut fs,
            &alice,
            IdempotencyKey::from_bytes([2; 16]),
            &mut ScriptedClock::new([])
        ),
        Err(AuthorizedError::Unauthorized)
    ));
    assert!(matches!(
        facade.committed_blob_usage(&mut fs, &alice, 0),
        Err(AuthorizedError::Unauthorized)
    ));
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(
        facade
            .committed_blob_usage(&mut fs, &bob, 2)
            .unwrap()
            .namespace_bytes,
        3
    );
}

#[test]
fn authorized_packed_missing_pending_and_uncertain_states_never_supply_policy_reads() {
    for bytes in [b"missing".as_slice(), b"pending".as_slice()] {
        let (mut fs, _, mut live, _, _) = fixture();
        let policy = kernel(1);
        live.commit(
            &mut fs,
            request(3, 13, bytes),
            &mut clock(2),
            &NeverCancel,
            commit_limits(),
            &mut PageCache::new(64 * 1024).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            AuthorizedPackedMetadata::new(&live, &policy),
            Err(AuthorizedError::InvalidPolicy)
        ));
    }
    let (mut fs, _, mut live, _, _) = fixture();
    let policy = kernel(1);
    fs.arm(
        FaultPlan::new([FaultPoint {
            operation: Operation::SyncData,
            occurrence: 2,
            action: FaultAction::CrashAfter,
        }])
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        live.commit(
            &mut fs,
            request(3, 13, &mutation(1, 1)),
            &mut clock(2),
            &NeverCancel,
            commit_limits(),
            &mut PageCache::new(64 * 1024).unwrap()
        ),
        Err(TransactionError::OutcomeUnknown)
    );
    assert!(matches!(
        AuthorizedPackedMetadata::new(&live, &policy),
        Err(AuthorizedError::Transaction(
            TransactionError::OutcomeUnknown
        ))
    ));
}

fn reopen_policy(fs: &mut Fs, name: &EntryName, count: u64, entropy: u64) -> PolicyLive {
    reopen_policy_with_corruption(fs, name, count, entropy, None)
}
fn reopen_policy_with_corruption(
    fs: &mut Fs,
    name: &EntryName,
    count: u64,
    entropy: u64,
    corrupt: Option<usize>,
) -> PolicyLive {
    let (mut recovery, transactions) = transactions_with_entropy(fs, name, count, entropy);
    let mut discover = |profile| {
        recovery
            .discover_packed_roots_at_revision(
                fs,
                profile,
                CommitRevision::new(2).unwrap(),
                limits().certificates,
                PackedRootDiscoveryLimits::new(4, 4 * 4177).unwrap(),
            )
            .unwrap()
            .0
    };
    let primary_root = discover(COORDINATOR_PACKED_PROFILE_V1).pop().unwrap();
    let quota_root = discover(COORDINATOR_PACKED_USAGE_PROFILE_V1).pop().unwrap();
    let primary =
        admit_packed_coordinator_prefix(&mut recovery, fs, &primary_root, primary_limits(2))
            .unwrap()
            .0;
    let quota = admit_packed_quota_prefix(&mut recovery, fs, &primary, &quota_root, admission())
        .unwrap()
        .0;
    let mut state = PolicyState::default();
    for transaction in &transactions[..2] {
        let prepared = state
            .prepare(
                transaction.canonical_request(),
                transaction.blob_inventory(),
                transaction.revision(),
            )
            .unwrap();
        assert_eq!(
            PolicyState::result_digest(&prepared),
            transaction.outcome().result_digest
        );
        state.publish_recovered(prepared, transaction).unwrap();
    }
    if let Some(index) = corrupt {
        let (family, profile) = if index < 2 {
            (primary.families()[index], COORDINATOR_PACKED_PROFILE_V1)
        } else {
            (quota.families()[0], COORDINATOR_PACKED_USAGE_PROFILE_V1)
        };
        let location = family
            .root
            .unwrap()
            .resolve(scope(), profile, family.family, primary.anchor().0)
            .unwrap();
        let object = location
            .object
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let directory = fs.open_directory(&fs.root(), name).unwrap();
        let file = fs
            .open_existing(
                &directory,
                &EntryName::new(format!("pack-{object}")).unwrap(),
            )
            .unwrap();
        let offset = location.page * 20545 + 137;
        let mut original = [0];
        fs.read_at(&file, offset, &mut original).unwrap();
        fs.write_at(&file, offset, &[original[0] ^ 1]).unwrap();
        fs.sync_all(&file).unwrap();
    }
    PackedCommitCoordinator::recover_from_admitted_prefixes(
        recovery,
        fs,
        primary,
        quota,
        &primary_root,
        &quota_root,
        state,
        RetentionDays::new(30).unwrap(),
        CoordinatorRecoveryLimits::new(4, 2).unwrap(),
        rebase_limits(),
    )
    .unwrap()
    .0
}

#[test]
fn authorized_packed_cold_suffix_policy_revocation_rejects_old_authority() {
    let (mut fs, name, mut live, _, _) = fixture();
    live.commit(
        &mut fs,
        request(3, 13, b"revoke"),
        &mut clock(2),
        &NeverCancel,
        commit_limits(),
        &mut PageCache::new(64 * 1024).unwrap(),
    )
    .unwrap();
    drop(live);
    fs.restart().unwrap();
    let live = reopen_policy(&mut fs, &name, 3, 7302);
    assert_eq!(live.overlay_counts(), (0, 0));
    let old = kernel(1);
    let current = kernel(2);
    assert!(matches!(
        AuthorizedPackedMetadata::new(&live, &old),
        Err(AuthorizedError::InvalidPolicy)
    ));
    let old_alice = authenticated(&old, 3);
    let alice = authenticated(&current, 3);
    let bob = authenticated(&current, 4);
    let facade = AuthorizedPackedMetadata::new(&live, &current).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    for principal in [&old_alice, &alice] {
        assert_eq!(
            facade.transaction_outcome(
                &mut fs,
                principal,
                TransactionId::from_bytes([12; 16]),
                &mut ScriptedClock::new([])
            ),
            Err(AuthorizedError::Unauthorized)
        );
        assert_eq!(
            facade.committed_blob_usage(&mut fs, principal, 0),
            Err(AuthorizedError::Unauthorized)
        );
    }
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(
        facade.committed_blob_usage(&mut fs, &bob, 2).unwrap(),
        CommittedBlobUsage {
            namespace_bytes: 3,
            principal_bytes: 0,
            owners: 2
        }
    );
}

#[test]
fn authorized_packed_all_observed_read_faults_fail_closed_and_cold_reopen() {
    for query in 0..3 {
        let policy = kernel(1);
        let alice = authenticated(&policy, 3);
        let read = |live: &PolicyLive, fs: &mut Fs| -> Result<(), AuthorizedError> {
            let facade = AuthorizedPackedMetadata::new(live, &policy)?;
            match query {
                0 => {
                    assert!(
                        facade
                            .outcome(
                                fs,
                                &alice,
                                IdempotencyKey::from_bytes([2; 16]),
                                &mut clock(3)
                            )?
                            .is_some()
                    );
                }
                1 => {
                    assert!(
                        facade
                            .transaction_outcome(
                                fs,
                                &alice,
                                TransactionId::from_bytes([12; 16]),
                                &mut clock(3)
                            )?
                            .is_some()
                    );
                }
                _ => {
                    assert_eq!(
                        facade.committed_blob_usage(fs, &alice, 2)?.namespace_bytes,
                        3
                    );
                }
            }
            Ok(())
        };
        let (mut observed_fs, _, observed, _, _) = fixture();
        observed_fs.arm(FaultPlan::default()).unwrap();
        read(&observed, &mut observed_fs).unwrap();
        let boundaries = [
            Operation::OpenExisting,
            Operation::Metadata,
            Operation::ReadAt,
        ]
        .map(|op| (op, observed_fs.operation_count(op)));
        let mut cases = 0;
        for (operation, count) in boundaries {
            for occurrence in 1..=count {
                for action in [
                    FaultAction::Error(AdapterErrorKind::Io),
                    FaultAction::CrashBefore,
                    FaultAction::CrashAfter,
                ] {
                    let (mut fs, name, live, _, _) = fixture();
                    fs.arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                    assert!(read(&live, &mut fs).is_err());
                    assert_eq!(fs.pending_faults(), 0);
                    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
                    drop(live);
                    fs.restart().unwrap();
                    fs.arm(FaultPlan::default()).unwrap();
                    let live = reopen_policy(&mut fs, &name, 2, 7402);
                    read(&live, &mut fs).unwrap();
                    cases += 1;
                }
            }
        }
        assert_eq!(cases, [27, 27, 36][query]);
    }
}

#[test]
fn authorized_packed_corrupt_admitted_metadata_is_neither_absence_nor_zero_usage() {
    for query in 0..3 {
        let (mut fs, name, live, _, _) = fixture();
        drop(live);
        fs.restart().unwrap();
        let live = reopen_policy_with_corruption(&mut fs, &name, 2, 7502, Some(query));
        let policy = kernel(1);
        let alice = authenticated(&policy, 3);
        let denied = authenticated(&policy, 99);
        let facade = AuthorizedPackedMetadata::new(&live, &policy).unwrap();
        fs.arm(FaultPlan::default()).unwrap();
        assert_eq!(
            facade.committed_blob_usage(&mut fs, &denied, 0),
            Err(AuthorizedError::Unauthorized)
        );
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        let result = match query {
            0 => facade
                .outcome(
                    &mut fs,
                    &alice,
                    IdempotencyKey::from_bytes([2; 16]),
                    &mut clock(3),
                )
                .map(|_| ()),
            1 => facade
                .transaction_outcome(
                    &mut fs,
                    &alice,
                    TransactionId::from_bytes([12; 16]),
                    &mut clock(3),
                )
                .map(|_| ()),
            _ => facade.committed_blob_usage(&mut fs, &alice, 2).map(|_| ()),
        };
        assert!(matches!(
            result,
            Err(AuthorizedError::Transaction(TransactionError::Storage(_)))
        ));
        assert_eq!(fs.operation_count(Operation::WriteAt), 0);
    }
}

#[test]
fn authorized_packed_outcome_and_quota_permissions_are_independent() {
    let (mut fs, _, live, _, _) = fixture();
    let policy = kernel(1);
    let outcome_only = authenticated(&policy, 5);
    let quota_only = authenticated(&policy, 6);
    let facade = AuthorizedPackedMetadata::new(&live, &policy).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        facade.committed_blob_usage(&mut fs, &outcome_only, 0),
        Err(AuthorizedError::Unauthorized)
    );
    assert_eq!(
        facade.outcome(
            &mut fs,
            &quota_only,
            IdempotencyKey::from_bytes([2; 16]),
            &mut ScriptedClock::new([])
        ),
        Err(AuthorizedError::Unauthorized)
    );
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(
        facade
            .transaction_outcome(
                &mut fs,
                &outcome_only,
                TransactionId::from_bytes([12; 16]),
                &mut clock(3)
            )
            .unwrap(),
        None
    );
    assert_eq!(
        facade
            .committed_blob_usage(&mut fs, &quota_only, 2)
            .unwrap(),
        CommittedBlobUsage {
            namespace_bytes: 3,
            principal_bytes: 0,
            owners: 2
        }
    );
}
