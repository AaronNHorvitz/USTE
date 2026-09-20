use super::*;
#[path = "packed_read_only.rs"]
mod read_only;
#[path = "packed_live_rebase.rs"]
mod rebase;
use uste_storage::PageCache;
use uste_txn::{
    DiskCommitCheck, PackedCommitCoordinator, PackedCommitLimits, PackedCoordinatorState,
};

struct ReadyCounter {
    state: CounterState,
    anchor: (CommitRevision, [u8; 32]),
}
impl TransactionState for ReadyCounter {
    type Prepared = i64;
    type Snapshot = CounterState;
    fn prepare(
        &self,
        bytes: &[u8],
        inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<i64, ApplyError> {
        self.state.prepare(bytes, inventory, revision)
    }
    fn result_digest(prepared: &i64) -> [u8; 32] {
        CounterState::result_digest(prepared)
    }
    fn publish(&mut self, prepared: i64) {
        self.state.publish(prepared);
    }
    fn snapshot(&self) -> CounterState {
        self.state.clone()
    }
}
impl PackedCoordinatorState for ReadyCounter {
    fn validate_packed_metadata(
        &self,
        selected: NamespaceRef,
        claims: PackedRootClaims,
    ) -> Result<(), ApplyError> {
        if selected != scope()
            || (claims.revision, claims.certificate_digest) != self.anchor
            || claims.reducer_profile != [41; 32]
            || claims.state_commitment_profile != [42; 32]
            || claims.state_digest != Self::result_digest(&self.state.0)
        {
            return Err(ApplyError::InvalidRequest);
        }
        Ok(())
    }
}
impl ExternallyPreparedTransactionState for ReadyCounter {
    fn validate_external_prepared(
        &self,
        bytes: &[u8],
        inventory: Option<&BlobInventory>,
        revision: CommitRevision,
        prepared: &i64,
    ) -> Result<(), ApplyError> {
        if self.prepare(bytes, inventory, revision)? != *prepared {
            return Err(ApplyError::Conflict);
        }
        Ok(())
    }
}
type Live = PackedCommitCoordinator<ReadyCounter, Fs, TestEnvelope, CounterEntropy, CounterEntropy>;
struct Parts {
    fs: Fs,
    name: EntryName,
    recovery: Recovery,
    primary: PackedCoordinatorPrefix,
    quota: PackedQuotaPrefix,
    primary_root: CertifiedPackedRoot,
    quota_root: CertifiedPackedRoot,
    state: ReadyCounter,
    transactions: Vec<uste_txn::RecoveredFrontierTransaction>,
}
fn parts(count: u8) -> Parts {
    let (mut fs, name) = fixture_count(count);
    let (mut recovery, transactions) = transactions(&mut fs, &name, count.into());
    let mut state = CounterState::default();
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
            CounterState::result_digest(&prepared),
            transaction.outcome().result_digest
        );
        state.publish(prepared);
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
    }
    let primary = primary.unwrap();
    let quota = quota.unwrap();
    let claims = PackedRootClaims {
        revision: primary.anchor().0,
        certificate_digest: primary.anchor().1,
        generation: 1,
        reducer_profile: [41; 32],
        state_commitment_profile: [42; 32],
        state_digest: CounterState::result_digest(&state.0),
    };
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
    let state = ReadyCounter {
        state,
        anchor: primary.anchor(),
    };
    Parts {
        fs,
        name,
        recovery,
        primary,
        quota,
        primary_root,
        quota_root,
        state,
        transactions,
    }
}
fn install(
    parts: Parts,
    outcomes: usize,
    owners: usize,
) -> (
    Fs,
    EntryName,
    Live,
    Vec<uste_txn::RecoveredFrontierTransaction>,
) {
    let live = PackedCommitCoordinator::from_admitted_prefixes(
        parts.recovery,
        parts.primary,
        parts.quota,
        &parts.primary_root,
        &parts.quota_root,
        parts.state,
        RetentionDays::new(30).unwrap(),
        CoordinatorRecoveryLimits::new(outcomes, owners).unwrap(),
    )
    .unwrap();
    (parts.fs, parts.name, live, parts.transactions)
}
fn commit_limits() -> PackedCommitLimits {
    PackedCommitLimits {
        lookup: reads(),
        storage: None,
    }
}

