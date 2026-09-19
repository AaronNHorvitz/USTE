use super::*;
use uste_txn::{PackedQuotaRebuildLimits, rebuild_packed_quota_prefix};

fn rebuild_limits(batch: usize) -> PackedQuotaRebuildLimits {
    let mut batch_limits = batches();
    batch_limits.maximum_deltas = 512;
    batch_limits.maximum_input_bytes = 128_000;
    PackedQuotaRebuildLimits {
        certificates: limits().certificates,
        cursor: cursors(),
        lookup: reads(),
        batch: batch_limits,
        maximum_owners: 4,
        maximum_batch_owners: batch,
        maximum_batches: 4,
        maximum_lookup_pages: 1000,
        maximum_lookup_bytes: 1000 * 20545,
    }
}

#[test]
fn packed_quota_rebuild_partition_independence_matches_inductive_and_cold_admission() {
    for count in [1, 5] {
        for batch in [1, 2, 3, 512] {
            let (mut fs, _, mut recovery, primary, expected, _, root) = root_fixture(count);
            let (actual, report) = rebuild_packed_quota_prefix(
                &mut recovery,
                &mut fs,
                &primary,
                rebuild_limits(batch),
            )
            .unwrap();
            assert_eq!(actual.anchor(), primary.anchor());
            assert_eq!(
                actual.families().map(|f| f.commitment),
                expected.families().map(|f| f.commitment)
            );
            assert_eq!(
                report.batches,
                primary.owner_count().max(1).div_ceil(batch as u64)
            );
            assert!(report.maximum_batch_owners <= batch);
            assert_eq!(report.cursor.returned_entries, primary.owner_count());
            let published = recovery
                .publish_recovered_packed_root(
                    &mut fs,
                    COORDINATOR_PACKED_USAGE_PROFILE_V1,
                    root.manifest().claims(),
                    &actual.families(),
                    2,
                )
                .unwrap();
            let admitted = admit_packed_quota_prefix(
                &mut recovery,
                &mut fs,
                &primary,
                &published,
                admission(),
            )
            .unwrap()
            .0;
            assert_eq!(
                admitted.families().map(|f| f.commitment),
                expected.families().map(|f| f.commitment)
            );
            admit_packed_quota_prefix(&mut recovery, &mut fs, &primary, &root, admission())
                .unwrap();
        }
    }
}

#[test]
fn packed_quota_rebuild_admission_and_aggregate_lookup_limits_preserve_old_projection() {
    let (mut fs, _, mut recovery, primary, _, _, root) = root_fixture(5);
    fs.arm(FaultPlan::default()).unwrap();
    for selected in [
        PackedQuotaRebuildLimits {
            maximum_batch_owners: 0,
            ..rebuild_limits(1)
        },
        PackedQuotaRebuildLimits {
            maximum_batch_owners: 513,
            ..rebuild_limits(1)
        },
        PackedQuotaRebuildLimits {
            maximum_batches: 2,
            ..rebuild_limits(1)
        },
        PackedQuotaRebuildLimits {
            maximum_owners: 2,
            ..rebuild_limits(1)
        },
    ] {
        assert!(rebuild_packed_quota_prefix(&mut recovery, &mut fs, &primary, selected).is_err());
    }
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    let (_, report) =
        rebuild_packed_quota_prefix(&mut recovery, &mut fs, &primary, rebuild_limits(1)).unwrap();
    assert!(report.lookup_pages > 0);
    let exact = PackedQuotaRebuildLimits {
        maximum_lookup_pages: report.lookup_pages,
        maximum_lookup_bytes: report.lookup_bytes,
        ..rebuild_limits(1)
    };
    rebuild_packed_quota_prefix(&mut recovery, &mut fs, &primary, exact).unwrap();
    for selected in [
        PackedQuotaRebuildLimits {
            maximum_lookup_pages: report.lookup_pages - 1,
            ..exact
        },
        PackedQuotaRebuildLimits {
            maximum_lookup_bytes: report.lookup_bytes - 1,
            ..exact
        },
    ] {
        assert!(rebuild_packed_quota_prefix(&mut recovery, &mut fs, &primary, selected).is_err());
    }
    admit_packed_quota_prefix(&mut recovery, &mut fs, &primary, &root, admission()).unwrap();
}

