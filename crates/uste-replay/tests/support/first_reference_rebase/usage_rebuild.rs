use super::*;

fn current() -> (Fs, EntryName, FaultDiskCounter, CounterState) {
    let (mut fs, name, mut disk, _, _) = fixture(true);
    disk.rebase_metadata_with_first_references(&mut fs, metadata_rebase_limits(), suffix())
        .unwrap();
    let state = disk.state().unwrap().clone();
    (fs, name, disk, state)
}

fn rebuild_limits(batch: usize) -> uste_txn::CoordinatorBlobUsageRebuildLimits {
    uste_txn::CoordinatorBlobUsageRebuildLimits {
        source: limits().run,
        admission: limits(),
        merge: metadata_rebase_limits(),
        maximum_batch_owners: batch,
        maximum_batches: 2,
        maximum_merge_output_bytes: if batch == 1 { 602 } else { 389 },
    }
}

#[test]
fn quota_rebuild_populated_base_exact_limits_private_failure_and_cold_admission() {
    for batch in [1, 2, uste_txn::MAX_BLOB_USAGE_REBUILD_BATCH_OWNERS] {
        let (mut fs, name, mut disk, state) = current();
        let exact = rebuild_limits(batch);
        for bad in [
            uste_txn::CoordinatorBlobUsageRebuildLimits {
                maximum_batch_owners: 0,
                ..exact
            },
            uste_txn::CoordinatorBlobUsageRebuildLimits {
                maximum_batch_owners: 4097,
                ..exact
            },
            uste_txn::CoordinatorBlobUsageRebuildLimits {
                maximum_batches: 0,
                ..exact
            },
            uste_txn::CoordinatorBlobUsageRebuildLimits {
                admission: uste_txn::CoordinatorBlobUsageLimits {
                    maximum_owners: 1,
                    ..limits()
                },
                ..exact
            },
        ] {
            fs.arm(FaultPlan::default()).unwrap();
            assert!(
                disk.rebuild_blob_usage_index(
                    &mut fs,
                    bad,
                    &mut PageCache::new(64 * 1024).unwrap()
                )
                .is_err()
            );
            assert_eq!(fs.operation_count(Operation::ReadAt), 0);
            assert_eq!(fs.operation_count(Operation::CreateNew), 0);
        }
        fs.arm(FaultPlan::default()).unwrap();
        assert!(
            disk.rebuild_blob_usage_index(
                &mut fs,
                uste_txn::CoordinatorBlobUsageRebuildLimits {
                    maximum_merge_output_bytes: exact.maximum_merge_output_bytes - 1,
                    ..exact
                },
                &mut PageCache::new(64 * 1024).unwrap()
            )
            .is_err()
        );
        assert_eq!(disk.checkpoint_anchor().unwrap().unwrap().0.get(), 2);
        assert_eq!(disk.overlay_counts(), (0, 0));
        assert!(!disk.rebase_required());
        assert!(
            disk.load_index_root_manifests(&mut fs, uste_txn::COORDINATOR_BLOB_USAGE_PROFILE_V1)
                .unwrap()
                .is_empty()
        );
        // Complete construction is still not publication: independent terminal admission can
        // refuse the owner run (2 * 128 logical bytes), leaving every scratch root private.
        assert!(
            disk.rebuild_blob_usage_index(
                &mut fs,
                uste_txn::CoordinatorBlobUsageRebuildLimits {
                    admission: uste_txn::CoordinatorBlobUsageLimits {
                        run: IndexRunReadLimits::new(16, 10, 255).unwrap(),
                        ..limits()
                    },
                    ..exact
                },
                &mut PageCache::new(64 * 1024).unwrap()
            )
            .is_err()
        );
        assert!(
            disk.load_index_root_manifests(&mut fs, uste_txn::COORDINATOR_BLOB_USAGE_PROFILE_V1)
                .unwrap()
                .is_empty()
        );
        let report = disk
            .rebuild_blob_usage_index(&mut fs, exact, &mut PageCache::new(64 * 1024).unwrap())
            .unwrap();
        assert_eq!(
            (report.owners, report.principals, report.source.entries),
            (2, 2, 2)
        );
        assert_eq!(report.source.logical_bytes, 192);
        assert_eq!(report.batches, 2_u64.div_ceil(batch as u64));
        assert_eq!(report.merge_output_bytes, exact.maximum_merge_output_bytes);
        compare(&mut fs, &disk);
        drop(disk);
        fs.restart().unwrap();
        let mut disk = reopen_certificate_mode(&mut fs, &name, state, true);
        compare(&mut fs, &disk);
        // Explicit replacement has the same independent accounting and cannot double-charge.
        disk.rebuild_blob_usage_index(&mut fs, exact, &mut PageCache::new(64 * 1024).unwrap())
            .unwrap();
        compare(&mut fs, &disk);
    }
}