#[test]
fn owner_work_packed_handoff_keeps_exact_vault_counters_without_io() {
    let mut parts = parts(5);
    let work = parts.recovery.vault_decrypt_report().unwrap();
    let encryption = parts.recovery.vault_encrypt_report().unwrap();
    assert!(encryption.successful_calls > 0 && encryption.produced_encoded_bytes > 0);
    let nonces = parts.recovery.vault_nonce_report().unwrap();
    assert!(work.successful_calls > 0 && nonces.issued_nonces > 0);
    parts.fs.arm(FaultPlan::default()).unwrap();
    let (fs, _, live, _) = install(parts, 1, 0);
    for _ in 0..3 {
        assert_eq!(live.vault_decrypt_report().unwrap(), work);
        assert_eq!(live.vault_encrypt_report().unwrap(), encryption);
        assert_eq!(live.vault_nonce_report().unwrap(), nonces);
    }
    for operation in [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
        Operation::CreateNew,
        Operation::WriteAt,
    ] {
        assert_eq!(fs.operation_count(operation), 0);
    }
}

#[test]
fn packed_live_base_retries_expiry_collision_cancellation_and_bounded_overlays() {
    let (mut fs, _, mut live, transactions) = install(parts(5), 1, 0);
    let mut cache = PageCache::new(64 * 1024).unwrap();
    assert_eq!(live.overlay_counts(), (0, 0));
    fs.arm(FaultPlan::default()).unwrap();
    for (index, transaction) in transactions.iter().enumerate() {
        let original = TransactionRequest {
            principal: if index == 1 || index == 4 {
                principal(1)
            } else {
                principal(0)
            },
            blob_inventory: transaction.blob_inventory(),
            ..request(
                index as u8 + 1,
                index as u8 + 11,
                transaction.canonical_request(),
            )
        };
        assert_eq!(
            live.commit(
                &mut fs,
                original,
                &mut clock(6),
                &AlwaysCancel,
                commit_limits(),
                &mut cache
            )
            .unwrap(),
            transaction.outcome()
        );
        assert_eq!(
            live.check_commit(
                &mut fs,
                original,
                &mut clock(6),
                &AlwaysCancel,
                commit_limits(),
                &mut cache
            )
            .unwrap(),
            DiskCommitCheck::Retry(transaction.outcome())
        );
        assert_eq!(
            live.commit(
                &mut fs,
                original,
                &mut clock(40),
                &NeverCancel,
                commit_limits(),
                &mut cache
            ),
            Err(TransactionError::IdempotencyExpired)
        );
        let changed = TransactionRequest {
            transaction_id: TransactionId::from_bytes([99; 16]),
            ..original
        };
        assert_eq!(
            live.commit(
                &mut fs,
                changed,
                &mut clock(6),
                &NeverCancel,
                commit_limits(),
                &mut cache
            ),
            Err(TransactionError::Conflict)
        );
        let collision = TransactionRequest {
            principal: PrincipalDigest::from_bytes([99; 32]),
            idempotency_key: IdempotencyKey::from_bytes([99; 16]),
            ..original
        };
        assert_eq!(
            live.commit(
                &mut fs,
                collision,
                &mut clock(6),
                &NeverCancel,
                commit_limits(),
                &mut cache
            ),
            Err(TransactionError::Conflict)
        );
    }
    let bytes = mutation(5, 1);
    let fresh = request(20, 30, &bytes);
    assert_eq!(
        live.commit(
            &mut fs,
            fresh,
            &mut clock(6),
            &AlwaysCancel,
            commit_limits(),
            &mut cache
        ),
        Err(TransactionError::Cancelled)
    );
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
    assert_eq!(
        live.check_commit(
            &mut fs,
            fresh,
            &mut clock(6),
            &NeverCancel,
            commit_limits(),
            &mut cache
        )
        .unwrap(),
        DiskCommitCheck::Ready {
            revision: CommitRevision::new(6).unwrap()
        }
    );
    let outcome = live
        .commit(
            &mut fs,
            fresh,
            &mut clock(6),
            &NeverCancel,
            commit_limits(),
            &mut cache,
        )
        .unwrap();
    assert_eq!(live.state().unwrap().state.0, 6);
    assert_eq!(live.overlay_counts(), (1, 0));
    assert_eq!(live.base_anchor().0.get(), 5);
    assert_eq!(
        live.commit(
            &mut fs,
            request(21, 31, &mutation(6, 1)),
            &mut clock(7),
            &NeverCancel,
            commit_limits(),
            &mut cache
        ),
        Err(TransactionError::ResourceLimit)
    );
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        live.commit(
            &mut fs,
            fresh,
            &mut clock(7),
            &AlwaysCancel,
            commit_limits(),
            &mut cache
        )
        .unwrap(),
        outcome
    );
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
}

