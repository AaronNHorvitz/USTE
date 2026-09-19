use super::*;
#[path = "packed_quota_admission.rs"]
mod admission;
use uste_txn::{COORDINATOR_PACKED_USAGE_PROFILE_V1, PackedQuotaPrefix, stage_packed_quota_prefix};

fn fixture() -> (Fs, EntryName) {
    fixture_count(5)
}
fn fixture_count(transaction_count: u8) -> (Fs, EntryName) {
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let name = EntryName::new("packed-quota").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 2401),
        CounterEntropy(2402),
        CounterState::default(),
    )
    .unwrap();
    let references = [b"".as_slice(), b"first".as_slice(), b"two".as_slice()].map(|bytes| {
        let mut upload = coordinator.start_blob_upload(scope()).unwrap();
        if !bytes.is_empty() {
            coordinator
                .write_blob_upload(&mut fs, &mut upload, bytes)
                .unwrap();
        }
        coordinator
            .finish_blob_upload(&mut fs, &mut upload)
            .unwrap()
    });
    for index in 0_u8..transaction_count {
        let count = usize::from(index.min(3));
        let inventory = (count != 0)
            .then(|| BlobInventory::new(scope(), references[..count].iter().copied()).unwrap());
        let principal = if index == 1 || index == 4 {
            principal(1)
        } else {
            principal(0)
        };
        coordinator
            .commit(
                &mut fs,
                TransactionRequest {
                    principal,
                    blob_inventory: inventory.as_ref(),
                    ..request(index + 1, index + 11, &mutation(i64::from(index), 1))
                },
                &mut clock(i64::from(index)),
                &NeverCancel,
            )
            .unwrap();
    }
    drop(coordinator);
    (fs, name)
}

#[test]
fn packed_quota_empty_zero_byte_and_new_existing_principal_totals_match_first_owners() {
    assert_eq!(
        <[u8; 32]>::from(Sha256::digest(b"USTE coordinator-packed-usage-v1")),
        COORDINATOR_PACKED_USAGE_PROFILE_V1
    );
    let (mut fs, name) = fixture();
    let (mut recovery, transactions) = transactions(&mut fs, &name, 5);
    let mut primary = None;
    let mut quota: Option<PackedQuotaPrefix> = None;
    for (index, transaction) in transactions.iter().enumerate() {
        let (next, _) = stage_packed_coordinator_prefix(
            &mut recovery,
            &mut fs,
            primary.as_ref(),
            transaction,
            limits(),
        )
        .unwrap();
        let (next_quota, report) = stage_packed_quota_prefix(
            &mut recovery,
            &mut fs,
            quota.as_ref(),
            &next,
            transaction,
            limits(),
        )
        .unwrap();
        assert_eq!(next_quota.anchor(), next.anchor());
        assert_eq!(report.new_owners, [0, 1, 1, 1, 0][index]);
        assert_eq!(report.charged_bytes, [0, 0, 5, 3, 0][index]);
        let maintenance = recovery
            .packed_indexes_with_io(&mut fs, transaction, limits().certificates)
            .unwrap();
        for selected in [
            principal(0),
            principal(1),
            PrincipalDigest::from_bytes([99; 32]),
        ] {
            let (actual, _) = next_quota
                .usage(&maintenance, &mut fs, &next, selected, reads())
                .unwrap();
            assert_eq!(actual.namespace_bytes, [0, 0, 5, 8, 8][index]);
            assert_eq!(actual.owners, [0, 1, 2, 3, 3][index]);
            assert_eq!(
                actual.principal_bytes,
                if selected == principal(0) {
                    [0, 0, 5, 8, 8][index]
                } else {
                    0
                }
            );
        }
        if let Some(previous) = &primary {
            fs.arm(FaultPlan::default()).unwrap();
            assert!(
                next_quota
                    .usage(&maintenance, &mut fs, previous, principal(0), reads())
                    .is_err()
            );
            assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        }
        primary = Some(next);
        quota = Some(next_quota);
    }
}