#[test]
fn quota_rebuild_every_io_boundary_preserves_the_authoritative_ledger() {
    let (mut fs, _, mut disk, _) = current();
    fs.arm(FaultPlan::default()).unwrap();
    disk.rebuild_blob_usage_index(
        &mut fs,
        rebuild_limits(1),
        &mut PageCache::new(64 * 1024).unwrap(),
    )
    .unwrap();
    let counts = [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SetLen,
        Operation::SyncAll,
        Operation::SyncDirectory,
        Operation::RemoveFile,
    ]
    .map(|operation| (operation, fs.operation_count(operation)));
    drop(disk);
    let mut attempts = 0;
    let mut successes = 0;
    for (operation, count) in counts {
        assert!(count > 0);
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut fs, name, mut disk, state) = current();
                fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                let result = disk.rebuild_blob_usage_index(
                    &mut fs,
                    rebuild_limits(1),
                    &mut PageCache::new(64 * 1024).unwrap(),
                );
                assert_eq!(fs.pending_faults(), 0);
                assert_eq!(disk.overlay_counts(), (0, 0));
                assert_eq!(disk.state().unwrap(), &state);
                if result.is_ok() {
                    assert!(!fs.is_crashed());
                    eprintln!(
                        "quota rebuild optional success: {operation:?}/{occurrence}/{action:?}"
                    );
                    compare(&mut fs, &disk);
                    successes += 1;
                }
                drop(disk);
                fs.restart().unwrap();
                let mut disk = reopen(&mut fs, &name, state);
                disk.rebuild_blob_usage_index(
                    &mut fs,
                    rebuild_limits(1),
                    &mut PageCache::new(64 * 1024).unwrap(),
                )
                .unwrap();
                compare(&mut fs, &disk);
                attempts += 1;
            }
        }
    }
    eprintln!("quota rebuild: {attempts} fault attempts, {successes} optional-cache successes");
    assert!(attempts > 100);
}

#[test]
fn quota_rebuild_corrupt_terminal_certificate_cannot_publish_private_batches() {
    use uste_storage::FileSystem;
    let (mut fs, name, disk, state) = current();
    drop(disk);
    fs.restart().unwrap();
    let mut disk = reopen_certificate_mode(&mut fs, &name, state, true);
    let directory = fs.open_directory(&fs.root(), &name).unwrap();
    let certificates = fs
        .open_existing(&directory, &EntryName::new("CERTIFICATES").unwrap())
        .unwrap();
    let offset = 2 * 4161 + 100;
    let mut byte = [0];
    assert_eq!(fs.read_at(&certificates, offset, &mut byte).unwrap(), 1);
    byte[0] ^= 1;
    assert_eq!(fs.write_at(&certificates, offset, &byte).unwrap(), 1);
    fs.sync_all(&certificates).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        disk.rebuild_blob_usage_index(
            &mut fs,
            rebuild_limits(1),
            &mut PageCache::new(64 * 1024).unwrap()
        )
        .is_err()
    );
    assert!(fs.operation_count(Operation::CreateNew) > 0);
    assert!(
        disk.load_index_root_manifests(&mut fs, uste_txn::COORDINATOR_BLOB_USAGE_PROFILE_V1)
            .unwrap()
            .is_empty()
    );
    byte[0] ^= 1;
    assert_eq!(fs.write_at(&certificates, offset, &byte).unwrap(), 1);
    fs.sync_all(&certificates).unwrap();
    disk.rebuild_blob_usage_index(
        &mut fs,
        rebuild_limits(1),
        &mut PageCache::new(64 * 1024).unwrap(),
    )
    .unwrap();
    compare(&mut fs, &disk);
}

#[test]
fn quota_rebuild_cross_batch_principal_updates_match_literal_first_owner_totals() {
    let (mut fs, _, mut disk, _) = current();
    let mut expected = [
        0_u64,
        b"first owner remains principal one".len() as u64,
        b"new owner is principal two".len() as u64,
        0,
    ];
    for number in 3_u8..=8 {
        let principal = number % 3 + 1;
        let bytes = vec![number; usize::from(number)];
        let mut upload = disk.start_blob_upload(scope()).unwrap();
        disk.write_blob_upload(&mut fs, &mut upload, &bytes)
            .unwrap();
        let reference = disk.finish_blob_upload(&mut fs, &mut upload).unwrap();
        let inventory = BlobInventory::new(scope(), [reference]).unwrap();
        disk.commit(
            &mut fs,
            TransactionRequest {
                principal: PrincipalDigest::from_bytes([principal; 32]),
                blob_inventory: Some(&inventory),
                ..request(number, &u64::from(number).to_be_bytes())
            },
            &mut TestClock(20),
            &NeverCancel,
            limits().lookup,
            &mut PageCache::new(64 * 1024).unwrap(),
        )
        .unwrap();
        disk.rebase_metadata_with_first_references(&mut fs, metadata_rebase_limits(), suffix())
            .unwrap();
        expected[usize::from(principal)] += u64::from(number);
    }
    let report = disk
        .rebuild_blob_usage_index(
            &mut fs,
            uste_txn::CoordinatorBlobUsageRebuildLimits {
                admission: uste_txn::CoordinatorBlobUsageLimits {
                    maximum_owners: 8,
                    ..limits()
                },
                maximum_batch_owners: 3,
                maximum_batches: 3,
                maximum_merge_output_bytes: 2719,
                ..rebuild_limits(3)
            },
            &mut PageCache::new(64 * 1024).unwrap(),
        )
        .unwrap();
    assert_eq!(
        (
            report.owners,
            report.principals,
            report.batches,
            report.source.entries
        ),
        (8, 3, 3, 8)
    );
    for principal in 0_u8..=3 {
        let usage = disk
            .committed_blob_usage_indexed(
                &mut fs,
                PrincipalDigest::from_bytes([principal; 32]),
                8,
                limits().lookup,
                &mut PageCache::new(64 * 1024).unwrap(),
            )
            .unwrap();
        assert_eq!(usage.owners, 8);
        assert_eq!(usage.namespace_bytes, expected.iter().sum());
        assert_eq!(usage.principal_bytes, expected[usize::from(principal)]);
    }
}
