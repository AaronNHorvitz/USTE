use super::*;
#[path = "packed_quota_rebuild.rs"]
mod rebuild;
use uste_storage::{
    journal::{CertifiedPackedRoot, PackedRootDiscoveryLimits},
    packed_root_manifest::PackedRootClaims,
    packed_tree_validation::TreeValidationLimits,
};
use uste_txn::{
    PackedCoordinatorAdmissionLimits, PackedQuotaAdmissionLimits, admit_packed_coordinator_prefix,
    admit_packed_quota_prefix,
};

fn admission() -> PackedQuotaAdmissionLimits {
    PackedQuotaAdmissionLimits {
        certificates: limits().certificates,
        family: TreeValidationLimits {
            maximum_path_branches: 128,
            maximum_nodes: 128,
            maximum_logical_bytes: 64000,
            maximum_pages: 128,
            maximum_encoded_bytes: 128 * 20545,
        },
        cursor: cursors(),
        lookup: reads(),
        maximum_owners: 4,
        maximum_lookup_pages: 1000,
        maximum_lookup_bytes: 1000 * 20545,
    }
}
fn primary_limits(count: u64) -> PackedCoordinatorAdmissionLimits {
    PackedCoordinatorAdmissionLimits {
        certificates: limits().certificates,
        family: admission().family,
        lookup: reads(),
        maximum_groups: count,
        maximum_journal_bytes: 2_000_000,
        maximum_references: 4,
        maximum_owners: 4,
        maximum_lookup_pages: 2000,
        maximum_lookup_bytes: 2000 * 20545,
    }
}
fn root_fixture(
    count: u8,
) -> (
    Fs,
    EntryName,
    Recovery,
    PackedCoordinatorPrefix,
    PackedQuotaPrefix,
    CertifiedPackedRoot,
    CertifiedPackedRoot,
) {
    let (mut fs, name) = fixture_count(count);
    let (mut recovery, transactions) = transactions(&mut fs, &name, u64::from(count));
    let mut primary = None;
    let mut quota = None;
    for transaction in &transactions {
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
        reducer_profile: [9; 32],
        state_commitment_profile: [10; 32],
        state_digest: [11; 32],
    };
    let primary_root = recovery
        .publish_recovered_packed_root(
            &mut fs,
            COORDINATOR_PACKED_PROFILE_V1,
            claims,
            &primary.families(),
            2,
        )
        .unwrap();
    let quota_root = recovery
        .publish_recovered_packed_root(
            &mut fs,
            COORDINATOR_PACKED_USAGE_PROFILE_V1,
            claims,
            &quota.families(),
            2,
        )
        .unwrap();
    (fs, name, recovery, primary, quota, primary_root, quota_root)
}

#[test]
fn packed_quota_admission_empty_and_populated_cold_pairing_and_exact_limits() {
    for count in [1, 5] {
        let (mut fs, name, recovery, old_primary, old_quota, _, _) = root_fixture(count);
        drop(recovery);
        let (mut recovery, transactions) =
            transactions_with_entropy(&mut fs, &name, u64::from(count), 2702);
        let primary_root = recovery
            .discover_packed_roots_at_revision(
                &mut fs,
                COORDINATOR_PACKED_PROFILE_V1,
                old_primary.anchor().0,
                limits().certificates,
                PackedRootDiscoveryLimits::new(2, 8354).unwrap(),
            )
            .unwrap()
            .0
            .remove(0);
        let quota_root = recovery
            .discover_packed_roots_at_revision(
                &mut fs,
                COORDINATOR_PACKED_USAGE_PROFILE_V1,
                old_primary.anchor().0,
                limits().certificates,
                PackedRootDiscoveryLimits::new(2, 8354).unwrap(),
            )
            .unwrap()
            .0
            .remove(0);
        fs.arm(FaultPlan::default()).unwrap();
        assert!(
            admit_packed_quota_prefix(
                &mut recovery,
                &mut fs,
                &old_primary,
                &quota_root,
                admission()
            )
            .is_err()
        );
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        let primary = admit_packed_coordinator_prefix(
            &mut recovery,
            &mut fs,
            &primary_root,
            primary_limits(u64::from(count)),
        )
        .unwrap()
        .0;
        fs.arm(FaultPlan::default()).unwrap();
        let (quota, report) =
            admit_packed_quota_prefix(&mut recovery, &mut fs, &primary, &quota_root, admission())
                .unwrap();
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
        assert_eq!(fs.operation_count(Operation::WriteAt), 0);
        assert_eq!(
            quota.families().map(|f| f.commitment),
            old_quota.families().map(|f| f.commitment)
        );
        let exact = PackedQuotaAdmissionLimits {
            maximum_lookup_pages: report.lookup_pages,
            maximum_lookup_bytes: report.lookup_bytes,
            ..admission()
        };
        admit_packed_quota_prefix(&mut recovery, &mut fs, &primary, &quota_root, exact).unwrap();
        for selected in [
            PackedQuotaAdmissionLimits {
                maximum_lookup_pages: report.lookup_pages - 1,
                ..exact
            },
            PackedQuotaAdmissionLimits {
                maximum_lookup_bytes: report.lookup_bytes - 1,
                ..exact
            },
        ] {
            assert!(
                admit_packed_quota_prefix(&mut recovery, &mut fs, &primary, &quota_root, selected)
                    .is_err()
            );
        }
        let maintenance = recovery
            .packed_indexes_with_io(&mut fs, transactions.last().unwrap(), limits().certificates)
            .unwrap();
        fs.arm(FaultPlan::default()).unwrap();
        assert!(
            quota
                .usage(&maintenance, &mut fs, &old_primary, principal(0), reads())
                .is_err()
        );
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        assert!(
            old_quota
                .usage(&maintenance, &mut fs, &primary, principal(0), reads())
                .is_err()
        );
        let (actual, _) = quota
            .usage(&maintenance, &mut fs, &primary, principal(0), reads())
            .unwrap();
        assert_eq!(actual.namespace_bytes, if count == 1 { 0 } else { 8 });
        assert_eq!(actual.principal_bytes, actual.namespace_bytes);
        assert_eq!(actual.owners, if count == 1 { 0 } else { 3 });
    }
}

