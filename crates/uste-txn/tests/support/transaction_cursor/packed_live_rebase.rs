use super::*;
use uste_txn::{PackedCoordinatorPublicationState, PackedMetadataRebaseLimits};

impl PackedCoordinatorPublicationState for ReadyCounter {
    fn packed_publication_claims(
        &self,
        selected: NamespaceRef,
        anchor: (CommitRevision, [u8; 32]),
    ) -> Result<PackedRootClaims, ApplyError> {
        // This synthetic fixture advances exactly one unit per certified transaction.
        if selected != scope() || self.state.0 < 0 || self.state.0 as u64 != anchor.0.get() {
            return Err(ApplyError::InvalidRequest);
        }
        Ok(PackedRootClaims {
            revision: anchor.0,
            certificate_digest: anchor.1,
            generation: 1,
            reducer_profile: [41; 32],
            state_commitment_profile: [42; 32],
            state_digest: Self::result_digest(&self.state.0),
        })
    }
}
fn rebase_limits() -> PackedMetadataRebaseLimits {
    let mut staging = limits();
    staging.maximum_owners = 16;
    PackedMetadataRebaseLimits {
        staging,
        maximum_groups: 2,
        maximum_encoded_bytes: 2_000_000,
        certificate_window: 2,
        maximum_publication_attempts: 4,
    }
}
fn append_pair(fs: &mut Fs, live: &mut Live, base: u8) -> (BlobInventory, [TransactionOutcome; 2]) {
    let mut cache = PageCache::new(64 * 1024).unwrap();
    let mut upload = live.start_blob_upload(scope()).unwrap();
    live.write_blob_upload(fs, &mut upload, b"owned").unwrap();
    let reference = live.finish_blob_upload(fs, &mut upload).unwrap();
    let inventory = BlobInventory::new(scope(), [reference]).unwrap();
    let outcomes = [0_u8, 1].map(|offset| {
        live.commit(
            fs,
            TransactionRequest {
                principal: principal(offset),
                blob_inventory: Some(&inventory),
                ..request(
                    base + offset + 20,
                    base + offset + 40,
                    &mutation((base + offset).into(), 1),
                )
            },
            &mut clock(6),
            &NeverCancel,
            commit_limits(),
            &mut cache,
        )
        .unwrap()
    });
    (inventory, outcomes)
}

