use super::*;

fn append_limits() -> DiskBlobAppendLimits {
    DiskBlobAppendLimits {
        maximum_pending_blobs: 2,
        maximum_pending_inventories: 2,
        maximum_pending_namespaces: 2,
        maximum_inventory_references: 2,
        maximum_verified_blob_bytes: 8,
        lookup: limits().admission.lookup,
    }
}

fn open_fixture(empty: bool) -> Fixture {
    let mut f = Fixture::new(empty);
    let database = f.store.database;
    drop(f.store);
    f.fs.restart().unwrap();
    f.store = cold::reopen(&mut f.fs, database, cold::recovery_limits(), |_| Ok(()))
        .unwrap()
        .0;
    f
}

fn upload(f: &mut Fixture, namespace: u8, bytes: &[u8]) -> BlobInventory {
    let scope = NamespaceRef::new(
        f.store.database,
        uste_types::NamespaceId::from_bytes([namespace; 16]),
    );
    let mut upload = f.store.start_blob_upload(scope).unwrap();
    f.store
        .write_blob_upload(&mut f.fs, &mut upload, bytes)
        .unwrap();
    let reference = f.store.finish_blob_upload(&mut f.fs, &mut upload).unwrap();
    BlobInventory::new(scope, [reference]).unwrap()
}

fn append(
    f: &mut Fixture,
    inventory: &BlobInventory,
    limits: DiskBlobAppendLimits,
) -> Result<DurableCommit, StorageError> {
    f.store.append_group_with_disk_inventory(
        &mut f.fs,
        CommitInput {
            encoded_group: b"disk inventory",
            logical_event_digest: [0xA1; 32],
        },
        inventory,
        limits,
        &mut Fixture::cache(),
    )
}

#[test]
fn disk_blob_append_repeat_first_reference_refresh_and_restart() {
    let mut f = open_fixture(false);
    let old = BlobInventory::new(f.references[0].scope(), [f.references[0]]).unwrap();
    assert_eq!(
        append(&mut f, &old, append_limits())
            .unwrap()
            .revision
            .get(),
        6
    );
    assert_eq!(f.store.disk_blob_pending_residency(), Some((0, 0, 0)));
    let new = upload(&mut f, 0, b"four");
    assert_eq!(
        append(&mut f, &new, append_limits())
            .unwrap()
            .revision
            .get(),
        7
    );
    assert_eq!(f.store.disk_blob_pending_residency(), Some((1, 1, 1)));
    for (inventory, first) in [(&old, 1), (&new, 7)] {
        let reference = inventory.references()[0];
        assert_eq!(
            f.store
                .disk_blob_reference(
                    &mut f.fs,
                    reference.scope(),
                    reference.id(),
                    limits().admission.lookup,
                    &mut Fixture::cache()
                )
                .unwrap(),
            Some((reference, CommitRevision::new(first).unwrap()))
        );
    }
    let report = f
        .store
        .refresh_disk_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache())
        .unwrap();
    assert_eq!(report.admission.journal.groups, 7);
    assert_eq!(f.store.disk_blob_pending_residency(), Some((0, 0, 0)));
    assert_eq!(
        append(&mut f, &new, append_limits())
            .unwrap()
            .revision
            .get(),
        8
    );
    assert_eq!(f.store.disk_blob_pending_residency(), Some((0, 0, 0)));
    assert_eq!(f.store.blob_metadata_residency(), (false, 0, 0, 0));
    let database = f.store.database;
    drop(f.store);
    f.fs.restart().unwrap();
    let mut recovery = cold::recovery_limits();
    recovery.maximum_verified_blob_bytes_per_pass = 17;
    let (store, _) = cold::reopen(&mut f.fs, database, recovery, |_| Ok(())).unwrap();
    assert_eq!(
        store.disk_blob_metadata().unwrap().counts(),
        BlobMetadataCounts {
            blobs: 3,
            namespaces: 2,
            inventories: 3,
            reference_bindings: 7
        }
    );
    assert_eq!(store.disk_blob_pending_residency(), Some((0, 0, 0)));
    assert_eq!(store.blob_metadata_residency(), (false, 0, 0, 0));
    let reference = new.references()[0];
    assert_eq!(
        store
            .disk_blob_reference(
                &mut f.fs,
                reference.scope(),
                reference.id(),
                limits().admission.lookup,
                &mut Fixture::cache()
            )
            .unwrap()
            .unwrap()
            .1
            .get(),
        7
    );
}