#[test]
fn packed_quota_admission_authenticated_false_totals_principals_and_owners_are_rejected() {
    for case in 0..7 {
        let (mut fs, _, mut recovery, primary, _, _, root) = root_fixture(5);
        let mut cursor = recovery
            .open_transaction_cursor(primary.anchor().0, primary.anchor().0, 1, 100_000)
            .unwrap();
        let frontier = recovery
            .next_recovered_transaction(&mut fs, &mut cursor)
            .unwrap()
            .unwrap();
        recovery.finish_transaction_cursor(cursor).unwrap();
        let mut families = root.manifest().families().to_vec();
        {
            let mut maintenance = recovery
                .packed_indexes_with_io(&mut fs, &frontier, limits().certificates)
                .unwrap();
            let family = match case {
                0 => 1,
                1 | 5 | 6 => 2,
                _ => 3,
            };
            let tree = maintenance
                .admit(&mut fs, &root, family, admission().family)
                .unwrap()
                .0;
            let mut cursor = maintenance.cursor(&tree, b"", None, cursors()).unwrap();
            let entry = maintenance.next(&mut fs, &mut cursor).unwrap().unwrap();
            let key = entry.key().to_vec();
            let before = entry.value().to_vec();
            let mut after = before.clone();
            let mut deltas = match case {
                4 => {
                    let mut wrong_key = key.clone();
                    wrong_key[32..].fill(255);
                    vec![
                        IndexDelta::new(key.clone(), Some(before.clone()), None).unwrap(),
                        IndexDelta::new(wrong_key, None, Some(after.clone())).unwrap(),
                    ]
                }
                5 => vec![
                    IndexDelta::new(
                        vec![99; 32],
                        None,
                        Some([1_u64.to_be_bytes(), 0_u64.to_be_bytes()].concat()),
                    )
                    .unwrap(),
                ],
                _ => {
                    match case {
                        0 | 1 => after[15] ^= 1,
                        2 => after[48] ^= 1,
                        3 => after[11] ^= 1,
                        6 => {
                            after.pop();
                        }
                        _ => unreachable!(),
                    }
                    vec![IndexDelta::new(key, Some(before), Some(after)).unwrap()]
                }
            };
            deltas.sort_by(|a, b| a.key().cmp(b.key()));
            let stage = maintenance
                .stage(
                    &mut fs,
                    COORDINATOR_PACKED_USAGE_PROFILE_V1,
                    family,
                    Some(&tree),
                    &deltas,
                    batches(),
                )
                .unwrap();
            families[usize::from(family - 1)] = stage.tree().family_descriptor();
            if case == 5 {
                let tree = maintenance
                    .admit(&mut fs, &root, 1, admission().family)
                    .unwrap()
                    .0;
                let before = maintenance
                    .get(&mut fs, &tree, b"packed-usage-v1", reads())
                    .unwrap()
                    .value
                    .unwrap()
                    .as_slice()
                    .to_vec();
                let mut after = before.clone();
                after[16..].copy_from_slice(&3_u64.to_be_bytes());
                let stage = maintenance
                    .stage(
                        &mut fs,
                        COORDINATOR_PACKED_USAGE_PROFILE_V1,
                        1,
                        Some(&tree),
                        &[
                            IndexDelta::new(b"packed-usage-v1".to_vec(), Some(before), Some(after))
                                .unwrap(),
                        ],
                        batches(),
                    )
                    .unwrap();
                families[0] = stage.tree().family_descriptor();
            }
        }
        let false_root = recovery
            .publish_recovered_packed_root(
                &mut fs,
                COORDINATOR_PACKED_USAGE_PROFILE_V1,
                root.manifest().claims(),
                &families,
                2,
            )
            .unwrap();
        fs.arm(FaultPlan::default()).unwrap();
        assert!(
            admit_packed_quota_prefix(&mut recovery, &mut fs, &primary, &false_root, admission())
                .is_err(),
            "case {case}"
        );
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
        admit_packed_quota_prefix(&mut recovery, &mut fs, &primary, &root, admission()).unwrap();
    }
}