#[test]
fn packed_rebase_repeated_tiny_overlays_preserve_first_owners_and_cold_retries() {
    let (mut fs, name, mut live, _) = install(parts(3), 2, 1);
    assert!(
        live.rebase_metadata(&mut fs, rebase_limits())
            .unwrap()
            .is_none()
    );
    let mut history = Vec::new();
    for base in [3, 5, 7] {
        let (inventory, outcomes) = append_pair(&mut fs, &mut live, base);
        assert_eq!(live.overlay_counts(), (2, 1));
        let report = live
            .rebase_metadata(&mut fs, rebase_limits())
            .unwrap()
            .unwrap();
        assert_eq!(report.journal.groups, 2);
        assert!(report.journal.encoded_bytes > 0);
        assert_eq!(
            report.primary_root.manifest().claims().revision.get(),
            u64::from(base) + 2
        );
        assert_eq!(
            report.quota_root.manifest().claims().revision.get(),
            u64::from(base) + 2
        );
        assert_eq!(live.overlay_counts(), (0, 0));
        assert!(!live.rebase_required());
        let mut cache = PageCache::new(64 * 1024).unwrap();
        for offset in [0_u8, 1] {
            assert_eq!(
                live.commit(
                    &mut fs,
                    TransactionRequest {
                        principal: principal(offset),
                        blob_inventory: Some(&inventory),
                        ..request(
                            base + offset + 20,
                            base + offset + 40,
                            &mutation((base + offset).into(), 1)
                        )
                    },
                    &mut clock(7),
                    &AlwaysCancel,
                    commit_limits(),
                    &mut cache
                )
                .unwrap(),
                outcomes[usize::from(offset)]
            );
        }
        history.push((base, inventory, outcomes));
    }
    drop(live);
    fs.restart().unwrap();
    let (mut recovery, transactions) = transactions_with_entropy(&mut fs, &name, 9, 5102);
    let primary_roots = recovery
        .discover_packed_roots_at_revision(
            &mut fs,
            COORDINATOR_PACKED_PROFILE_V1,
            CommitRevision::new(9).unwrap(),
            limits().certificates,
            PackedRootDiscoveryLimits::new(4, 4 * 4177).unwrap(),
        )
        .unwrap()
        .0;
    let quota_roots = recovery
        .discover_packed_roots_at_revision(
            &mut fs,
            COORDINATOR_PACKED_USAGE_PROFILE_V1,
            CommitRevision::new(9).unwrap(),
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
        PackedCoordinatorAdmissionLimits {
            maximum_owners: 16,
            ..primary_limits(9)
        },
    )
    .unwrap()
    .0;
    let quota = admit_packed_quota_prefix(
        &mut recovery,
        &mut fs,
        &primary,
        &quota_roots[0],
        PackedQuotaAdmissionLimits {
            maximum_owners: 16,
            ..admission()
        },
    )
    .unwrap()
    .0;
    let maintenance = recovery
        .packed_indexes_with_io(&mut fs, &transactions[8], limits().certificates)
        .unwrap();
    let usage = quota
        .usage(&maintenance, &mut fs, &primary, principal(0), reads())
        .unwrap()
        .0;
    assert_eq!(usage.owners, 5);
    assert_eq!(usage.namespace_bytes, 20);
    assert_eq!(usage.principal_bytes, 20);
    let mut state = CounterState::default();
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
    }
    let state = ReadyCounter {
        state,
        anchor: primary.anchor(),
    };
    let mut live = PackedCommitCoordinator::from_admitted_prefixes(
        recovery,
        primary,
        quota,
        &primary_roots[0],
        &quota_roots[0],
        state,
        RetentionDays::new(30).unwrap(),
        CoordinatorRecoveryLimits::new(2, 1).unwrap(),
    )
    .unwrap();
    let mut cache = PageCache::new(64 * 1024).unwrap();
    for (base, inventory, outcomes) in history {
        for offset in [0_u8, 1] {
            assert_eq!(
                live.commit(
                    &mut fs,
                    TransactionRequest {
                        principal: principal(offset),
                        blob_inventory: Some(&inventory),
                        ..request(
                            base + offset + 20,
                            base + offset + 40,
                            &mutation((base + offset).into(), 1)
                        )
                    },
                    &mut clock(7),
                    &AlwaysCancel,
                    commit_limits(),
                    &mut cache
                )
                .unwrap(),
                outcomes[usize::from(offset)]
            );
        }
    }
}

#[test]
fn packed_rebase_exact_suffix_budget_failure_blocks_fresh_but_not_exact_retry() {
    let (mut fs, _, mut live, _) = install(parts(3), 3, 1);
    append_pair(&mut fs, &mut live, 3);
    let report = live
        .rebase_metadata(&mut fs, rebase_limits())
        .unwrap()
        .unwrap();
    for selected in [
        PackedMetadataRebaseLimits {
            maximum_encoded_bytes: report.journal.encoded_bytes - 1,
            ..rebase_limits()
        },
        PackedMetadataRebaseLimits {
            maximum_groups: 1,
            ..rebase_limits()
        },
        PackedMetadataRebaseLimits {
            certificate_window: 0,
            ..rebase_limits()
        },
        PackedMetadataRebaseLimits {
            maximum_publication_attempts: 0,
            ..rebase_limits()
        },
    ] {
        let (mut fs, _, mut live, _) = install(parts(3), 3, 1);
        let (inventory, outcomes) = append_pair(&mut fs, &mut live, 3);
        assert!(live.rebase_metadata(&mut fs, selected).is_err());
        assert_eq!(live.base_anchor().0.get(), 3);
        assert_eq!(live.overlay_counts(), (2, 1));
        assert!(live.rebase_required());
        let mut cache = PageCache::new(64 * 1024).unwrap();
        assert_eq!(
            live.commit(
                &mut fs,
                request(90, 91, &mutation(5, 1)),
                &mut clock(7),
                &NeverCancel,
                commit_limits(),
                &mut cache
            ),
            Err(TransactionError::ResourceLimit)
        );
        assert_eq!(
            live.commit(
                &mut fs,
                TransactionRequest {
                    blob_inventory: Some(&inventory),
                    ..request(23, 43, &mutation(3, 1))
                },
                &mut clock(7),
                &AlwaysCancel,
                commit_limits(),
                &mut cache
            )
            .unwrap(),
            outcomes[0]
        );
        let exact = PackedMetadataRebaseLimits {
            maximum_encoded_bytes: report.journal.encoded_bytes,
            ..rebase_limits()
        };
        live.rebase_metadata(&mut fs, exact).unwrap().unwrap();
        assert_eq!(live.overlay_counts(), (0, 0));
        live.commit(
            &mut fs,
            request(90, 91, &mutation(5, 1)),
            &mut clock(7),
            &NeverCancel,
            commit_limits(),
            &mut cache,
        )
        .unwrap();
    }
}

