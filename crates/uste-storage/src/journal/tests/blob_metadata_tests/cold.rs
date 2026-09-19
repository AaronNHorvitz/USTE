use super::*;

pub(super) fn recovery_limits() -> BlobRecoveryLimits {
    BlobRecoveryLimits {
        catalog: limits(),
        catalog_recovery: BlobCatalogRecovery::AdmitOrRebuild,
        maximum_verified_blob_bytes_per_pass: 6,
        maximum_uncommitted_segment_tails: 8,
    }
}

pub(super) fn reopen<V>(
    fs: &mut FaultFileSystem<MemoryFileSystem>,
    database: DatabaseId,
    limits: BlobRecoveryLimits,
    visitor: V,
) -> Result<(FaultStore, RecoveryReport), StorageError>
where
    V: FnMut(RecoveredGroup<'_>) -> Result<(), StorageError>,
{
    // Each real reopen supplies fresh entropy. Reusing a scripted counter range would
    // intentionally collide with immutable scratch object identities from the previous open.
    static NEXT_SEED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(10_000_000);
    let seed = NEXT_SEED.fetch_add(100_000, std::sync::atomic::Ordering::Relaxed);
    JournalStore::open_with_disk_blob_metadata(
        fs,
        &entry("blob-metadata"),
        database,
        CounterEntropy::new(seed),
        CounterEntropy::new(seed + 50_000),
        &mut TestKeyAdapter,
        limits,
        &mut Fixture::cache(),
        visitor,
    )
}

#[test]
fn disk_blob_cold_recovery_rebuilds_then_admits_without_resident_metadata() {
    let mut f = Fixture::new(false);
    let database = f.store.database;
    drop(f.store);
    for rebuilt in [true, false] {
        f.fs.restart().unwrap();
        let mut revisions = Vec::new();
        let (mut store, report) = reopen(&mut f.fs, database, recovery_limits(), |group| {
            revisions.push(group.revision.get());
            Ok(())
        })
        .unwrap();
        assert_eq!(revisions, [1, 2, 3, 4, 5]);
        assert_eq!(report.frontier.unwrap().get(), 5);
        assert_eq!(store.certificate_anchor_residency(), (false, 0));
        assert_eq!(store.blob_metadata_residency(), (false, 0, 0, 0));
        let work = store.blob_recovery_report().unwrap();
        assert_eq!(work.used_existing_catalog, !rebuilt);
        assert_eq!(work.rebuild.is_some(), rebuilt);
        for pass in [&work.validation, &work.replay] {
            assert_eq!(pass.groups, 5);
            assert_eq!(pass.reference_bindings, 4);
            assert_eq!(pass.verified_blob_bytes, 6);
            assert_eq!(pass.maximum_live_inventory_references, 1);
        }
        let base = store.disk_blob_metadata().unwrap();
        assert_eq!(base.counts().blobs, 2);
        for (index, reference) in f.references.iter().enumerate() {
            assert_eq!(
                store
                    .blob_metadata_reference(
                        &mut f.fs,
                        base,
                        reference.scope(),
                        reference.id(),
                        limits().admission.lookup,
                        &mut Fixture::cache()
                    )
                    .unwrap(),
                Some((
                    *reference,
                    CommitRevision::new(index as u64 * 2 + 1).unwrap()
                ))
            );
            let revision = CommitRevision::new(index as u64 * 2 + 1).unwrap();
            let certificate = store
                .authenticate_certificate_revision(
                    &mut f.fs,
                    revision,
                    limits().admission.certificates,
                )
                .unwrap();
            let proof = store
                .authenticate_blob_reference(
                    &mut f.fs,
                    &certificate,
                    *reference,
                    BlobReferenceProofLimits::new(1, 2 * SMALL_ENVELOPE_BYTES).unwrap(),
                )
                .unwrap();
            let mut output = [0; 4];
            let count = store
                .read_proven_blob_range(&mut f.fs, &proof, 0, &mut output)
                .unwrap();
            assert_eq!(
                &output[..count],
                if index == 0 {
                    b"abc".as_slice()
                } else {
                    b"".as_slice()
                }
            );
        }
        let inventory = BlobInventory::new(f.references[0].scope(), [f.references[0]]).unwrap();
        f.fs.arm(FaultPlan::default()).unwrap();
        assert!(matches!(
            store.append_group_with_inventory(
                &mut f.fs,
                CommitInput {
                    encoded_group: b"must prove absence",
                    logical_event_digest: [9; 32],
                },
                &inventory
            ),
            Err(StorageError::InvalidState)
        ));
        assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
        assert_eq!(f.fs.operation_count(Operation::WriteAt), 0);
        drop(store);
    }
}

#[test]
fn disk_blob_cold_recovery_limits_and_payload_corruption_precede_callbacks_and_repairs() {
    for case in 0..5 {
        let mut f = Fixture::new(false);
        let database = f.store.database;
        let mut admitted = recovery_limits();
        match case {
            0 => admitted.maximum_verified_blob_bytes_per_pass = 5,
            1 => admitted.catalog.admission.maximum_reference_bindings = 3,
            2 => admitted.catalog.admission.maximum_journal_groups = 4,
            3 => admitted.catalog.admission.maximum_journal_encoded_bytes = 1,
            _ => {
                let name = crate::blob::final_chunk_name(f.references[0].id(), 0).unwrap();
                let file =
                    f.fs.open_existing(&f.store.database_directory, &name)
                        .unwrap();
                let mut byte = [0];
                read_exact_at(&mut f.fs, &file, 100, &mut byte).unwrap();
                byte[0] ^= 1;
                write_all_at(&mut f.fs, &file, 100, &byte).unwrap();
                f.fs.sync_all(&file).unwrap();
            }
        }
        drop(f.store);
        f.fs.restart().unwrap();
        f.fs.arm(FaultPlan::default()).unwrap();
        let mut callbacks = 0;
        let result = reopen(&mut f.fs, database, admitted, |_| {
            callbacks += 1;
            Ok(())
        });
        assert!(result.is_err(), "case {case}");
        assert_eq!(callbacks, 0);
        assert_eq!(f.fs.operation_count(Operation::WriteAt), 0);
        assert_eq!(f.fs.operation_count(Operation::SetLen), 0);
        assert_eq!(f.fs.operation_count(Operation::SyncData), 0);
    }
}

#[test]
fn disk_blob_cold_tail_admission_and_resynchronization_precede_replay() {
    let mut f = Fixture::new(false);
    f.store
        .rebuild_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache())
        .unwrap();
    let database = f.store.database;
    let certificate_end = f.fs.metadata(&f.store.certificate_file).unwrap().len;
    write_all_at(
        &mut f.fs,
        &f.store.certificate_file,
        certificate_end,
        b"123",
    )
    .unwrap();
    f.fs.sync_data(&f.store.certificate_file).unwrap();
    write_all_at(
        &mut f.fs,
        &f.store.current_segment_file,
        f.store.current_segment_offset,
        b"tail",
    )
    .unwrap();
    f.fs.sync_data(&f.store.current_segment_file).unwrap();
    drop(f.store);
    f.fs.restart().unwrap();
    let mut bounded = recovery_limits();
    bounded.maximum_uncommitted_segment_tails = 0;
    f.fs.arm(FaultPlan::default()).unwrap();
    let mut callbacks = 0;
    assert!(matches!(
        reopen(&mut f.fs, database, bounded, |_| {
            callbacks += 1;
            Ok(())
        }),
        Err(StorageError::ResourceLimit)
    ));
    assert_eq!(callbacks, 0);
    assert_eq!(f.fs.operation_count(Operation::SetLen), 0);
    assert_eq!(f.fs.operation_count(Operation::SyncData), 0);
    bounded.maximum_uncommitted_segment_tails = 1;
    f.fs.arm(
        FaultPlan::new([FaultPoint {
            operation: Operation::SyncData,
            occurrence: 1,
            action: FaultAction::Error(AdapterErrorKind::Io),
        }])
        .unwrap(),
    )
    .unwrap();
    assert!(
        reopen(&mut f.fs, database, bounded, |_| {
            callbacks += 1;
            Ok(())
        })
        .is_err()
    );
    assert_eq!(callbacks, 0);
    f.fs.restart().unwrap();
    let (store, report) = reopen(&mut f.fs, database, bounded, |_| {
        callbacks += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(callbacks, 5);
    assert_eq!(report.repaired_certificate_tail_bytes, 3);
    assert_eq!(report.ignored_uncommitted_journal_bytes, 4);
    let work = store.blob_recovery_report().unwrap();
    assert_eq!(work.validation.peak_pending_segment_tails, 1);
    assert_eq!(work.replay.peak_pending_segment_tails, 0);
    assert!(work.used_existing_catalog);
}

#[test]
fn disk_blob_cold_empty_and_stale_catalogs_keep_the_exact_frontier() {
    let database = DatabaseId::from_bytes([0x99; 16]);
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let store = JournalStore::create(
        &mut fs,
        options(database, "blob-metadata"),
        create_vault(database, 990_000),
        CounterEntropy::new(991_000),
    )
    .unwrap();
    drop(store);
    fs.restart().unwrap();
    let (mut store, report) = reopen(&mut fs, database, recovery_limits(), |_| {
        panic!("empty journal")
    })
    .unwrap();
    assert_eq!(report.frontier, None);
    assert!(store.disk_blob_metadata().is_none());
    assert_eq!(store.blob_metadata_residency(), (false, 0, 0, 0));
    store
        .append_group(
            &mut fs,
            CommitInput {
                encoded_group: b"first",
                logical_event_digest: [1; 32],
            },
        )
        .unwrap();
    drop(store);
    fs.restart().unwrap();
    let (mut store, _) = reopen(&mut fs, database, recovery_limits(), |_| Ok(())).unwrap();
    assert_eq!(
        store.disk_blob_metadata().unwrap().revision(),
        CommitRevision::FIRST
    );
    store
        .append_group(
            &mut fs,
            CommitInput {
                encoded_group: b"second",
                logical_event_digest: [2; 32],
            },
        )
        .unwrap();
    assert_eq!(
        store.disk_blob_metadata().unwrap().revision(),
        CommitRevision::FIRST
    );
    drop(store);
    fs.restart().unwrap();
    let (store, report) = reopen(&mut fs, database, recovery_limits(), |_| Ok(())).unwrap();
    assert_eq!(report.frontier.unwrap().get(), 2);
    assert_eq!(store.disk_blob_metadata().unwrap().revision().get(), 2);
    assert!(store.blob_recovery_report().unwrap().rebuild.is_some());
    assert_eq!(store.blob_metadata_residency(), (false, 0, 0, 0));
}

#[test]
fn disk_blob_cold_false_current_catalog_requires_explicit_rebuild() {
    let mut f = Fixture::new(false);
    let database = f.store.database;
    let (revision, certificate_digest) = f.store.checkpoint_anchor().unwrap();
    let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0; 16]));
    let mut value = vec![0; 48];
    value[..4].copy_from_slice(b"SBMD");
    value[4] = 1;
    let run = f
        .store
        .publish_index_run(
            &mut f.fs,
            scope,
            revision,
            BLOB_METADATA_PROFILE_V1,
            1,
            [IndexEntry {
                key: b"storage-blob-meta-v1".to_vec(),
                value,
            }],
        )
        .unwrap();
    f.store
        .publish_index_root_recovered(
            &mut f.fs,
            IndexRootInput {
                scope,
                revision,
                certificate_digest,
                reducer_profile: BLOB_METADATA_PROFILE_V1,
                logical_state_digest: [0; 32],
                index_profile: BLOB_METADATA_PROFILE_V1,
            },
            &[run],
        )
        .unwrap();
    drop(f.store);
    f.fs.restart().unwrap();
    f.fs.arm(FaultPlan::default()).unwrap();
    let mut callbacks = 0;
    assert!(matches!(
        reopen(&mut f.fs, database, recovery_limits(), |_| {
            callbacks += 1;
            Ok(())
        }),
        Err(StorageError::IntegrityFailure)
    ));
    assert_eq!(callbacks, 0);
    assert_eq!(f.fs.operation_count(Operation::SyncData), 0);
    let mut explicit = recovery_limits();
    explicit.catalog_recovery = BlobCatalogRecovery::Rebuild;
    let (store, _) = reopen(&mut f.fs, database, explicit, |_| {
        callbacks += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(callbacks, 5);
    assert_eq!(store.disk_blob_metadata().unwrap().counts().blobs, 2);
    assert!(store.blob_recovery_report().unwrap().rebuild.is_some());
}

#[test]
fn disk_blob_cold_recovery_faults_keep_journal_authority_and_restart_exactly() {
    let prepared = |ready| {
        let mut f = Fixture::new(false);
        if ready {
            f.store
                .rebuild_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache())
                .unwrap();
        }
        let database = f.store.database;
        drop(f.store);
        f.fs.restart().unwrap();
        (f.fs, database)
    };
    let mut attempts = 0;
    let mut non_crashes = 0;
    for ready in [true, false] {
        let (mut sample, database) = prepared(ready);
        sample.arm(FaultPlan::default()).unwrap();
        drop(reopen(&mut sample, database, recovery_limits(), |_| Ok(())).unwrap());
        let operations = if ready {
            vec![
                Operation::OpenDirectory,
                Operation::TryLockExclusive,
                Operation::OpenExisting,
                Operation::Metadata,
                Operation::ReadAt,
                Operation::SyncData,
            ]
        } else {
            vec![
                Operation::CreateNew,
                Operation::WriteAt,
                Operation::SetLen,
                Operation::SyncAll,
                Operation::SyncDirectory,
                Operation::SyncData,
            ]
        };
        for operation in operations {
            let count = sample.operation_count(operation);
            assert!(count > 0);
            // Ready-open reads/resynchronization are exhaustive. Rebuild writes are selected
            // first/middle/last here; the separate catalog sweep covers every mutation boundary.
            let selected = if ready {
                (1..=count).collect::<BTreeSet<_>>()
            } else {
                BTreeSet::from([1, count.div_ceil(2), count])
            };
            eprintln!(
                "disk blob cold ready={ready} {operation:?}: {count} boundaries, {} selected",
                selected.len()
            );
            for occurrence in selected {
                for action in [
                    FaultAction::Error(AdapterErrorKind::Io),
                    FaultAction::CrashBefore,
                    FaultAction::CrashAfter,
                ] {
                    let (mut fs, database) = prepared(ready);
                    fs.arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                    let result = reopen(&mut fs, database, recovery_limits(), |_| Ok(()));
                    if let Ok((store, report)) = &result {
                        assert_eq!(operation, Operation::OpenExisting);
                        assert_eq!(action, FaultAction::CrashAfter);
                        assert_eq!(report.frontier.unwrap().get(), 5);
                        assert_eq!(store.disk_blob_metadata().unwrap().counts().blobs, 2);
                        non_crashes += 1;
                    }
                    drop(result);
                    assert_eq!(fs.pending_faults(), 0);
                    fs.restart().unwrap();
                    let mut callbacks = 0;
                    let (store, report) = reopen(&mut fs, database, recovery_limits(), |_| {
                        callbacks += 1;
                        Ok(())
                    })
                    .unwrap();
                    assert_eq!(callbacks, 5);
                    assert_eq!(report.frontier.unwrap().get(), 5);
                    assert_eq!(
                        store
                            .disk_blob_metadata()
                            .unwrap()
                            .counts()
                            .reference_bindings,
                        4
                    );
                    assert_eq!(store.blob_metadata_residency(), (false, 0, 0, 0));
                    attempts += 1;
                }
            }
        }
    }
    eprintln!("disk blob cold: {attempts} attempts, {non_crashes} optional non-crashes");
}