#[test]
fn packed_quota_missing_base_limits_and_foreign_empty_primary_refuse_before_output() {
    let (mut fs, name) = fixture();
    let (mut recovery, transactions) = transactions(&mut fs, &name, 5);
    let (first, _) =
        stage_packed_coordinator_prefix(&mut recovery, &mut fs, None, &transactions[0], limits())
            .unwrap();
    let (second, _) = stage_packed_coordinator_prefix(
        &mut recovery,
        &mut fs,
        Some(&first),
        &transactions[1],
        limits(),
    )
    .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        stage_packed_quota_prefix(
            &mut recovery,
            &mut fs,
            None,
            &second,
            &transactions[1],
            limits()
        )
        .is_err()
    );
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    let (quota, _) = stage_packed_quota_prefix(
        &mut recovery,
        &mut fs,
        None,
        &first,
        &transactions[0],
        limits(),
    )
    .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    for selected in [
        PackedCoordinatorLimits {
            maximum_references: 0,
            ..limits()
        },
        PackedCoordinatorLimits {
            maximum_owners: 0,
            ..limits()
        },
    ] {
        assert!(
            stage_packed_quota_prefix(
                &mut recovery,
                &mut fs,
                Some(&quota),
                &second,
                &transactions[1],
                selected
            )
            .is_err()
        );
    }
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    let (mut foreign_fs, foreign_name) = fixture();
    let (mut foreign, foreign_transactions) = self::transactions(&mut foreign_fs, &foreign_name, 5);
    let (foreign_first, _) = stage_packed_coordinator_prefix(
        &mut foreign,
        &mut foreign_fs,
        None,
        &foreign_transactions[0],
        limits(),
    )
    .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        stage_packed_quota_prefix(
            &mut recovery,
            &mut fs,
            None,
            &foreign_first,
            &transactions[0],
            limits()
        )
        .is_err()
    );
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
}

fn stage_first_three(
    recovery: &mut Recovery,
    fs: &mut Fs,
    transactions: &[uste_txn::RecoveredFrontierTransaction],
) -> (PackedCoordinatorPrefix, PackedQuotaPrefix) {
    let mut primary = None;
    let mut quota = None;
    for transaction in &transactions[..3] {
        let next =
            stage_packed_coordinator_prefix(recovery, fs, primary.as_ref(), transaction, limits())
                .unwrap()
                .0;
        quota = Some(
            stage_packed_quota_prefix(recovery, fs, quota.as_ref(), &next, transaction, limits())
                .unwrap()
                .0,
        );
        primary = Some(next);
    }
    (primary.unwrap(), quota.unwrap())
}