#[test]
fn packed_quota_admission_every_observed_read_fault_keeps_pair_recoverable() {
    let (mut observed_fs, _, mut observed, primary, _, _, root) = root_fixture(5);
    observed_fs.arm(FaultPlan::default()).unwrap();
    let expected = admit_packed_quota_prefix(
        &mut observed,
        &mut observed_fs,
        &primary,
        &root,
        admission(),
    )
    .unwrap()
    .0;
    let boundaries = [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
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
                let (mut fs, name, mut recovery, primary, _, _, root) = root_fixture(5);
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
                    admit_packed_quota_prefix(&mut recovery, &mut fs, &primary, &root, admission())
                        .is_err(),
                    "{operation:?}/{occurrence}/{action:?}"
                );
                assert_eq!(fs.pending_faults(), 0);
                assert_eq!(fs.operation_count(Operation::CreateNew), 0);
                assert_eq!(fs.operation_count(Operation::WriteAt), 0);
                drop(recovery);
                fs.restart().unwrap();
                fs.arm(FaultPlan::default()).unwrap();
                let (mut recovery, _) = transactions_with_entropy(&mut fs, &name, 5, 2802);
                let mut discover = |profile| {
                    recovery
                        .discover_packed_roots_at_revision(
                            &mut fs,
                            profile,
                            primary.anchor().0,
                            limits().certificates,
                            PackedRootDiscoveryLimits::new(2, 8354).unwrap(),
                        )
                        .unwrap()
                        .0
                        .remove(0)
                };
                let primary_root = discover(COORDINATOR_PACKED_PROFILE_V1);
                let quota_root = discover(COORDINATOR_PACKED_USAGE_PROFILE_V1);
                let primary = admit_packed_coordinator_prefix(
                    &mut recovery,
                    &mut fs,
                    &primary_root,
                    primary_limits(5),
                )
                .unwrap()
                .0;
                let actual = admit_packed_quota_prefix(
                    &mut recovery,
                    &mut fs,
                    &primary,
                    &quota_root,
                    admission(),
                )
                .unwrap()
                .0;
                assert_eq!(actual.anchor(), expected.anchor());
                assert_eq!(
                    actual.families().map(|f| f.commitment),
                    expected.families().map(|f| f.commitment)
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 381);
}

#[test]
fn packed_quota_admission_cursor_limits_and_late_ciphertext_corruption_fail_closed() {
    let (mut fs, name, mut recovery, primary, quota, _, root) = root_fixture(5);
    let (_, report) =
        admit_packed_quota_prefix(&mut recovery, &mut fs, &primary, &root, admission()).unwrap();
    let mut exact = admission();
    exact.cursor.maximum_pages = report.cursor.pages;
    exact.cursor.maximum_encoded_bytes = report.cursor.encoded_bytes;
    exact.cursor.maximum_candidates = report.cursor.candidates;
    exact.cursor.maximum_returned_bytes = report.cursor.returned_bytes;
    admit_packed_quota_prefix(&mut recovery, &mut fs, &primary, &root, exact).unwrap();
    for cursor in [
        TreeCursorLimits {
            maximum_pages: report.cursor.pages - 1,
            ..exact.cursor
        },
        TreeCursorLimits {
            maximum_encoded_bytes: report.cursor.encoded_bytes - 1,
            ..exact.cursor
        },
        TreeCursorLimits {
            maximum_candidates: report.cursor.candidates - 1,
            ..exact.cursor
        },
        TreeCursorLimits {
            maximum_returned_bytes: report.cursor.returned_bytes - 1,
            ..exact.cursor
        },
    ] {
        assert!(
            admit_packed_quota_prefix(
                &mut recovery,
                &mut fs,
                &primary,
                &root,
                PackedQuotaAdmissionLimits { cursor, ..exact }
            )
            .is_err()
        );
    }
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        admit_packed_quota_prefix(
            &mut recovery,
            &mut fs,
            &primary,
            &root,
            PackedQuotaAdmissionLimits {
                maximum_owners: 2,
                ..exact
            }
        )
        .is_err()
    );
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    let location = quota.families()[2]
        .root
        .unwrap()
        .resolve(
            scope(),
            COORDINATOR_PACKED_USAGE_PROFILE_V1,
            3,
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
    assert!(
        admit_packed_quota_prefix(&mut recovery, &mut fs, &primary, &root, admission()).is_err()
    );
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
}