#[test]
fn disk_blob_append_bounds_refuse_without_publication_and_exact_limits_pass() {
    let mut f = open_fixture(true);
    let inventory = upload(&mut f, 4, b"abc");
    for case in 0..5 {
        let mut admitted = append_limits();
        match case {
            0 => admitted.maximum_pending_blobs = 0,
            1 => admitted.maximum_pending_inventories = 0,
            2 => admitted.maximum_pending_namespaces = 0,
            3 => admitted.maximum_inventory_references = 0,
            _ => admitted.maximum_verified_blob_bytes = 2,
        }
        f.fs.arm(FaultPlan::default()).unwrap();
        assert!(
            matches!(
                append(&mut f, &inventory, admitted),
                Err(StorageError::ResourceLimit)
            ),
            "case {case}"
        );
        assert_eq!(f.store.frontier().unwrap().get(), 1);
        assert_eq!(f.store.disk_blob_pending_residency(), Some((0, 0, 0)));
        for operation in [
            Operation::WriteAt,
            Operation::SetLen,
            Operation::CreateNew,
            Operation::SyncData,
            Operation::SyncAll,
        ] {
            assert_eq!(f.fs.operation_count(operation), 0, "{operation:?}");
        }
    }
    let exact = DiskBlobAppendLimits {
        maximum_pending_blobs: 1,
        maximum_pending_inventories: 1,
        maximum_pending_namespaces: 1,
        maximum_inventory_references: 1,
        maximum_verified_blob_bytes: 3,
        lookup: append_limits().lookup,
    };
    append(&mut f, &inventory, exact).unwrap();
    // Repetition consumes another binding, not another unique reference, inventory or namespace.
    append(&mut f, &inventory, exact).unwrap();
    assert_eq!(f.store.disk_blob_pending_residency(), Some((1, 1, 1)));
    let second = upload(&mut f, 4, b"");
    assert!(matches!(
        append(&mut f, &second, exact),
        Err(StorageError::ResourceLimit)
    ));
    assert_eq!(f.store.frontier().unwrap().get(), 3);
    f.store
        .refresh_disk_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache())
        .unwrap();
    append(&mut f, &second, exact).unwrap();
    assert_eq!(f.store.disk_blob_pending_residency(), Some((1, 1, 0)));
}

#[test]
fn disk_blob_append_collision_and_committed_inventory_corruption_fail_closed() {
    for from_disk in [true, false] {
        let mut f = open_fixture(from_disk);
        let inventory = if from_disk {
            // Empty initial base; publish then rebase to exercise the disk lookup.
            let inventory = upload(&mut f, 0, b"abc");
            append(&mut f, &inventory, append_limits()).unwrap();
            f.store
                .refresh_disk_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache())
                .unwrap();
            inventory
        } else {
            let inventory = upload(&mut f, 0, b"abc");
            append(&mut f, &inventory, append_limits()).unwrap();
            inventory
        };
        let reference = inventory.references()[0];
        let forged = BlobReference::new(
            reference.scope(),
            reference.id(),
            reference.byte_len(),
            reference.chunk_count(),
            [0xFF; 32],
        )
        .unwrap();
        let collision = BlobInventory::new(reference.scope(), [forged]).unwrap();
        f.fs.arm(FaultPlan::default()).unwrap();
        assert!(matches!(
            append(&mut f, &collision, append_limits()),
            Err(StorageError::IntegrityFailure)
        ));
        assert_eq!(f.fs.operation_count(Operation::WriteAt), 0);
        let name = inventory_name(
            &f.store.vault,
            f.store.database,
            f.store.epoch,
            f.store.writer,
            inventory.digest(),
        )
        .unwrap();
        let file =
            f.fs.open_existing(&f.store.database_directory, &name)
                .unwrap();
        let mut byte = [0];
        read_exact_at(&mut f.fs, &file, 100, &mut byte).unwrap();
        byte[0] ^= 1;
        write_all_at(&mut f.fs, &file, 100, &byte).unwrap();
        f.fs.sync_all(&file).unwrap();
        f.fs.arm(FaultPlan::default()).unwrap();
        assert!(matches!(
            append(&mut f, &inventory, append_limits()),
            Err(StorageError::IntegrityFailure)
        ));
        assert_eq!(f.fs.operation_count(Operation::SetLen), 0);
        assert_eq!(f.fs.operation_count(Operation::WriteAt), 0);
    }
}

#[test]
fn disk_blob_append_faults_preserve_old_or_exact_new_frontier() {
    for case in 0..3 {
        append_fault_case(case);
    }
}

