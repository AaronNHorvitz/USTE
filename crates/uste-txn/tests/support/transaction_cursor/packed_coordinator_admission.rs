use super::*;
use uste_storage::{
    journal::{CertifiedPackedRoot, PackedRootDiscoveryLimits},
    packed_root_manifest::PackedRootClaims,
    packed_tree_validation::TreeValidationLimits,
};
use uste_txn::{PackedCoordinatorAdmissionLimits, admit_packed_coordinator_prefix};

fn admission() -> PackedCoordinatorAdmissionLimits {
    PackedCoordinatorAdmissionLimits {
        certificates: limits().certificates,
        family: TreeValidationLimits {
            maximum_path_branches: 128,
            maximum_nodes: 128,
            maximum_logical_bytes: 64000,
            maximum_pages: 128,
            maximum_encoded_bytes: 128 * 20545,
        },
        lookup: reads(),
        maximum_groups: 3,
        maximum_journal_bytes: 2_000_000,
        maximum_references: 4,
        maximum_owners: 4,
        maximum_lookup_pages: 1000,
        maximum_lookup_bytes: 1000 * 20545,
    }
}
fn root_fixture() -> (
    Fs,
    EntryName,
    Recovery,
    CertifiedPackedRoot,
    PackedCoordinatorPrefix,
) {
    let (mut fs, name, _, _) = populated(3);
    let (mut recovery, transactions) = transactions(&mut fs, &name, 3);
    let mut prefix = None;
    for transaction in &transactions {
        prefix = Some(
            stage_packed_coordinator_prefix(
                &mut recovery,
                &mut fs,
                prefix.as_ref(),
                transaction,
                limits(),
            )
            .unwrap()
            .0,
        );
    }
    let prefix = prefix.unwrap();
    let root = recovery
        .publish_recovered_packed_root(
            &mut fs,
            COORDINATOR_PACKED_PROFILE_V1,
            PackedRootClaims {
                revision: prefix.anchor().0,
                certificate_digest: prefix.anchor().1,
                generation: 1,
                reducer_profile: [9; 32],
                state_commitment_profile: [10; 32],
                state_digest: [11; 32],
            },
            &prefix.families(),
            2,
        )
        .unwrap();
    (fs, name, recovery, root, prefix)
}

#[test]
fn packed_admission_cold_correspondence_exact_budgets_and_no_index_writes() {
    let (mut fs, name, recovery, _, old) = root_fixture();
    drop(recovery);
    let (mut recovery, transactions) = transactions_with_entropy(&mut fs, &name, 3, 2002);
    let roots = recovery
        .discover_packed_roots_at_revision(
            &mut fs,
            COORDINATOR_PACKED_PROFILE_V1,
            old.anchor().0,
            limits().certificates,
            PackedRootDiscoveryLimits::new(2, 8354).unwrap(),
        )
        .unwrap()
        .0;
    assert_eq!(roots.len(), 1);
    fs.arm(FaultPlan::default()).unwrap();
    let (prefix, report) =
        admit_packed_coordinator_prefix(&mut recovery, &mut fs, &roots[0], admission()).unwrap();
    assert_eq!(prefix.anchor(), old.anchor());
    assert_eq!(
        prefix.families().map(|f| f.commitment),
        old.families().map(|f| f.commitment)
    );
    assert_eq!(report.first_owners, 2);
    assert_eq!(report.certificate.certificates, 1);
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
    let mut exact = admission();
    exact.maximum_lookup_pages = report.lookup_pages;
    exact.maximum_lookup_bytes = report.lookup_bytes;
    exact.maximum_journal_bytes = report.journal.encoded_bytes;
    exact.maximum_references = 2;
    exact.maximum_owners = 2;
    admit_packed_coordinator_prefix(&mut recovery, &mut fs, &roots[0], exact).unwrap();
    for limited in [
        PackedCoordinatorAdmissionLimits {
            maximum_lookup_pages: report.lookup_pages - 1,
            ..exact
        },
        PackedCoordinatorAdmissionLimits {
            maximum_lookup_bytes: report.lookup_bytes - 1,
            ..exact
        },
        PackedCoordinatorAdmissionLimits {
            maximum_journal_bytes: report.journal.encoded_bytes - 1,
            ..exact
        },
        PackedCoordinatorAdmissionLimits {
            maximum_references: 1,
            ..exact
        },
        PackedCoordinatorAdmissionLimits {
            maximum_groups: 2,
            ..exact
        },
        PackedCoordinatorAdmissionLimits {
            maximum_owners: 1,
            ..exact
        },
    ] {
        assert!(
            admit_packed_coordinator_prefix(&mut recovery, &mut fs, &roots[0], limited).is_err()
        );
    }
    let maintenance = recovery
        .packed_indexes_with_io(&mut fs, &transactions[2], limits().certificates)
        .unwrap();
    assert!(
        old.transaction(
            &maintenance,
            &mut fs,
            transactions[0].outcome().transaction_id,
            reads()
        )
        .is_err()
    );
    assert_eq!(
        prefix
            .transaction(
                &maintenance,
                &mut fs,
                transactions[0].outcome().transaction_id,
                reads()
            )
            .unwrap()
            .0
            .unwrap()
            .1,
        transactions[0].outcome()
    );
}

