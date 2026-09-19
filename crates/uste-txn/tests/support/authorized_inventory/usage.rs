use super::*;

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