#[test]
fn disk_blob_append_first_commit_and_empty_inventory_keep_mode_explicit() {
    let database = DatabaseId::from_bytes([0xA1; 16]);
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let store = JournalStore::create(
        &mut fs,
        options(database, "blob-metadata"),
        create_vault(database, 101_000),
        CounterEntropy::new(102_000),
    )
    .unwrap();
    drop(store);
    fs.restart().unwrap();
    let (store, _) = cold::reopen(&mut fs, database, cold::recovery_limits(), |_| Ok(())).unwrap();
    assert!(store.disk_blob_metadata().is_none());
    let mut f = Fixture {
        fs,
        store,
        references: Vec::new(),
    };
    let inventory = upload(&mut f, 0, b"");
    assert_eq!(
        append(&mut f, &inventory, append_limits())
            .unwrap()
            .revision,
        CommitRevision::FIRST
    );
    assert_eq!(f.store.disk_blob_pending_residency(), Some((1, 1, 1)));
    let empty = BlobInventory::new(inventory.scope(), []).unwrap();
    append(&mut f, &empty, append_limits()).unwrap();
    f.store
        .refresh_disk_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache())
        .unwrap();
    assert_eq!(
        f.store
            .disk_blob_metadata()
            .unwrap()
            .counts()
            .reference_bindings,
        1
    );
    assert_eq!(f.store.disk_blob_pending_residency(), Some((0, 0, 0)));
    // The old writer API remains explicit, not silently rerouted with invented limits.
    let mut legacy = Fixture::new(true);
    let empty = BlobInventory::new(
        NamespaceRef::new(legacy.store.database, inventory.scope().namespace()),
        [],
    )
    .unwrap();
    legacy.fs.arm(FaultPlan::default()).unwrap();
    assert!(matches!(
        append(&mut legacy, &empty, append_limits()),
        Err(StorageError::InvalidState)
    ));
    assert_eq!(legacy.fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(legacy.fs.operation_count(Operation::WriteAt), 0);
}

fn append_fault_case(case: u8) {
    let prepared = || {
        let mut f = open_fixture(case != 1);
        let inventory = upload(&mut f, 2, b"abc");
        if case == 2 {
            append(&mut f, &inventory, append_limits()).unwrap();
        }
        f.fs.arm(FaultPlan::default()).unwrap();
        (f, inventory)
    };
    let (mut sample, inventory) = prepared();
    let old_revision = sample.store.frontier().unwrap().get();
    let old_blobs = if case == 1 {
        2
    } else if case == 2 {
        1
    } else {
        0
    };
    append(&mut sample, &inventory, append_limits()).unwrap();
    let mut attempts = 0;
    let mut optional_non_crashes = 0;
    for operation in [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SetLen,
        Operation::SyncAll,
        Operation::SyncDirectory,
        Operation::SyncData,
    ] {
        let count = sample.fs.operation_count(operation);
        eprintln!("disk blob append case={case} {operation:?}: {count} boundaries");
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut f, inventory) = prepared();
                f.fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                let result = append(&mut f, &inventory, append_limits());
                assert_eq!(f.fs.pending_faults(), 0);
                let acknowledged = result.is_ok();
                if acknowledged {
                    assert_eq!(operation, Operation::OpenExisting);
                    assert_eq!(action, FaultAction::CrashAfter);
                    optional_non_crashes += 1;
                }
                if f.store.poisoned {
                    assert!(matches!(
                        append(&mut f, &inventory, append_limits()),
                        Err(StorageError::NeedsRecovery)
                    ));
                    assert!(matches!(
                        f.store.disk_blob_reference(
                            &mut f.fs,
                            inventory.scope(),
                            inventory.references()[0].id(),
                            append_limits().lookup,
                            &mut Fixture::cache()
                        ),
                        Err(StorageError::NeedsRecovery)
                    ));
                }
                let database = f.store.database;
                drop(f.store);
                f.fs.restart().unwrap();
                let mut recovery = cold::recovery_limits();
                recovery.maximum_verified_blob_bytes_per_pass = 9;
                let (store, report) =
                    cold::reopen(&mut f.fs, database, recovery, |_| Ok(())).unwrap();
                let frontier = report.frontier.unwrap().get();
                assert!((old_revision..=old_revision + 1).contains(&frontier));
                if acknowledged {
                    assert_eq!(frontier, old_revision + 1);
                }
                assert_eq!(
                    store.disk_blob_metadata().unwrap().counts().blobs,
                    old_blobs + u64::from(case != 2 && frontier > old_revision)
                );
                assert_eq!(store.disk_blob_pending_residency(), Some((0, 0, 0)));
                let reference = inventory.references()[0];
                let actual = store
                    .disk_blob_reference(
                        &mut f.fs,
                        reference.scope(),
                        reference.id(),
                        append_limits().lookup,
                        &mut Fixture::cache(),
                    )
                    .unwrap();
                assert_eq!(
                    actual,
                    (case == 2 || frontier > old_revision).then_some((
                        reference,
                        CommitRevision::new(if case == 2 { 2 } else { old_revision + 1 }).unwrap()
                    ))
                );
                attempts += 1;
            }
        }
    }
    eprintln!(
        "disk blob append case={case}: {attempts} fault attempts, {optional_non_crashes} optional non-crashes"
    );
}