#[test]
fn packed_quota_rebuild_every_observed_fault_keeps_published_pair_and_restarts() {
    let (mut observed_fs, _, mut observed, primary, _, _, _) = root_fixture(5);
    observed_fs.arm(FaultPlan::default()).unwrap();
    let expected =
        rebuild_packed_quota_prefix(&mut observed, &mut observed_fs, &primary, rebuild_limits(1))
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
                let (mut fs, name, mut recovery, primary, _, _, _) = root_fixture(5);
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
                    rebuild_packed_quota_prefix(
                        &mut recovery,
                        &mut fs,
                        &primary,
                        rebuild_limits(1)
                    )
                    .is_err(),
                    "{operation:?}/{occurrence}/{action:?}"
                );
                assert_eq!(fs.pending_faults(), 0);
                drop(recovery);
                fs.restart().unwrap();
                fs.arm(FaultPlan::default()).unwrap();
                let (mut recovery, _) = transactions_with_entropy(&mut fs, &name, 5, 2902);
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
                };
                let primary_roots = discover(COORDINATOR_PACKED_PROFILE_V1);
                let quota_roots = discover(COORDINATOR_PACKED_USAGE_PROFILE_V1);
                assert_eq!(primary_roots.len(), 1);
                assert_eq!(quota_roots.len(), 1);
                let primary = admit_packed_coordinator_prefix(
                    &mut recovery,
                    &mut fs,
                    &primary_roots[0],
                    primary_limits(5),
                )
                .unwrap()
                .0;
                let old = admit_packed_quota_prefix(
                    &mut recovery,
                    &mut fs,
                    &primary,
                    &quota_roots[0],
                    admission(),
                )
                .unwrap()
                .0;
                let rebuilt = rebuild_packed_quota_prefix(
                    &mut recovery,
                    &mut fs,
                    &primary,
                    rebuild_limits(1),
                )
                .unwrap()
                .0;
                assert_eq!(
                    rebuilt.families().map(|f| f.commitment),
                    old.families().map(|f| f.commitment)
                );
                assert_eq!(
                    rebuilt.families().map(|f| f.commitment),
                    expected.families().map(|f| f.commitment)
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 318);
}

#[test]
fn packed_quota_rebuild_exact_512_owner_batch_matches_inductive_construction() {
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let name = EntryName::new("packed-quota-maximum").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 3001),
        CounterEntropy(3002),
        CounterState::default(),
    )
    .unwrap();
    let mut references = Vec::new();
    for index in 0_u64..512 {
        let mut upload = coordinator.start_blob_upload(scope()).unwrap();
        if index != 0 {
            coordinator
                .write_blob_upload(&mut fs, &mut upload, &index.to_be_bytes())
                .unwrap();
        }
        references.push(
            coordinator
                .finish_blob_upload(&mut fs, &mut upload)
                .unwrap(),
        );
    }
    let inventory = BlobInventory::new(scope(), references.iter().copied()).unwrap();
    coordinator
        .commit(
            &mut fs,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(1, 11, &mutation(0, 1))
            },
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();
    drop(coordinator);
    let (mut recovery, transactions) = transactions(&mut fs, &name, 1);
    let mut construction = limits();
    construction.maximum_references = 512;
    construction.maximum_owners = 512;
    construction.batch.maximum_deltas = 512;
    construction.batch.maximum_input_bytes = 128_000;
    construction.batch.maximum_dirty_nodes = 4096;
    construction.batch.pack = PackWriteLimits {
        maximum_pages: 128,
        maximum_records: 4096,
        maximum_payload_bytes: 1_000_000,
    };
    let primary = stage_packed_coordinator_prefix(
        &mut recovery,
        &mut fs,
        None,
        &transactions[0],
        construction,
    )
    .unwrap()
    .0;
    let expected = stage_packed_quota_prefix(
        &mut recovery,
        &mut fs,
        None,
        &primary,
        &transactions[0],
        construction,
    )
    .unwrap()
    .0;
    let mut selected = rebuild_limits(512);
    selected.maximum_owners = 512;
    selected.batch = construction.batch;
    selected.cursor.maximum_candidates = 512;
    selected.cursor.maximum_returned_bytes = 128_000;
    selected.cursor.maximum_pages = 10_000;
    selected.cursor.maximum_encoded_bytes = 10_000 * 20545;
    let (actual, report) =
        rebuild_packed_quota_prefix(&mut recovery, &mut fs, &primary, selected).unwrap();
    assert_eq!(report.batches, 1);
    assert_eq!(report.maximum_batch_owners, 512);
    assert_eq!(
        actual.families().map(|f| f.commitment),
        expected.families().map(|f| f.commitment)
    );
    let maintenance = recovery
        .packed_indexes_with_io(&mut fs, &transactions[0], limits().certificates)
        .unwrap();
    let (usage, _) = actual
        .usage(&maintenance, &mut fs, &primary, principal(0), reads())
        .unwrap();
    assert_eq!(usage.owners, 512);
    assert_eq!(usage.namespace_bytes, 511 * 8);
    assert_eq!(usage.principal_bytes, 511 * 8);
}

#[test]
fn packed_quota_rebuild_foreign_primary_and_corrupt_source_do_not_publish_accounting() {
    let (mut fs, name, mut recovery, primary, _, _, _) = root_fixture(5);
    let location = primary.families()[2]
        .root
        .unwrap()
        .resolve(
            scope(),
            COORDINATOR_PACKED_PROFILE_V1,
            3,
            primary.anchor().0,
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
        rebuild_packed_quota_prefix(&mut recovery, &mut fs, &primary, rebuild_limits(1)).is_err()
    );
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    let (mut foreign_fs, _, mut foreign, _, _, _, _) = root_fixture(5);
    foreign_fs.arm(FaultPlan::default()).unwrap();
    assert!(
        rebuild_packed_quota_prefix(&mut foreign, &mut foreign_fs, &primary, rebuild_limits(1))
            .is_err()
    );
    assert_eq!(foreign_fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(foreign_fs.operation_count(Operation::CreateNew), 0);
}