#[test]
fn packed_rebase_every_observed_fault_preserves_old_pair_and_cold_suffix_recovery() {
    let (mut observed_fs, _, mut observed, _) = install(parts(3), 3, 1);
    append_pair(&mut observed_fs, &mut observed, 3);
    observed_fs.arm(FaultPlan::default()).unwrap();
    let expected = observed
        .rebase_metadata(&mut observed_fs, rebase_limits())
        .unwrap()
        .unwrap();
    let expected_primary = expected
        .primary_root
        .manifest()
        .families()
        .iter()
        .map(|f| f.commitment)
        .collect::<Vec<_>>();
    let expected_quota = expected
        .quota_root
        .manifest()
        .families()
        .iter()
        .map(|f| f.commitment)
        .collect::<Vec<_>>();
    let boundaries = [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SetLen,
        Operation::SyncAll,
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
                let (mut fs, name, mut live, _) = install(parts(3), 3, 1);
                let (inventory, outcomes) = append_pair(&mut fs, &mut live, 3);
                fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                assert!(
                    live.rebase_metadata(&mut fs, rebase_limits()).is_err(),
                    "{operation:?}/{occurrence}/{action:?}"
                );
                assert_eq!(fs.pending_faults(), 0);
                assert_eq!(live.base_anchor().0.get(), 3);
                assert_eq!(live.overlay_counts(), (2, 1));
                assert!(live.rebase_required());
                let mut cache = PageCache::new(64 * 1024).unwrap();
                let bytes = mutation(3, 1);
                let retry = TransactionRequest {
                    blob_inventory: Some(&inventory),
                    ..request(23, 43, &bytes)
                };
                assert_eq!(
                    live.commit(
                        &mut fs,
                        retry,
                        &mut clock(7),
                        &AlwaysCancel,
                        commit_limits(),
                        &mut cache
                    )
                    .unwrap(),
                    outcomes[0]
                );
                let writes = fs.operation_count(Operation::WriteAt);
                let fresh_result = live.commit(
                    &mut fs,
                    request(90, 91, &mutation(5, 1)),
                    &mut clock(7),
                    &NeverCancel,
                    commit_limits(),
                    &mut cache,
                );
                // Retry/collision metadata reads precede the fresh-write ceiling. A crashed
                // adapter refuses those reads first; neither path may issue a journal write.
                if matches!(action, FaultAction::Error(_)) {
                    assert_eq!(fresh_result, Err(TransactionError::ResourceLimit));
                } else {
                    assert!(matches!(fresh_result, Err(TransactionError::Storage(_))));
                }
                assert_eq!(fs.operation_count(Operation::WriteAt), writes);
                drop(live);
                fs.restart().unwrap();
                fs.arm(FaultPlan::default()).unwrap();
                let (mut recovery, transactions) =
                    transactions_with_entropy(&mut fs, &name, 5, 5302);
                let mut discover = |profile, revision| {
                    recovery
                        .discover_packed_roots_at_revision(
                            &mut fs,
                            profile,
                            CommitRevision::new(revision).unwrap(),
                            limits().certificates,
                            PackedRootDiscoveryLimits::new(4, 4 * 4177).unwrap(),
                        )
                        .unwrap()
                        .0
                };
                let old_primary = discover(COORDINATOR_PACKED_PROFILE_V1, 3);
                let old_quota = discover(COORDINATOR_PACKED_USAGE_PROFILE_V1, 3);
                let new_primary = discover(COORDINATOR_PACKED_PROFILE_V1, 5);
                let new_quota = discover(COORDINATOR_PACKED_USAGE_PROFILE_V1, 5);
                assert_eq!(old_primary.len(), 1);
                assert_eq!(old_quota.len(), 1);
                assert!(new_primary.len() <= 1 && new_quota.len() <= 1);
                for root in &new_primary {
                    let primary = admit_packed_coordinator_prefix(
                        &mut recovery,
                        &mut fs,
                        root,
                        primary_limits(5),
                    )
                    .unwrap()
                    .0;
                    assert_eq!(
                        primary.families().map(|f| f.commitment).as_slice(),
                        expected_primary
                    );
                    for quota in &new_quota {
                        let quota = admit_packed_quota_prefix(
                            &mut recovery,
                            &mut fs,
                            &primary,
                            quota,
                            admission(),
                        )
                        .unwrap()
                        .0;
                        assert_eq!(
                            quota.families().map(|f| f.commitment).as_slice(),
                            expected_quota
                        );
                    }
                }
                let mut primary = admit_packed_coordinator_prefix(
                    &mut recovery,
                    &mut fs,
                    &old_primary[0],
                    primary_limits(3),
                )
                .unwrap()
                .0;
                let mut quota = admit_packed_quota_prefix(
                    &mut recovery,
                    &mut fs,
                    &primary,
                    &old_quota[0],
                    admission(),
                )
                .unwrap()
                .0;
                for transaction in &transactions[3..] {
                    primary = stage_packed_coordinator_prefix(
                        &mut recovery,
                        &mut fs,
                        Some(&primary),
                        transaction,
                        rebase_limits().staging,
                    )
                    .unwrap()
                    .0;
                    quota = stage_packed_quota_prefix(
                        &mut recovery,
                        &mut fs,
                        Some(&quota),
                        &primary,
                        transaction,
                        rebase_limits().staging,
                    )
                    .unwrap()
                    .0;
                }
                assert_eq!(
                    primary.families().map(|f| f.commitment).as_slice(),
                    expected_primary
                );
                assert_eq!(
                    quota.families().map(|f| f.commitment).as_slice(),
                    expected_quota
                );
                let state = ReadyCounter {
                    state: CounterState(5),
                    anchor: primary.anchor(),
                };
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
                let mut live = PackedCommitCoordinator::from_admitted_prefixes(
                    recovery,
                    primary,
                    quota,
                    &primary_root,
                    &quota_root,
                    state,
                    RetentionDays::new(30).unwrap(),
                    CoordinatorRecoveryLimits::new(3, 1).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    live.commit(
                        &mut fs,
                        retry,
                        &mut clock(7),
                        &AlwaysCancel,
                        commit_limits(),
                        &mut cache
                    )
                    .unwrap(),
                    outcomes[0]
                );
                live.commit(
                    &mut fs,
                    request(90, 91, &mutation(5, 1)),
                    &mut clock(7),
                    &NeverCancel,
                    commit_limits(),
                    &mut cache,
                )
                .unwrap();
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 738);
}

