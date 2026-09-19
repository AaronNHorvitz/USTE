use super::*;

fn merge_limits() -> uste_txn::CoordinatorMetadataRebaseLimits {
    uste_txn::CoordinatorMetadataRebaseLimits {
        merge: IndexRunMergeLimits::new(run(), 32, 8192, 32, 8192).unwrap(),
        reuse: run(),
    }
}

fn usage_limits() -> uste_txn::CoordinatorBlobUsageLimits {
    uste_txn::CoordinatorBlobUsageLimits {
        run: run(),
        lookup: lookup(),
        maximum_owners: 4,
    }
}

pub(super) fn bootstrap(fs: &mut Fs, disk: &mut Disk) {
    disk.bootstrap_blob_usage_index(fs, merge_limits(), usage_limits())
        .unwrap();
}

pub(super) fn inventory_facade<'a>(
    disk: &'a mut Disk,
    kernel: &'a PolicyKernel,
    indexed: bool,
) -> AuthorizedDiskUploads<'a, PolicyCounter, Fs, TestEnvelope, CounterEntropy, CounterEntropy> {
    if indexed {
        AuthorizedDiskUploads::new_with_indexed_inventory_commits(disk, kernel, 4, append_limits())
            .unwrap()
    } else {
        AuthorizedDiskUploads::new_with_inventory_commits(
            disk,
            kernel,
            accounting(),
            append_limits(),
        )
        .unwrap()
    }
}

#[test]
fn authorized_indexed_usage_requires_admission_checks_authority_and_fails_closed_on_reads() {
    let (mut fs, _, mut disk, _, kernel, alice, bob) = fixture();
    assert_eq!(
        AuthorizedDiskUploads::new_with_indexed_inventory_commits(
            &mut disk,
            &kernel,
            4,
            append_limits()
        )
        .err(),
        Some(AuthorizedError::Transaction(
            TransactionError::InvalidRequest
        ))
    );
    bootstrap(&mut fs, &mut disk);
    let mut staging =
        AuthorizedDiskUploads::new_with_indexed_accounting(&mut disk, &kernel, 4).unwrap();
    assert!(
        !staging
            .quota_usage(&mut fs, &alice)
            .unwrap()
            .staging_complete
    );
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        staging.commit_inventory(
            &mut fs,
            &alice,
            raw(2, &mutation(0, 1), None),
            &mut ScriptedClock::new([]),
            &NeverCancel
        ),
        Err(AuthorizedDiskInventoryError::Authorization(
            AuthorizedError::Transaction(TransactionError::InvalidRequest)
        ))
    );
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
    drop(staging);
    let mut uploads = inventory_facade(&mut disk, &kernel, true);
    uploads
        .complete_recovered_upload_reconciliation(&mut fs, &alice, &[])
        .unwrap();
    let reference = finish(&mut uploads, &mut fs, &alice, b"abc");
    let inventory = BlobInventory::new(scope(), [reference]).unwrap();
    fs.arm(
        FaultPlan::new([FaultPoint {
            operation: Operation::ReadAt,
            occurrence: 1,
            action: FaultAction::Error(AdapterErrorKind::Io),
        }])
        .unwrap(),
    )
    .unwrap();
    assert!(
        uploads
            .commit_inventory(
                &mut fs,
                &alice,
                raw(2, &mutation(0, 1), Some(&inventory)),
                &mut ScriptedClock::new([]),
                &NeverCancel
            )
            .is_err()
    );
    assert_eq!(fs.pending_faults(), 0);
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
    assert_eq!(
        uploads
            .quota_usage(&mut fs, &alice)
            .unwrap()
            .known_usage
            .principal_staged_bytes,
        3
    );
    uploads
        .commit_inventory(
            &mut fs,
            &alice,
            raw(2, &mutation(0, 1), Some(&inventory)),
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();
    drop(uploads);
    disk.rebase_metadata_with_blob_usage(
        &mut fs,
        merge_limits(),
        uste_txn::CoordinatorFirstReferenceLimits {
            maximum_owners: 1,
            maximum_groups: 1,
            maximum_encoded_bytes: 1_000_000,
        },
        usage_limits(),
    )
    .unwrap();
    let facade = uste_txn::AuthorizedDiskMetadata::new(&disk, &kernel).unwrap();
    let alice_usage = facade
        .committed_blob_usage_indexed(&mut fs, &alice, 4)
        .unwrap();
    let bob_usage = facade
        .committed_blob_usage_indexed(&mut fs, &bob, 4)
        .unwrap();
    assert_eq!(
        (
            alice_usage.namespace_bytes,
            alice_usage.principal_bytes,
            bob_usage.principal_bytes
        ),
        (3, 3, 0)
    );
    let denied = authenticated(&kernel, 3);
    let mut foreign_kernel = PolicyKernel::new();
    foreign_kernel.install_initial_policy(policy(1)).unwrap();
    let foreign = authenticated(&foreign_kernel, 1);
    fs.arm(FaultPlan::default()).unwrap();
    for principal in [&denied, &foreign] {
        assert_eq!(
            facade.committed_blob_usage_indexed(&mut fs, principal, 4),
            Err(AuthorizedError::Unauthorized)
        );
    }
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    drop(facade);
    // Fresh fixed cache per accounting call: every actual read failure must refuse totals.
    let uploads = inventory_facade(&mut disk, &kernel, true);
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        uploads
            .quota_usage(&mut fs, &alice)
            .unwrap()
            .known_usage
            .principal_committed_bytes,
        3
    );
    let reads = fs.operation_count(Operation::ReadAt);
    assert!(reads > 0);
    eprintln!("indexed quota: {reads} authenticated page reads, each faulted independently");
    for occurrence in 1..=reads {
        fs.arm(
            FaultPlan::new([FaultPoint {
                operation: Operation::ReadAt,
                occurrence,
                action: FaultAction::Error(AdapterErrorKind::Io),
            }])
            .unwrap(),
        )
        .unwrap();
        assert!(uploads.quota_usage(&mut fs, &alice).is_err());
        assert_eq!(fs.pending_faults(), 0);
    }
    assert_eq!(
        uploads
            .quota_usage(&mut fs, &alice)
            .unwrap()
            .known_usage
            .principal_committed_bytes,
        3
    );
}