#[test]
fn packed_admission_authenticated_false_metadata_never_grants_a_prefix() {
    for case in 0..7 {
        let (mut fs, _, mut recovery, root, _) = root_fixture();
        let mut cursor = recovery
            .open_transaction_cursor(
                root.manifest().claims().revision,
                root.manifest().claims().revision,
                1,
                100_000,
            )
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
                1 => 2,
                2 => 3,
                _ => 4,
            };
            let tree = maintenance
                .admit(&mut fs, &root, family, admission().family)
                .unwrap()
                .0;
            let mut entries = maintenance.cursor(&tree, b"", None, cursors()).unwrap();
            let mut selected = maintenance.next(&mut fs, &mut entries).unwrap().unwrap();
            if case == 4 {
                while selected.value() != 2_u64.to_be_bytes() {
                    selected = maintenance.next(&mut fs, &mut entries).unwrap().unwrap();
                }
            }
            let key = selected.key().to_vec();
            let before = selected.value().to_vec();
            if case < 5 {
                let mut after = before.clone();
                match case {
                    0 => after[24] ^= 1,
                    1 => after[0] ^= 1,
                    2 => after[48] ^= 1,
                    3 => after.copy_from_slice(&3_u64.to_be_bytes()),
                    4 => after.copy_from_slice(&1_u64.to_be_bytes()),
                    _ => unreachable!(),
                }
                let stage = maintenance
                    .stage(
                        &mut fs,
                        COORDINATOR_PACKED_PROFILE_V1,
                        family,
                        Some(&tree),
                        &[IndexDelta::new(key, Some(before), Some(after)).unwrap()],
                        batches(),
                    )
                    .unwrap();
                families[usize::from(family - 1)] = stage.tree().family_descriptor();
            } else {
                let owner = maintenance
                    .admit(&mut fs, &root, 3, admission().family)
                    .unwrap()
                    .0;
                let owner_before = maintenance
                    .get(&mut fs, &owner, &key, reads())
                    .unwrap()
                    .value
                    .unwrap()
                    .as_slice()
                    .to_vec();
                let (key, owner_before, owner_after, witness_before, witness_after) = if case == 5 {
                    (
                        vec![255; 16],
                        None,
                        Some(owner_before),
                        None,
                        Some(1_u64.to_be_bytes().to_vec()),
                    )
                } else {
                    (key, Some(owner_before), None, Some(before), None)
                };
                for (family, base, before, after) in [
                    (3, &owner, owner_before, owner_after),
                    (4, &tree, witness_before, witness_after),
                ] {
                    let stage = maintenance
                        .stage(
                            &mut fs,
                            COORDINATOR_PACKED_PROFILE_V1,
                            family,
                            Some(base),
                            &[IndexDelta::new(key.clone(), before, after).unwrap()],
                            batches(),
                        )
                        .unwrap();
                    families[usize::from(family - 1)] = stage.tree().family_descriptor();
                }
            }
        }
        let false_root = recovery
            .publish_recovered_packed_root(
                &mut fs,
                COORDINATOR_PACKED_PROFILE_V1,
                root.manifest().claims(),
                &families,
                2,
            )
            .unwrap();
        fs.arm(FaultPlan::default()).unwrap();
        assert!(
            admit_packed_coordinator_prefix(&mut recovery, &mut fs, &false_root, admission())
                .is_err(),
            "case {case}"
        );
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
        let (valid, _) =
            admit_packed_coordinator_prefix(&mut recovery, &mut fs, &root, admission()).unwrap();
        assert_eq!(valid.owner_count(), 2);
    }
}