#[test]
fn packed_rebase_corrupt_suffix_primary_or_quota_never_installs_partial_pair() {
    for variant in 0..3 {
        let p = parts(3);
        let location = match variant {
            0 => None,
            1 => Some(
                p.primary.families()[2]
                    .root
                    .unwrap()
                    .resolve(
                        scope(),
                        COORDINATOR_PACKED_PROFILE_V1,
                        3,
                        p.primary.anchor().0,
                    )
                    .unwrap(),
            ),
            _ => Some(
                p.quota.families()[0]
                    .root
                    .unwrap()
                    .resolve(
                        scope(),
                        COORDINATOR_PACKED_USAGE_PROFILE_V1,
                        1,
                        p.primary.anchor().0,
                    )
                    .unwrap(),
            ),
        };
        let (mut fs, name, mut live, _) = install(p, 3, 1);
        let (inventory, outcomes) = append_pair(&mut fs, &mut live, 3);
        let (filename, offset) = if let Some(location) = location {
            let object = location
                .object
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            (format!("pack-{object}"), location.page * 20545 + 137)
        } else {
            ("CERTIFICATES".to_owned(), 5 * 4161 + 137)
        };
        let directory = fs.open_directory(&fs.root(), &name).unwrap();
        let file = fs
            .open_existing(&directory, &EntryName::new(filename).unwrap())
            .unwrap();
        let mut original = [0];
        fs.read_at(&file, offset, &mut original).unwrap();
        fs.write_at(&file, offset, &[original[0] ^ 1]).unwrap();
        fs.sync_all(&file).unwrap();
        fs.arm(FaultPlan::default()).unwrap();
        assert!(live.rebase_metadata(&mut fs, rebase_limits()).is_err());
        assert_eq!(live.base_anchor().0.get(), 3);
        assert_eq!(live.overlay_counts(), (2, 1));
        assert!(live.rebase_required());
        let mut cache = PageCache::new(64 * 1024).unwrap();
        assert_eq!(
            live.commit(
                &mut fs,
                TransactionRequest {
                    blob_inventory: Some(&inventory),
                    ..request(23, 43, &mutation(3, 1))
                },
                &mut clock(7),
                &AlwaysCancel,
                commit_limits(),
                &mut cache
            )
            .unwrap(),
            outcomes[0]
        );
        // Restore only the test-injected byte; no production repair substitutes derived data.
        fs.write_at(&file, offset, &original).unwrap();
        fs.sync_all(&file).unwrap();
        live.rebase_metadata(&mut fs, rebase_limits())
            .unwrap()
            .unwrap();
        assert_eq!(live.base_anchor().0.get(), 5);
        assert_eq!(live.overlay_counts(), (0, 0));
    }
}