#[test]
fn packed_live_install_rejects_mismatched_domain_claims_and_foreign_receipts() {
    for variant in 0..7 {
        let mut p = parts(3);
        match variant {
            0 => p.state.state.0 += 1,
            1 => p.state.anchor.0 = CommitRevision::FIRST,
            2..=4 => {
                let mut claims = p.quota_root.manifest().claims();
                match variant {
                    2 => claims.reducer_profile[0] ^= 1,
                    3 => claims.state_commitment_profile[0] ^= 1,
                    _ => claims.state_digest[0] ^= 1,
                }
                p.quota_root = p
                    .recovery
                    .publish_recovered_packed_root(
                        &mut p.fs,
                        COORDINATOR_PACKED_USAGE_PROFILE_V1,
                        claims,
                        &p.quota.families(),
                        4,
                    )
                    .unwrap();
            }
            5 => p.primary_root = parts(3).primary_root,
            _ => p.quota = parts(3).quota,
        }
        p.fs.arm(FaultPlan::default()).unwrap();
        let result = PackedCommitCoordinator::from_admitted_prefixes(
            p.recovery,
            p.primary,
            p.quota,
            &p.primary_root,
            &p.quota_root,
            p.state,
            RetentionDays::new(30).unwrap(),
            CoordinatorRecoveryLimits::new(1, 1).unwrap(),
        );
        assert!(result.is_err(), "variant {variant}");
        assert_eq!(p.fs.operation_count(Operation::ReadAt), 0);
        assert_eq!(p.fs.operation_count(Operation::CreateNew), 0);
    }
}

#[test]
fn packed_live_first_owner_reuse_new_owner_limit_and_reference_mismatch() {
    for owner_limit in [0, 1] {
        let (mut fs, _, mut live, transactions) = install(parts(5), 3, owner_limit);
        let mut cache = PageCache::new(64 * 1024).unwrap();
        let inventory = transactions[4].blob_inventory().unwrap();
        let original = *inventory
            .references()
            .iter()
            .find(|r| r.byte_len() > 0)
            .unwrap();
        let mut digest = original.content_digest();
        digest[0] ^= 1;
        let altered = uste_storage::BlobReference::new(
            scope(),
            original.id(),
            original.byte_len(),
            original.chunk_count(),
            digest,
        )
        .unwrap();
        let altered_inventory = BlobInventory::new(scope(), [altered]).unwrap();
        fs.arm(FaultPlan::default()).unwrap();
        assert_eq!(
            live.commit(
                &mut fs,
                TransactionRequest {
                    blob_inventory: Some(&altered_inventory),
                    ..request(50, 60, &mutation(5, 1))
                },
                &mut clock(6),
                &NeverCancel,
                commit_limits(),
                &mut cache
            ),
            Err(TransactionError::IntegrityFailure)
        );
        assert_eq!(fs.operation_count(Operation::WriteAt), 0);
        let bytes = mutation(5, 1);
        let reused = TransactionRequest {
            principal: PrincipalDigest::from_bytes([80; 32]),
            blob_inventory: Some(inventory),
            ..request(20, 30, &bytes)
        };
        live.commit(
            &mut fs,
            reused,
            &mut clock(6),
            &NeverCancel,
            commit_limits(),
            &mut cache,
        )
        .unwrap();
        assert_eq!(live.overlay_counts(), (1, 0));
        let mut upload = live.start_blob_upload(scope()).unwrap();
        live.write_blob_upload(&mut fs, &mut upload, b"new owner")
            .unwrap();
        let reference = live.finish_blob_upload(&mut fs, &mut upload).unwrap();
        let new_inventory = BlobInventory::new(scope(), [reference]).unwrap();
        let bytes = mutation(6, 1);
        let fresh = TransactionRequest {
            blob_inventory: Some(&new_inventory),
            ..request(21, 31, &bytes)
        };
        fs.arm(FaultPlan::default()).unwrap();
        let result = live.commit(
            &mut fs,
            fresh,
            &mut clock(7),
            &NeverCancel,
            commit_limits(),
            &mut cache,
        );
        if owner_limit == 0 {
            assert_eq!(result, Err(TransactionError::ResourceLimit));
            assert_eq!(fs.operation_count(Operation::WriteAt), 0);
            assert_eq!(live.state().unwrap().state.0, 6);
        } else {
            let outcome = result.unwrap();
            assert_eq!(live.overlay_counts(), (2, 1));
            assert_eq!(
                live.commit(
                    &mut fs,
                    fresh,
                    &mut clock(7),
                    &AlwaysCancel,
                    commit_limits(),
                    &mut cache
                )
                .unwrap(),
                outcome
            );
        }
    }
}

