use super::*;
use uste_txn::PackedCoordinatorRecoveryState;

impl PackedCoordinatorRecoveryState for ReadyCounter {
    fn publish_recovered(
        &mut self,
        prepared: i64,
        transaction: &uste_txn::RecoveredFrontierTransaction,
    ) -> Result<(), ApplyError> {
        if self.anchor.0.checked_next().ok() != Some(transaction.revision()) {
            return Err(ApplyError::Conflict);
        }
        self.state.publish(prepared);
        self.anchor = (transaction.revision(), *transaction.certificate_digest());
        Ok(())
    }
}
fn lagging() -> (Parts, BlobInventory, [TransactionOutcome; 2]) {
    let (mut fs, name, mut live, _) = install(parts(3), 2, 1);
    let (inventory, outcomes) = append_pair(&mut fs, &mut live, 3);
    drop(live);
    fs.restart().unwrap();
    (cold_parts(fs, name, 6102), inventory, outcomes)
}
fn cold_parts(mut fs: Fs, name: EntryName, entropy: u64) -> Parts {
    let (mut recovery, transactions) = transactions_with_entropy(&mut fs, &name, 5, entropy);
    let mut discover = |profile| {
        recovery
            .discover_packed_roots_at_revision(
                &mut fs,
                profile,
                CommitRevision::new(3).unwrap(),
                limits().certificates,
                PackedRootDiscoveryLimits::new(4, 4 * 4177).unwrap(),
            )
            .unwrap()
            .0
    };
    let primary_root = discover(COORDINATOR_PACKED_PROFILE_V1).pop().unwrap();
    let quota_root = discover(COORDINATOR_PACKED_USAGE_PROFILE_V1).pop().unwrap();
    let primary =
        admit_packed_coordinator_prefix(&mut recovery, &mut fs, &primary_root, primary_limits(3))
            .unwrap()
            .0;
    let quota =
        admit_packed_quota_prefix(&mut recovery, &mut fs, &primary, &quota_root, admission())
            .unwrap()
            .0;
    let mut state = CounterState::default();
    for transaction in &transactions[..3] {
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

#[test]
fn packed_recovery_streams_paired_suffix_without_overlay_maps_and_retries_exactly() {
    for window in [1, 2, 64] {
        let (p, inventory, outcomes) = lagging();
        let mut fs = p.fs;
        let (mut live, report) = PackedCommitCoordinator::recover_from_admitted_prefixes(
            p.recovery,
            &mut fs,
            p.primary,
            p.quota,
            &p.primary_root,
            &p.quota_root,
            p.state,
            RetentionDays::new(30).unwrap(),
            CoordinatorRecoveryLimits::new(0, 0).unwrap(),
            PackedMetadataRebaseLimits {
                certificate_window: window,
                ..rebase_limits()
            },
        )
        .unwrap();
        let report = report.unwrap();
        assert_eq!(report.journal.groups, 2);
        assert_eq!(live.base_anchor().0.get(), 5);
        assert_eq!(live.overlay_counts(), (0, 0));
        assert_eq!(live.state().unwrap().state.0, 5);
        let mut cache = PageCache::new(64 * 1024).unwrap();
        for offset in [0_u8, 1] {
            assert_eq!(
                live.commit(
                    &mut fs,
                    TransactionRequest {
                        principal: principal(offset),
                        blob_inventory: Some(&inventory),
                        ..request(
                            23 + offset,
                            43 + offset,
                            &mutation(i64::from(3 + offset), 1)
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
        drop(live);
        fs.restart().unwrap();
        let (mut recovery, transactions) = transactions_with_entropy(&mut fs, &p.name, 5, 6202);
        let mut discover = |profile| {
            recovery
                .discover_packed_roots_at_revision(
                    &mut fs,
                    profile,
                    CommitRevision::new(5).unwrap(),
                    limits().certificates,
                    PackedRootDiscoveryLimits::new(4, 4 * 4177).unwrap(),
                )
                .unwrap()
                .0
        };
        let primary_root = discover(COORDINATOR_PACKED_PROFILE_V1).pop().unwrap();
        let quota_root = discover(COORDINATOR_PACKED_USAGE_PROFILE_V1).pop().unwrap();
        let primary = admit_packed_coordinator_prefix(
            &mut recovery,
            &mut fs,
            &primary_root,
            primary_limits(5),
        )
        .unwrap()
        .0;
        let quota =
            admit_packed_quota_prefix(&mut recovery, &mut fs, &primary, &quota_root, admission())
                .unwrap()
                .0;
        assert_eq!(
            primary.families().map(|f| f.commitment).as_slice(),
            report
                .primary_root
                .manifest()
                .families()
                .iter()
                .map(|f| f.commitment)
                .collect::<Vec<_>>()
        );
        let maintenance = recovery
            .packed_indexes_with_io(&mut fs, &transactions[4], limits().certificates)
            .unwrap();
        let usage = quota
            .usage(&maintenance, &mut fs, &primary, principal(0), reads())
            .unwrap()
            .0;
        assert_eq!(usage.owners, 3);
        assert_eq!(usage.namespace_bytes, 10);
        assert_eq!(usage.principal_bytes, 10);
        let state = ReadyCounter {
            state: CounterState(5),
            anchor: primary.anchor(),
        };
        fs.arm(FaultPlan::default()).unwrap();
        let (_, report) = PackedCommitCoordinator::recover_from_admitted_prefixes(
            recovery,
            &mut fs,
            primary,
            quota,
            &primary_root,
            &quota_root,
            state,
            RetentionDays::new(30).unwrap(),
            CoordinatorRecoveryLimits::new(0, 0).unwrap(),
            PackedMetadataRebaseLimits {
                maximum_groups: 0,
                maximum_encoded_bytes: 0,
                certificate_window: 0,
                maximum_publication_attempts: 0,
                ..rebase_limits()
            },
        )
        .unwrap();
        assert!(report.is_none());
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    }
}

#[test]
fn packed_recovery_exact_range_limits_and_late_failure_leave_only_old_roots() {
    let (p, _, _) = lagging();
    let mut fs = p.fs;
    let (_, report) = PackedCommitCoordinator::recover_from_admitted_prefixes(
        p.recovery,
        &mut fs,
        p.primary,
        p.quota,
        &p.primary_root,
        &p.quota_root,
        p.state,
        RetentionDays::new(30).unwrap(),
        CoordinatorRecoveryLimits::new(0, 0).unwrap(),
        rebase_limits(),
    )
    .unwrap();
    let bytes = report.unwrap().journal.encoded_bytes;
    for selected in [
        PackedMetadataRebaseLimits {
            maximum_encoded_bytes: bytes - 1,
            ..rebase_limits()
        },
        PackedMetadataRebaseLimits {
            maximum_groups: 1,
            ..rebase_limits()
        },
        PackedMetadataRebaseLimits {
            certificate_window: 65,
            ..rebase_limits()
        },
        PackedMetadataRebaseLimits {
            maximum_publication_attempts: 0,
            ..rebase_limits()
        },
    ] {
        let (p, inventory, outcomes) = lagging();
        let mut fs = p.fs;
        let result = PackedCommitCoordinator::recover_from_admitted_prefixes(
            p.recovery,
            &mut fs,
            p.primary,
            p.quota,
            &p.primary_root,
            &p.quota_root,
            p.state,
            RetentionDays::new(30).unwrap(),
            CoordinatorRecoveryLimits::new(0, 0).unwrap(),
            selected,
        );
        assert!(result.is_err());
        let p = cold_parts(fs, p.name, 6302);
        let mut fs = p.fs;
        for profile in [
            COORDINATOR_PACKED_PROFILE_V1,
            COORDINATOR_PACKED_USAGE_PROFILE_V1,
        ] {
            assert!(
                p.recovery
                    .discover_packed_roots_at_revision(
                        &mut fs,
                        profile,
                        CommitRevision::new(5).unwrap(),
                        limits().certificates,
                        PackedRootDiscoveryLimits::new(4, 4 * 4177).unwrap()
                    )
                    .unwrap()
                    .0
                    .is_empty()
            );
        }
        let (mut live, report) = PackedCommitCoordinator::recover_from_admitted_prefixes(
            p.recovery,
            &mut fs,
            p.primary,
            p.quota,
            &p.primary_root,
            &p.quota_root,
            p.state,
            RetentionDays::new(30).unwrap(),
            CoordinatorRecoveryLimits::new(0, 0).unwrap(),
            PackedMetadataRebaseLimits {
                maximum_encoded_bytes: bytes,
                ..rebase_limits()
            },
        )
        .unwrap();
        assert_eq!(report.unwrap().journal.encoded_bytes, bytes);
        assert_eq!(live.overlay_counts(), (0, 0));
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
    }
}

#[test]
fn packed_recovery_every_observed_fault_returns_no_live_state_and_restarts() {
    let (p, _, _) = lagging();
    let mut observed_fs = p.fs;
    observed_fs.arm(FaultPlan::default()).unwrap();
    let (observed, report) = PackedCommitCoordinator::recover_from_admitted_prefixes(
        p.recovery,
        &mut observed_fs,
        p.primary,
        p.quota,
        &p.primary_root,
        &p.quota_root,
        p.state,
        RetentionDays::new(30).unwrap(),
        CoordinatorRecoveryLimits::new(0, 0).unwrap(),
        rebase_limits(),
    )
    .unwrap();
    let report = report.unwrap();
    let expected_primary = report
        .primary_root
        .manifest()
        .families()
        .iter()
        .map(|f| f.commitment)
        .collect::<Vec<_>>();
    let expected_quota = report
        .quota_root
        .manifest()
        .families()
        .iter()
        .map(|f| f.commitment)
        .collect::<Vec<_>>();
    assert_eq!(observed.overlay_counts(), (0, 0));
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
                let (p, inventory, outcomes) = lagging();
                let mut fs = p.fs;
                fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                let result = PackedCommitCoordinator::recover_from_admitted_prefixes(
                    p.recovery,
                    &mut fs,
                    p.primary,
                    p.quota,
                    &p.primary_root,
                    &p.quota_root,
                    p.state,
                    RetentionDays::new(30).unwrap(),
                    CoordinatorRecoveryLimits::new(0, 0).unwrap(),
                    rebase_limits(),
                );
                assert!(result.is_err(), "{operation:?}/{occurrence}/{action:?}");
                assert_eq!(fs.pending_faults(), 0);
                fs.restart().unwrap();
                fs.arm(FaultPlan::default()).unwrap();
                let p = cold_parts(fs, p.name, 6402);
                let mut fs = p.fs;
                for profile in [
                    COORDINATOR_PACKED_PROFILE_V1,
                    COORDINATOR_PACKED_USAGE_PROFILE_V1,
                ] {
                    // No revision-four manifest may have been published, even after staging it.
                    assert!(
                        p.recovery
                            .discover_packed_roots_at_revision(
                                &mut fs,
                                profile,
                                CommitRevision::new(4).unwrap(),
                                limits().certificates,
                                PackedRootDiscoveryLimits::new(4, 4 * 4177).unwrap()
                            )
                            .unwrap()
                            .0
                            .is_empty()
                    );
                }
                let (mut live, report) = PackedCommitCoordinator::recover_from_admitted_prefixes(
                    p.recovery,
                    &mut fs,
                    p.primary,
                    p.quota,
                    &p.primary_root,
                    &p.quota_root,
                    p.state,
                    RetentionDays::new(30).unwrap(),
                    CoordinatorRecoveryLimits::new(0, 0).unwrap(),
                    rebase_limits(),
                )
                .unwrap();
                let report = report.unwrap();
                assert_eq!(
                    report
                        .primary_root
                        .manifest()
                        .families()
                        .iter()
                        .map(|f| f.commitment)
                        .collect::<Vec<_>>(),
                    expected_primary
                );
                assert_eq!(
                    report
                        .quota_root
                        .manifest()
                        .families()
                        .iter()
                        .map(|f| f.commitment)
                        .collect::<Vec<_>>(),
                    expected_quota
                );
                assert_eq!(live.overlay_counts(), (0, 0));
                assert_eq!(live.state().unwrap().state.0, 5);
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
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 702);
}

struct RejectRecovery {
    inner: ReadyCounter,
    mode: u8,
    calls: std::rc::Rc<Cell<u64>>,
}
impl TransactionState for RejectRecovery {
    type Prepared = (i64, bool);
    type Snapshot = CounterState;
    fn prepare(
        &self,
        bytes: &[u8],
        inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<Self::Prepared, ApplyError> {
        self.calls.set(self.calls.get() + 1);
        if self.mode == 0 && revision.get() == 5 {
            return Err(ApplyError::Conflict);
        }
        Ok((
            self.inner.prepare(bytes, inventory, revision)?,
            self.mode == 1 && revision.get() == 5,
        ))
    }
    fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
        let mut digest = ReadyCounter::result_digest(&prepared.0);
        if prepared.1 {
            digest[0] ^= 1;
        }
        digest
    }
    fn publish(&mut self, prepared: Self::Prepared) {
        self.inner.publish(prepared.0);
    }
    fn snapshot(&self) -> CounterState {
        self.inner.snapshot()
    }
}
impl PackedCoordinatorState for RejectRecovery {
    fn validate_packed_metadata(
        &self,
        scope: NamespaceRef,
        claims: PackedRootClaims,
    ) -> Result<(), ApplyError> {
        self.inner.validate_packed_metadata(scope, claims)
    }
}
impl PackedCoordinatorPublicationState for RejectRecovery {
    fn packed_publication_claims(
        &self,
        scope: NamespaceRef,
        anchor: (CommitRevision, [u8; 32]),
    ) -> Result<PackedRootClaims, ApplyError> {
        let mut claims = self.inner.packed_publication_claims(scope, anchor)?;
        if self.mode == 4 {
            claims.state_commitment_profile[0] ^= 1;
        }
        Ok(claims)
    }
}
impl PackedCoordinatorRecoveryState for RejectRecovery {
    fn publish_recovered(
        &mut self,
        prepared: Self::Prepared,
        transaction: &uste_txn::RecoveredFrontierTransaction,
    ) -> Result<(), ApplyError> {
        if transaction.revision().get() == 5 {
            if self.mode == 2 {
                return Err(ApplyError::InvalidRequest);
            }
            if self.mode == 3 {
                self.inner.publish(prepared.0);
                return Ok(());
            }
        }
        self.inner.publish_recovered(prepared.0, transaction)
    }
}
#[test]
fn packed_recovery_late_false_result_preparation_anchor_or_profile_never_publishes() {
    for mode in 0..5 {
        let (p, _, _) = lagging();
        let mut fs = p.fs;
        let calls = std::rc::Rc::new(Cell::new(0));
        let state = RejectRecovery {
            inner: p.state,
            mode,
            calls: calls.clone(),
        };
        assert!(
            PackedCommitCoordinator::recover_from_admitted_prefixes(
                p.recovery,
                &mut fs,
                p.primary,
                p.quota,
                &p.primary_root,
                &p.quota_root,
                state,
                RetentionDays::new(30).unwrap(),
                CoordinatorRecoveryLimits::new(0, 0).unwrap(),
                rebase_limits()
            )
            .is_err()
        );
        assert_eq!(calls.get(), 2);
        let p = cold_parts(fs, p.name, 6502);
        let mut fs = p.fs;
        for revision in [4, 5] {
            for profile in [
                COORDINATOR_PACKED_PROFILE_V1,
                COORDINATOR_PACKED_USAGE_PROFILE_V1,
            ] {
                assert!(
                    p.recovery
                        .discover_packed_roots_at_revision(
                            &mut fs,
                            profile,
                            CommitRevision::new(revision).unwrap(),
                            limits().certificates,
                            PackedRootDiscoveryLimits::new(4, 4 * 4177).unwrap()
                        )
                        .unwrap()
                        .0
                        .is_empty()
                );
            }
        }
    }
    let (p, _, _) = lagging();
    let mut fs = p.fs;
    let calls = std::rc::Rc::new(Cell::new(0));
    let state = RejectRecovery {
        inner: p.state,
        mode: 0,
        calls: calls.clone(),
    };
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        PackedCommitCoordinator::recover_from_admitted_prefixes(
            p.recovery,
            &mut fs,
            p.primary,
            p.quota,
            &p.primary_root,
            &p.quota_root,
            state,
            RetentionDays::new(30).unwrap(),
            CoordinatorRecoveryLimits::new(0, 0).unwrap(),
            PackedMetadataRebaseLimits {
                maximum_groups: 1,
                ..rebase_limits()
            }
        )
        .is_err()
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
}

#[test]
fn packed_recovery_corrupt_suffix_and_derived_families_return_no_live_state() {
    for variant in 0..3 {
        let (p, _, _) = lagging();
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
        let mut fs = p.fs;
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
        let directory = fs.open_directory(&fs.root(), &p.name).unwrap();
        let file = fs
            .open_existing(&directory, &EntryName::new(filename).unwrap())
            .unwrap();
        let mut original = [0];
        fs.read_at(&file, offset, &mut original).unwrap();
        fs.write_at(&file, offset, &[original[0] ^ 1]).unwrap();
        fs.sync_all(&file).unwrap();
        assert!(
            PackedCommitCoordinator::recover_from_admitted_prefixes(
                p.recovery,
                &mut fs,
                p.primary,
                p.quota,
                &p.primary_root,
                &p.quota_root,
                p.state,
                RetentionDays::new(30).unwrap(),
                CoordinatorRecoveryLimits::new(0, 0).unwrap(),
                rebase_limits()
            )
            .is_err()
        );
        // Restore the deliberately damaged test byte, then independently reauthenticate.
        fs.write_at(&file, offset, &original).unwrap();
        fs.sync_all(&file).unwrap();
        let p = cold_parts(fs, p.name, 6602);
        let mut fs = p.fs;
        for revision in [4, 5] {
            for profile in [
                COORDINATOR_PACKED_PROFILE_V1,
                COORDINATOR_PACKED_USAGE_PROFILE_V1,
            ] {
                assert!(
                    p.recovery
                        .discover_packed_roots_at_revision(
                            &mut fs,
                            profile,
                            CommitRevision::new(revision).unwrap(),
                            limits().certificates,
                            PackedRootDiscoveryLimits::new(4, 4 * 4177).unwrap()
                        )
                        .unwrap()
                        .0
                        .is_empty()
                );
            }
        }
        PackedCommitCoordinator::recover_from_admitted_prefixes(
            p.recovery,
            &mut fs,
            p.primary,
            p.quota,
            &p.primary_root,
            &p.quota_root,
            p.state,
            RetentionDays::new(30).unwrap(),
            CoordinatorRecoveryLimits::new(0, 0).unwrap(),
            rebase_limits(),
        )
        .unwrap();
    }
}

#[test]
fn packed_recovery_authenticated_retry_transaction_and_false_result_suffixes_are_rejected() {
    for variant in 0..3 {
        let p = parts(1);
        let mut fs = p.fs;
        drop(p.recovery);
        let (mut journal, _) = JournalStore::open(
            &mut fs,
            &p.name,
            scope().database(),
            CounterEntropy(6701),
            CounterEntropy(6702),
            &mut TestKeyAdapter,
            |_| Ok(()),
        )
        .unwrap();
        let mut group: Vec<u8> = include_str!("../../../../../acceptance/r1/txn-group-v1.hex")
            .trim()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        let bytes = mutation(1, 1);
        group[192..].copy_from_slice(&bytes);
        group[120..152].copy_from_slice(&Sha256::digest(bytes));
        group[152..184].copy_from_slice(&CounterState::result_digest(
            &(if variant == 2 { 3 } else { 2 }),
        ));
        group[24..56].fill(if variant == 1 { 4 } else { 3 });
        group[56..72].fill(if variant == 0 { 1 } else { 2 });
        group[72..88].fill(if variant == 1 { 11 } else { 12 });
        journal
            .append_group(
                &mut fs,
                CommitInput {
                    encoded_group: &group,
                    logical_event_digest: Sha256::digest(&group).into(),
                },
            )
            .unwrap();
        drop(journal);
        let (mut recovery, _) = transactions_with_entropy(&mut fs, &p.name, 2, 6802);
        let mut discover = |profile| {
            recovery
                .discover_packed_roots_at_revision(
                    &mut fs,
                    profile,
                    CommitRevision::FIRST,
                    limits().certificates,
                    PackedRootDiscoveryLimits::new(4, 4 * 4177).unwrap(),
                )
                .unwrap()
                .0
        };
        let primary_root = discover(COORDINATOR_PACKED_PROFILE_V1).pop().unwrap();
        let quota_root = discover(COORDINATOR_PACKED_USAGE_PROFILE_V1).pop().unwrap();
        let primary = admit_packed_coordinator_prefix(
            &mut recovery,
            &mut fs,
            &primary_root,
            primary_limits(1),
        )
        .unwrap()
        .0;
        let quota =
            admit_packed_quota_prefix(&mut recovery, &mut fs, &primary, &quota_root, admission())
                .unwrap()
                .0;
        let state = ReadyCounter {
            state: CounterState(1),
            anchor: primary.anchor(),
        };
        let result = PackedCommitCoordinator::recover_from_admitted_prefixes(
            recovery,
            &mut fs,
            primary,
            quota,
            &primary_root,
            &quota_root,
            state,
            RetentionDays::new(30).unwrap(),
            CoordinatorRecoveryLimits::new(0, 0).unwrap(),
            rebase_limits(),
        );
        assert!(result.is_err(), "variant {variant}");
        if variant == 2 {
            assert!(matches!(result, Err(TransactionError::IntegrityFailure)));
        }
        let (recovery, _) = transactions_with_entropy(&mut fs, &p.name, 2, 6902);
        for profile in [
            COORDINATOR_PACKED_PROFILE_V1,
            COORDINATOR_PACKED_USAGE_PROFILE_V1,
        ] {
            assert!(
                recovery
                    .discover_packed_roots_at_revision(
                        &mut fs,
                        profile,
                        CommitRevision::new(2).unwrap(),
                        limits().certificates,
                        PackedRootDiscoveryLimits::new(4, 4 * 4177).unwrap()
                    )
                    .unwrap()
                    .0
                    .is_empty()
            );
            assert_eq!(
                recovery
                    .discover_packed_roots_at_revision(
                        &mut fs,
                        profile,
                        CommitRevision::FIRST,
                        limits().certificates,
                        PackedRootDiscoveryLimits::new(4, 4 * 4177).unwrap()
                    )
                    .unwrap()
                    .0
                    .len(),
                1
            );
        }
    }
}