struct RejectPublication(ReadyCounter, u8);
impl TransactionState for RejectPublication {
    type Prepared = i64;
    type Snapshot = CounterState;
    fn prepare(
        &self,
        bytes: &[u8],
        inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<i64, ApplyError> {
        self.0.prepare(bytes, inventory, revision)
    }
    fn result_digest(prepared: &i64) -> [u8; 32] {
        ReadyCounter::result_digest(prepared)
    }
    fn publish(&mut self, prepared: i64) {
        self.0.publish(prepared);
    }
    fn snapshot(&self) -> CounterState {
        self.0.snapshot()
    }
}
impl PackedCoordinatorState for RejectPublication {
    fn validate_packed_metadata(
        &self,
        scope: NamespaceRef,
        claims: PackedRootClaims,
    ) -> Result<(), ApplyError> {
        self.0.validate_packed_metadata(scope, claims)
    }
}
impl PackedCoordinatorPublicationState for RejectPublication {
    fn packed_publication_claims(
        &self,
        scope: NamespaceRef,
        anchor: (CommitRevision, [u8; 32]),
    ) -> Result<PackedRootClaims, ApplyError> {
        if self.1 == 0 {
            return Err(ApplyError::InvalidRequest);
        }
        let mut claims = self.0.packed_publication_claims(scope, anchor)?;
        match self.1 {
            1 => claims.revision = CommitRevision::FIRST,
            2 => claims.reducer_profile[0] ^= 1,
            _ => claims.state_commitment_profile[0] ^= 1,
        }
        Ok(claims)
    }
}
#[test]
fn packed_rebase_ready_domain_and_profile_rejection_precede_io() {
    for variant in 0..4 {
        let p = parts(3);
        let mut fs = p.fs;
        let mut live = PackedCommitCoordinator::from_admitted_prefixes(
            p.recovery,
            p.primary,
            p.quota,
            &p.primary_root,
            &p.quota_root,
            RejectPublication(p.state, variant),
            RetentionDays::new(30).unwrap(),
            CoordinatorRecoveryLimits::new(2, 1).unwrap(),
        )
        .unwrap();
        let mut cache = PageCache::new(64 * 1024).unwrap();
        let bytes = mutation(3, 1);
        let fresh = request(20, 30, &bytes);
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
        fs.arm(FaultPlan::default()).unwrap();
        assert!(live.rebase_metadata(&mut fs, rebase_limits()).is_err());
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
        assert_eq!(live.base_anchor().0.get(), 3);
        assert_eq!(live.overlay_counts(), (1, 0));
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