#[test]
fn disk_blob_usage_owner_free_bootstrap_and_zero_length_owner_survive_restart() {
    let (mut fs, name, mut disk, base, kernel, alice, _) = fixture();
    let merge = uste_txn::CoordinatorMetadataRebaseLimits {
        merge: IndexRunMergeLimits::new(run(), 32, 8192, 32, 8192).unwrap(),
        reuse: run(),
    };
    let usage = uste_txn::CoordinatorBlobUsageLimits {
        run: run(),
        lookup: lookup(),
        maximum_owners: 4,
    };
    for _ in 0..2 {
        // exact-root reuse must resynchronize rather than change the journal
        disk.bootstrap_blob_usage_index(&mut fs, merge, usage)
            .unwrap();
        assert_eq!(
            disk.checkpoint_anchor().unwrap().unwrap().0,
            CommitRevision::FIRST
        );
    }
    let rebuilt = disk
        .rebuild_blob_usage_index(
            &mut fs,
            uste_txn::CoordinatorBlobUsageRebuildLimits {
                source: run(),
                admission: usage,
                merge,
                maximum_batch_owners: 1,
                maximum_batches: 1,
                maximum_merge_output_bytes: 37,
            },
            &mut cache(),
        )
        .unwrap();
    assert_eq!(
        (
            rebuilt.owners,
            rebuilt.principals,
            rebuilt.batches,
            rebuilt.merge_output_bytes
        ),
        (0, 0, 1, 37)
    );
    drop(disk);
    fs.restart().unwrap();
    let mut disk = reopen(&mut fs, &name, base.clone());
    assert_eq!(
        disk.committed_blob_usage_indexed(&mut fs, alice.digest(), 0, lookup(), &mut cache())
            .unwrap(),
        uste_txn::CommittedBlobUsage::default()
    );
    let mut uploads = AuthorizedDiskUploads::new_with_inventory_commits(
        &mut disk,
        &kernel,
        accounting(),
        append_limits(),
    )
    .unwrap();
    uploads
        .complete_recovered_upload_reconciliation(&mut fs, &alice, &[])
        .unwrap();
    let reference = finish(&mut uploads, &mut fs, &alice, b"");
    let inventory = BlobInventory::new(scope(), [reference]).unwrap();
    uploads
        .commit_inventory(
            &mut fs,
            &alice,
            raw(2, &mutation(0, 1), Some(&inventory)),
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();
    drop(uploads);
    let indexed = disk
        .committed_blob_usage_indexed(&mut fs, alice.digest(), 1, lookup(), &mut cache())
        .unwrap();
    assert_eq!(
        (
            indexed.owners,
            indexed.namespace_bytes,
            indexed.principal_bytes
        ),
        (1, 0, 0)
    );
    drop(disk);
    fs.restart().unwrap();
    let mut disk = reopen(&mut fs, &name, base);
    assert_eq!(
        disk.committed_blob_usage_indexed(&mut fs, alice.digest(), 1, lookup(), &mut cache())
            .unwrap(),
        indexed
    );
    disk.rebase_metadata_with_blob_usage(
        &mut fs,
        merge,
        uste_txn::CoordinatorFirstReferenceLimits {
            maximum_owners: 1,
            maximum_groups: 1,
            maximum_encoded_bytes: 1_000_000,
        },
        usage,
    )
    .unwrap();
    assert_eq!(
        disk.committed_blob_usage_indexed(&mut fs, alice.digest(), 1, lookup(), &mut cache())
            .unwrap(),
        indexed
    );
}