#[test]
fn packed_live_every_observed_commit_fault_preserves_cold_exact_retry() {
    let (mut observed_fs, _, mut observed, _) = install(parts(3), 2, 1);
    let mut cache = PageCache::new(64 * 1024).unwrap();
    let bytes = mutation(3, 1);
    let fresh = request(20, 30, &bytes);
    observed_fs.arm(FaultPlan::default()).unwrap();
    let expected = observed
        .commit(
            &mut observed_fs,
            fresh,
            &mut clock(6),
            &NeverCancel,
            commit_limits(),
            &mut cache,
        )
        .unwrap();
    let boundaries = [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SetLen,
        Operation::SyncAll,
        Operation::SyncData,
        Operation::SyncDirectory,
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
                let (mut fs, name, mut live, _) = install(parts(3), 2, 1);
                fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                let result = live.commit(
                    &mut fs,
                    fresh,
                    &mut clock(6),
                    &NeverCancel,
                    commit_limits(),
                    &mut cache,
                );
                assert_eq!(fs.pending_faults(), 0);
                assert!(result.is_err(), "{operation:?}/{occurrence}/{action:?}");
                if result == Err(TransactionError::OutcomeUnknown) {
                    assert_eq!(
                        live.vault_nonce_report(),
                        Err(TransactionError::OutcomeUnknown)
                    );
                    assert!(matches!(
                        live.packed_index_reader(),
                        Err(TransactionError::OutcomeUnknown)
                    ));
                    assert!(matches!(
                        live.state(),
                        Err(TransactionError::OutcomeUnknown)
                    ));
                    assert_eq!(
                        live.commit(
                            &mut fs,
                            fresh,
                            &mut clock(6),
                            &NeverCancel,
                            commit_limits(),
                            &mut cache
                        ),
                        Err(TransactionError::OutcomeUnknown)
                    );
                }
                drop(live);
                fs.restart().unwrap();
                fs.arm(FaultPlan::default()).unwrap();
                // Independent small-fixture full replay is the reference oracle, not the live path.
                let (mut reference, report) = CommitCoordinator::open(
                    &mut fs,
                    &name,
                    scope(),
                    RetentionDays::new(30).unwrap(),
                    CounterEntropy(4101),
                    CounterEntropy(4102),
                    &mut TestKeyAdapter,
                    CounterState::default(),
                )
                .unwrap();
                let revision = report.frontier.unwrap().get();
                assert!(revision == 3 || revision == 4);
                assert_eq!(reference.read_view().unwrap().state().0, revision as i64);
                assert_eq!(
                    reference
                        .commit(&mut fs, fresh, &mut clock(6), &NeverCancel)
                        .unwrap(),
                    expected
                );
                drop(reference);
                let (mut recovery, _) = transactions_with_entropy(&mut fs, &name, 4, 4202);
                let primary_roots = recovery
                    .discover_packed_roots_at_revision(
                        &mut fs,
                        COORDINATOR_PACKED_PROFILE_V1,
                        CommitRevision::new(3).unwrap(),
                        limits().certificates,
                        PackedRootDiscoveryLimits::new(4, 4 * 4177).unwrap(),
                    )
                    .unwrap()
                    .0;
                let quota_roots = recovery
                    .discover_packed_roots_at_revision(
                        &mut fs,
                        COORDINATOR_PACKED_USAGE_PROFILE_V1,
                        CommitRevision::new(3).unwrap(),
                        limits().certificates,
                        PackedRootDiscoveryLimits::new(4, 4 * 4177).unwrap(),
                    )
                    .unwrap()
                    .0;
                assert_eq!(primary_roots.len(), 1);
                assert_eq!(quota_roots.len(), 1);
                let primary = admit_packed_coordinator_prefix(
                    &mut recovery,
                    &mut fs,
                    &primary_roots[0],
                    primary_limits(3),
                )
                .unwrap()
                .0;
                admit_packed_quota_prefix(
                    &mut recovery,
                    &mut fs,
                    &primary,
                    &quota_roots[0],
                    admission(),
                )
                .unwrap();
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 66);
}