#[test]
fn packed_admission_late_certificate_corruption_and_wrong_shape_fail_closed() {
    let (mut fs, name, mut recovery, root, _) = root_fixture();
    let wrong = recovery
        .publish_recovered_packed_root(
            &mut fs,
            COORDINATOR_PACKED_PROFILE_V1,
            root.manifest().claims(),
            &root.manifest().families()[..3],
            2,
        )
        .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert!(admit_packed_coordinator_prefix(&mut recovery, &mut fs, &wrong, admission()).is_err());
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    let directory = fs.open_directory(&fs.root(), &name).unwrap();
    let file = fs
        .open_existing(&directory, &EntryName::new("CERTIFICATES").unwrap())
        .unwrap();
    let offset = 2 * 4161 + 137;
    let mut byte = [0];
    fs.read_at(&file, offset, &mut byte).unwrap();
    fs.write_at(&file, offset, &[byte[0] ^ 1]).unwrap();
    fs.sync_all(&file).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert!(admit_packed_coordinator_prefix(&mut recovery, &mut fs, &root, admission()).is_err());
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
}

#[test]
fn packed_admission_every_observed_read_fault_returns_no_prefix_and_restarts() {
    let (mut observed_fs, _, mut observed, root, _) = root_fixture();
    observed_fs.arm(FaultPlan::default()).unwrap();
    let (expected, _) =
        admit_packed_coordinator_prefix(&mut observed, &mut observed_fs, &root, admission())
            .unwrap();
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
                let (mut fs, name, mut recovery, root, _) = root_fixture();
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
                    admit_packed_coordinator_prefix(&mut recovery, &mut fs, &root, admission())
                        .is_err(),
                    "{operation:?}/{occurrence}/{action:?}"
                );
                assert_eq!(fs.pending_faults(), 0);
                assert_eq!(fs.operation_count(Operation::CreateNew), 0);
                assert_eq!(fs.operation_count(Operation::WriteAt), 0);
                drop(recovery);
                fs.restart().unwrap();
                fs.arm(FaultPlan::default()).unwrap();
                let (mut recovery, _) = transactions_with_entropy(&mut fs, &name, 3, 2102);
                let roots = recovery
                    .discover_packed_roots_at_revision(
                        &mut fs,
                        COORDINATOR_PACKED_PROFILE_V1,
                        expected.anchor().0,
                        limits().certificates,
                        PackedRootDiscoveryLimits::new(2, 8354).unwrap(),
                    )
                    .unwrap()
                    .0;
                assert_eq!(roots.len(), 1);
                let (actual, _) =
                    admit_packed_coordinator_prefix(&mut recovery, &mut fs, &roots[0], admission())
                        .unwrap();
                assert_eq!(actual.anchor(), expected.anchor());
                assert_eq!(
                    actual.families().map(|f| f.commitment),
                    expected.families().map(|f| f.commitment)
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 798);
}

#[test]
fn packed_admission_historical_base_advances_only_through_exact_authenticated_suffix() {
    let (mut fs, name, recovery, _, old) = root_fixture();
    drop(recovery);
    let (mut coordinator, _) = CommitCoordinator::open(
        &mut fs,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(2201),
        CounterEntropy(2202),
        &mut TestKeyAdapter,
        CounterState::default(),
    )
    .unwrap();
    let fourth = coordinator
        .commit(
            &mut fs,
            request(4, 14, &mutation(3, 1)),
            &mut clock(4),
            &NeverCancel,
        )
        .unwrap();
    drop(coordinator);
    let (mut recovery, transactions) = transactions_with_entropy(&mut fs, &name, 4, 2302);
    let roots = recovery
        .discover_packed_roots_at_revision(
            &mut fs,
            COORDINATOR_PACKED_PROFILE_V1,
            old.anchor().0,
            limits().certificates,
            PackedRootDiscoveryLimits::new(2, 8354).unwrap(),
        )
        .unwrap()
        .0;
    let (base, report) =
        admit_packed_coordinator_prefix(&mut recovery, &mut fs, &roots[0], admission()).unwrap();
    assert_eq!(report.certificate.certificates, 2);
    assert_eq!(base.anchor().0.get(), 3);
    let (next, report) = stage_packed_coordinator_prefix(
        &mut recovery,
        &mut fs,
        Some(&base),
        &transactions[3],
        limits(),
    )
    .unwrap();
    assert_eq!(next.anchor().0, fourth.revision);
    assert_eq!(report.new_owners, 0);
    let maintenance = recovery
        .packed_indexes_with_io(&mut fs, &transactions[3], limits().certificates)
        .unwrap();
    assert!(
        base.transaction(&maintenance, &mut fs, fourth.transaction_id, reads())
            .unwrap()
            .0
            .is_none()
    );
    assert_eq!(
        next.transaction(&maintenance, &mut fs, fourth.transaction_id, reads())
            .unwrap()
            .0
            .unwrap()
            .1,
        fourth
    );
    assert_eq!(next.owner_count(), 2);
}