#[test]
fn disk_blob_append_disk_lookup_budget_and_scope_are_not_absence() {
    let mut f = open_fixture(false);
    let inventory = BlobInventory::new(f.references[0].scope(), [f.references[0]]).unwrap();
    let mut bounded = append_limits();
    bounded.lookup = IndexGetLimits::new(1, 1).unwrap();
    f.fs.arm(FaultPlan::default()).unwrap();
    assert!(matches!(
        append(&mut f, &inventory, bounded),
        Err(StorageError::ResourceLimit)
    ));
    assert_eq!(f.fs.operation_count(Operation::WriteAt), 0);
    let foreign = NamespaceRef::new(
        DatabaseId::from_bytes([0xFF; 16]),
        inventory.scope().namespace(),
    );
    let empty = BlobInventory::new(foreign, []).unwrap();
    f.fs.arm(FaultPlan::default()).unwrap();
    assert!(matches!(
        append(&mut f, &empty, append_limits()),
        Err(StorageError::InvalidState)
    ));
    assert!(matches!(
        f.store.disk_blob_reference(
            &mut f.fs,
            foreign,
            inventory.references()[0].id(),
            bounded.lookup,
            &mut Fixture::cache()
        ),
        Err(StorageError::InvalidState)
    ));
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(f.fs.operation_count(Operation::WriteAt), 0);
}

#[test]
fn disk_blob_append_refresh_failure_retains_overlay_until_exact_publication() {
    let prepared = || {
        let mut f = open_fixture(true);
        let inventory = upload(&mut f, 0, b"");
        append(&mut f, &inventory, append_limits()).unwrap();
        f.fs.arm(FaultPlan::default()).unwrap();
        (f, inventory)
    };
    let (mut sample, _) = prepared();
    sample
        .store
        .refresh_disk_blob_metadata(&mut sample.fs, limits(), &mut Fixture::cache())
        .unwrap();
    let mut attempts = 0;
    for operation in [
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SetLen,
        Operation::SyncAll,
        Operation::SyncDirectory,
    ] {
        let count = sample.fs.operation_count(operation);
        assert!(count > 0);
        // The existing rebuild sweep covers every publication boundary. Here select first,
        // middle and last to additionally check the live overlay's release contract.
        for occurrence in BTreeSet::from([1, count.div_ceil(2), count]) {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut f, inventory) = prepared();
                f.fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                assert!(
                    f.store
                        .refresh_disk_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache())
                        .is_err()
                );
                assert_eq!(f.fs.pending_faults(), 0);
                assert_eq!(f.store.disk_blob_pending_residency(), Some((1, 1, 1)));
                assert_eq!(f.store.disk_blob_metadata().unwrap().revision().get(), 1);
                let database = f.store.database;
                drop(f.store);
                f.fs.restart().unwrap();
                let (store, report) =
                    cold::reopen(&mut f.fs, database, cold::recovery_limits(), |_| Ok(())).unwrap();
                assert_eq!(report.frontier.unwrap().get(), 2);
                assert_eq!(store.disk_blob_pending_residency(), Some((0, 0, 0)));
                assert_eq!(
                    store.disk_blob_metadata().unwrap().counts(),
                    BlobMetadataCounts {
                        blobs: 1,
                        namespaces: 1,
                        inventories: 1,
                        reference_bindings: 1
                    }
                );
                let reference = inventory.references()[0];
                assert_eq!(
                    store
                        .disk_blob_reference(
                            &mut f.fs,
                            reference.scope(),
                            reference.id(),
                            append_limits().lookup,
                            &mut Fixture::cache()
                        )
                        .unwrap()
                        .unwrap()
                        .1
                        .get(),
                    2
                );
                attempts += 1;
            }
        }
    }
    eprintln!("disk blob refresh overlay: {attempts} selected fault attempts");
}