#[test]
fn packed_quota_all_observed_staging_faults_preserve_prior_pair_and_rebuild() {
    let (mut observed_fs, name) = fixture();
    let (mut observed, transactions) = transactions(&mut observed_fs, &name, 5);
    let (base, quota) = stage_first_three(&mut observed, &mut observed_fs, &transactions);
    let next = stage_packed_coordinator_prefix(
        &mut observed,
        &mut observed_fs,
        Some(&base),
        &transactions[3],
        limits(),
    )
    .unwrap()
    .0;
    observed_fs.arm(FaultPlan::default()).unwrap();
    let expected = stage_packed_quota_prefix(
        &mut observed,
        &mut observed_fs,
        Some(&quota),
        &next,
        &transactions[3],
        limits(),
    )
    .unwrap()
    .0;
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
    .map(|operation| (operation, observed_fs.operation_count(operation)));
    let mut cases = 0;
    for (operation, count) in boundaries {
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut fs, name) = fixture();
                let (mut recovery, transactions) = self::transactions(&mut fs, &name, 5);
                let (base, quota) = stage_first_three(&mut recovery, &mut fs, &transactions);
                let next = stage_packed_coordinator_prefix(
                    &mut recovery,
                    &mut fs,
                    Some(&base),
                    &transactions[3],
                    limits(),
                )
                .unwrap()
                .0;
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
                    stage_packed_quota_prefix(
                        &mut recovery,
                        &mut fs,
                        Some(&quota),
                        &next,
                        &transactions[3],
                        limits()
                    )
                    .is_err(),
                    "{operation:?}/{occurrence}/{action:?}"
                );
                assert_eq!(fs.pending_faults(), 0);
                if matches!(action, FaultAction::Error(_)) {
                    fs.arm(FaultPlan::default()).unwrap();
                    let maintenance = recovery
                        .packed_indexes_with_io(&mut fs, &transactions[3], limits().certificates)
                        .unwrap();
                    let (usage, _) = quota
                        .usage(&maintenance, &mut fs, &base, principal(0), reads())
                        .unwrap();
                    assert_eq!(usage.namespace_bytes, 5);
                    assert_eq!(usage.principal_bytes, 5);
                    assert_eq!(usage.owners, 2);
                }
                drop(recovery);
                fs.restart().unwrap();
                fs.arm(FaultPlan::default()).unwrap();
                let (mut recovery, transactions) =
                    transactions_with_entropy(&mut fs, &name, 5, 2602);
                let (base, quota) = stage_first_three(&mut recovery, &mut fs, &transactions);
                let next = stage_packed_coordinator_prefix(
                    &mut recovery,
                    &mut fs,
                    Some(&base),
                    &transactions[3],
                    limits(),
                )
                .unwrap()
                .0;
                let actual = stage_packed_quota_prefix(
                    &mut recovery,
                    &mut fs,
                    Some(&quota),
                    &next,
                    &transactions[3],
                    limits(),
                )
                .unwrap()
                .0;
                assert_eq!(
                    actual.families().map(|f| f.commitment),
                    expected.families().map(|f| f.commitment)
                );
                let maintenance = recovery
                    .packed_indexes_with_io(&mut fs, &transactions[3], limits().certificates)
                    .unwrap();
                let (usage, _) = actual
                    .usage(&maintenance, &mut fs, &next, principal(0), reads())
                    .unwrap();
                assert_eq!(usage.namespace_bytes, 8);
                assert_eq!(usage.principal_bytes, 8);
                assert_eq!(usage.owners, 3);
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 351);
}

#[test]
fn packed_quota_corrupt_head_or_principal_and_narrow_reads_never_return_partial_usage() {
    for family in [1, 2] {
        let (mut fs, name) = fixture();
        let (mut recovery, transactions) = transactions(&mut fs, &name, 5);
        let (base, quota) = stage_first_three(&mut recovery, &mut fs, &transactions);
        let next = stage_packed_coordinator_prefix(
            &mut recovery,
            &mut fs,
            Some(&base),
            &transactions[3],
            limits(),
        )
        .unwrap()
        .0;
        {
            let maintenance = recovery
                .packed_indexes_with_io(&mut fs, &transactions[3], limits().certificates)
                .unwrap();
            assert!(
                quota
                    .usage(
                        &maintenance,
                        &mut fs,
                        &base,
                        principal(0),
                        TreeLookupLimits {
                            maximum_encoded_bytes: 0,
                            ..reads()
                        }
                    )
                    .is_err()
            );
        }
        let location = quota.families()[usize::from(family - 1)]
            .root
            .unwrap()
            .resolve(
                scope(),
                COORDINATOR_PACKED_USAGE_PROFILE_V1,
                family,
                quota.anchor().0,
            )
            .unwrap();
        let object = location
            .object
            .iter()
            .map(|byte| format!("{byte:02x}"))
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
        {
            let maintenance = recovery
                .packed_indexes_with_io(&mut fs, &transactions[3], limits().certificates)
                .unwrap();
            assert!(
                quota
                    .usage(&maintenance, &mut fs, &base, principal(0), reads())
                    .is_err()
            );
        }
        assert!(
            stage_packed_quota_prefix(
                &mut recovery,
                &mut fs,
                Some(&quota),
                &next,
                &transactions[3],
                limits()
            )
            .is_err()
        );
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    }
}