#[test]
fn packed_live_prepared_binding_and_second_cancellation_precede_writes() {
    let (mut fs, _, mut live, _) = install(parts(1), 1, 0);
    let mut cache = PageCache::new(64 * 1024).unwrap();
    let bytes = mutation(1, 1);
    let fresh = request(20, 30, &bytes);
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        live.commit_prepared(
            &mut fs,
            fresh,
            99,
            &mut clock(6),
            &NeverCancel,
            commit_limits(),
            &mut cache
        ),
        Err(TransactionError::Conflict)
    );
    assert_eq!(
        live.commit_prepared(
            &mut fs,
            fresh,
            2,
            &mut clock(6),
            &CancelOnCall::new(2),
            commit_limits(),
            &mut cache
        ),
        Err(TransactionError::Cancelled)
    );
    assert_eq!(live.state().unwrap().state.0, 1);
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
    let outcome = live
        .commit_prepared(
            &mut fs,
            fresh,
            2,
            &mut clock(6),
            &NeverCancel,
            commit_limits(),
            &mut cache,
        )
        .unwrap();
    // Exact retry does not invoke preparation, even when the supplied prepared value is stale.
    assert_eq!(
        live.commit_prepared(
            &mut fs,
            fresh,
            99,
            &mut clock(6),
            &AlwaysCancel,
            commit_limits(),
            &mut cache
        )
        .unwrap(),
        outcome
    );
}

#[test]
fn packed_live_corruption_and_lookup_limit_cannot_become_a_metadata_miss() {
    for family in 0..3 {
        let p = parts(5);
        let location = p.primary.families()[family]
            .root
            .unwrap()
            .resolve(
                scope(),
                COORDINATOR_PACKED_PROFILE_V1,
                family as u8 + 1,
                p.primary.anchor().0,
            )
            .unwrap();
        let (mut fs, name, mut live, transactions) = install(p, 1, 0);
        let mut cache = PageCache::new(64 * 1024).unwrap();
        let transaction = &transactions[4];
        let bytes = mutation(5, 1);
        let fresh = TransactionRequest {
            blob_inventory: (family == 2).then(|| transaction.blob_inventory().unwrap()),
            ..request(20, 30, &bytes)
        };
        let narrow = PackedCommitLimits {
            lookup: TreeLookupLimits {
                maximum_pages: 0,
                ..reads()
            },
            storage: None,
        };
        fs.arm(FaultPlan::default()).unwrap();
        assert!(
            live.commit(
                &mut fs,
                fresh,
                &mut clock(6),
                &NeverCancel,
                narrow,
                &mut cache
            )
            .is_err()
        );
        assert_eq!(fs.operation_count(Operation::WriteAt), 0);
        let object = location
            .object
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let directory = fs.open_directory(&fs.root(), &name).unwrap();
        let file = fs
            .open_existing(
                &directory,
                &EntryName::new(format!("pack-{object}")).unwrap(),
            )
            .unwrap();
        let offset = location.page * 20545 + 137;
        let mut byte = [0];
        fs.read_at(&file, offset, &mut byte).unwrap();
        fs.write_at(&file, offset, &[byte[0] ^ 1]).unwrap();
        fs.sync_all(&file).unwrap();
        fs.arm(FaultPlan::default()).unwrap();
        assert!(
            live.commit(
                &mut fs,
                fresh,
                &mut clock(6),
                &NeverCancel,
                commit_limits(),
                &mut cache
            )
            .is_err()
        );
        assert_eq!(fs.operation_count(Operation::WriteAt), 0);
        assert_eq!(live.state().unwrap().state.0, 5);
        assert_eq!(live.overlay_counts(), (0, 0));
    }
}
