use super::*;

#[path = "usage_admission.rs"]
mod admission;

pub(super) fn limits() -> uste_txn::CoordinatorBlobUsageLimits {
    uste_txn::CoordinatorBlobUsageLimits {
        run: IndexRunReadLimits::new(16, 10, 4096).unwrap(),
        lookup: IndexGetLimits::new(16, 136).unwrap(),
        maximum_owners: 2,
    }
}

fn rebase(fs: &mut Fs, disk: &mut FaultDiskCounter) -> Result<(), TransactionError> {
    disk.rebase_metadata_with_blob_usage(fs, metadata_rebase_limits(), suffix(), limits())
}

fn compare(fs: &mut Fs, disk: &FaultDiskCounter) {
    for principal in [1, 2, 3, 99] {
        let principal = PrincipalDigest::from_bytes([principal; 32]);
        let scan = disk
            .committed_blob_usage(
                fs,
                principal,
                uste_txn::DiskBlobAccountingLimits {
                    base: limits().run,
                    maximum_total_owners: 2,
                },
            )
            .unwrap();
        let indexed = disk
            .committed_blob_usage_indexed(
                fs,
                principal,
                2,
                limits().lookup,
                &mut PageCache::new(64 * 1024).unwrap(),
            )
            .unwrap();
        assert_eq!(scan, indexed);
    }
}

#[test]
fn indexed_blob_usage_preserves_first_owner_overlays_rebase_and_cold_admission() {
    for next_principal in [2, 3] {
        let (mut fs, name, mut disk, _, inventory) = fixture(false);
        assert!(
            disk.committed_blob_usage_indexed(
                &mut fs,
                PrincipalDigest::from_bytes([2; 32]),
                2,
                limits().lookup,
                &mut PageCache::new(64 * 1024).unwrap()
            )
            .is_err()
        );
        rebase(&mut fs, &mut disk).unwrap();
        compare(&mut fs, &disk);
        let state = disk.state().unwrap().clone();
        drop(disk);
        fs.restart().unwrap();
        let mut disk = reopen_certificate_mode(&mut fs, &name, state, true);
        compare(&mut fs, &disk);
        // A new owner and an old reference in one later principal's transaction charge only the new.
        let mut upload = disk.start_blob_upload(scope()).unwrap();
        disk.write_blob_upload(&mut fs, &mut upload, b"third")
            .unwrap();
        let new = disk.finish_blob_upload(&mut fs, &mut upload).unwrap();
        let inventory =
            BlobInventory::new(scope(), inventory.references().iter().copied().chain([new]))
                .unwrap();
        disk.commit(
            &mut fs,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                principal: PrincipalDigest::from_bytes([next_principal; 32]),
                ..request(3, &3_u64.to_be_bytes())
            },
            &mut TestClock(20),
            &NeverCancel,
            limits().lookup,
            &mut PageCache::new(64 * 1024).unwrap(),
        )
        .unwrap();
        compare(&mut fs, &disk);
        // Omitting the projection cannot silently drop it or poison a still usable coordinator.
        assert!(
            disk.rebase_metadata_with_first_references(&mut fs, metadata_rebase_limits(), suffix())
                .is_err()
        );
        assert!(!disk.rebase_required());
        fs.arm(FaultPlan::default()).unwrap();
        assert!(
            disk.committed_blob_usage_indexed(
                &mut fs,
                PrincipalDigest::from_bytes([2; 32]),
                1,
                limits().lookup,
                &mut PageCache::new(64 * 1024).unwrap()
            )
            .is_err()
        );
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        rebase(&mut fs, &mut disk).unwrap();
        compare(&mut fs, &disk);
        let state = disk.state().unwrap().clone();
        drop(disk);
        fs.restart().unwrap();
        let disk = reopen_certificate_mode(&mut fs, &name, state, true);
        compare(&mut fs, &disk);
    }
}

#[test]
fn indexed_blob_usage_every_publication_fault_preserves_retry_and_restart() {
    let (mut fs, _, mut sample, _, _) = fixture(false);
    fs.arm(FaultPlan::default()).unwrap();
    rebase(&mut fs, &mut sample).unwrap();
    let counts = [
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SetLen,
        Operation::SyncAll,
        Operation::SyncDirectory,
        Operation::RemoveFile,
    ]
    .map(|op| (op, fs.operation_count(op)));
    drop(sample);
    let mut attempts = 0;
    let mut cleanup_successes = 0;
    for (operation, count) in counts {
        assert!(count > 0);
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut fs, name, mut disk, base_state, _) = fixture(false);
                let current = disk.state().unwrap().clone();
                fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                let result = rebase(&mut fs, &mut disk);
                if result.is_ok() {
                    assert_eq!(operation, Operation::RemoveFile);
                    cleanup_successes += 1;
                    eprintln!("quota cleanup succeeded: {operation:?}/{occurrence}/{action:?}");
                }
                assert_eq!(fs.pending_faults(), 0);
                assert_eq!(
                    disk.overlay_counts(),
                    if result.is_ok() { (0, 0) } else { (1, 1) }
                );
                drop(disk);
                fs.restart().unwrap();
                let mut disk = reopen(
                    &mut fs,
                    &name,
                    if result.is_ok() {
                        current.clone()
                    } else {
                        base_state
                    },
                );
                rebase(&mut fs, &mut disk).unwrap();
                compare(&mut fs, &disk);
                drop(disk);
                fs.restart().unwrap();
                let disk = reopen(&mut fs, &name, current);
                compare(&mut fs, &disk);
                attempts += 1;
            }
        }
    }
    eprintln!(
        "quota publication: {attempts} fault attempts, {cleanup_successes} cleanup successes"
    );
    assert!(attempts >= 45);
}
