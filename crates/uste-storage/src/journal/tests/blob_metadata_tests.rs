use super::*;
mod cold;

fn limits() -> BlobMetadataRebuildLimits {
    let run = IndexRunReadLimits::new(32, 32, 8192).unwrap();
    BlobMetadataRebuildLimits {
        admission: BlobMetadataAdmissionLimits {
            maximum_blobs: 32,
            maximum_namespaces: 32,
            maximum_inventories: 32,
            maximum_reference_bindings: 64,
            run,
            lookup: IndexGetLimits::new(32, 128).unwrap(),
            certificates: CertificateAnchorReadLimits::new(8, 8 * SMALL_ENVELOPE_BYTES).unwrap(),
            maximum_journal_groups: 8,
            maximum_journal_encoded_bytes: 1024 * 1024,
        },
        merge: IndexRunMergeLimits::new(run, 32, 8192, 32, 8192).unwrap(),
        maximum_merge_output_bytes: 8192,
    }
}

struct Fixture {
    fs: FaultFileSystem<MemoryFileSystem>,
    store: FaultStore,
    references: Vec<BlobReference>,
}

impl Fixture {
    fn new(empty: bool) -> Self {
        let database = DatabaseId::from_bytes([0x98; 16]);
        let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
        let mut store = JournalStore::create(
            &mut fs,
            options(database, "blob-metadata"),
            create_vault(database, 980_000),
            CounterEntropy::new(981_000),
        )
        .unwrap();
        let mut references = Vec::new();
        if !empty {
            for (namespace, payload) in [(0, b"abc".as_slice()), (1, b"".as_slice())] {
                let scope = NamespaceRef::new(
                    database,
                    uste_types::NamespaceId::from_bytes([namespace; 16]),
                );
                let mut upload = store.start_blob_upload(scope).unwrap();
                store
                    .write_blob_upload(&mut fs, &mut upload, payload)
                    .unwrap();
                let reference = store.finish_blob_upload(&mut fs, &mut upload).unwrap();
                references.push(reference);
                let inventory = BlobInventory::new(scope, [reference]).unwrap();
                for _ in 0..2 {
                    store
                        .append_group_with_inventory(
                            &mut fs,
                            CommitInput {
                                encoded_group: b"inventory",
                                logical_event_digest: [namespace; 32],
                            },
                            &inventory,
                        )
                        .unwrap();
                }
            }
        }
        store
            .append_group(
                &mut fs,
                CommitInput {
                    encoded_group: b"empty",
                    logical_event_digest: [3; 32],
                },
            )
            .unwrap();
        drop(store);
        fs.restart().unwrap();
        let (mut store, _) = JournalStore::open_with_disk_certificate_anchors(
            &mut fs,
            &entry("blob-metadata"),
            database,
            CounterEntropy::new(982_000),
            CounterEntropy::new(983_000),
            &mut TestKeyAdapter,
            limits().admission.certificates,
            |_| Ok(()),
        )
        .unwrap();
        // Isolate rebuild/admission from the legacy blob recovery maps; not a map-free open claim.
        store.committed_blobs.clear();
        store.committed_blob_inventories.clear();
        store.committed_blob_bytes.clear();
        Self {
            fs,
            store,
            references,
        }
    }

    fn cache() -> PageCache {
        PageCache::new(crate::MIN_INDEX_CACHE_BYTES).unwrap()
    }
}

#[test]
fn blob_metadata_rebuild_preserves_first_references_repeats_and_empty_namespaces() {
    let mut f = Fixture::new(false);
    let mut cache = Fixture::cache();
    let (base, report) = f
        .store
        .rebuild_blob_metadata(&mut f.fs, limits(), &mut cache)
        .unwrap();
    assert_eq!(base.revision().get(), 5);
    assert_eq!(
        base.counts(),
        BlobMetadataCounts {
            blobs: 2,
            namespaces: 2,
            inventories: 2,
            reference_bindings: 4
        }
    );
    assert_eq!(report.journal.groups, 5);
    assert_eq!(report.admission.journal.groups, 5);
    for (index, reference) in f.references.iter().enumerate() {
        assert_eq!(
            f.store
                .blob_metadata_reference(
                    &mut f.fs,
                    &base,
                    reference.scope(),
                    reference.id(),
                    limits().admission.lookup,
                    &mut cache
                )
                .unwrap(),
            Some((
                *reference,
                CommitRevision::new(index as u64 * 2 + 1).unwrap()
            ))
        );
        assert_eq!(
            f.store
                .blob_metadata_namespace_bytes(
                    &mut f.fs,
                    &base,
                    reference.scope(),
                    limits().admission.lookup,
                    &mut cache
                )
                .unwrap(),
            Some(reference.byte_len())
        );
    }
    assert!(f.store.certificate_anchors.is_empty());
    assert!(f.store.committed_blobs.is_empty());
    assert!(f.store.committed_blob_inventories.is_empty());
    assert!(f.store.committed_blob_bytes.is_empty());
    let scope = NamespaceRef::new(
        f.store.database,
        uste_types::NamespaceId::from_bytes([0; 16]),
    );
    let roots = f
        .store
        .load_index_root_manifests(&mut f.fs, scope, BLOB_METADATA_PROFILE_V1)
        .unwrap();
    assert_eq!(roots.len(), 1);
    let (again, work) = f
        .store
        .admit_blob_metadata(&mut f.fs, roots[0].clone(), limits().admission, &mut cache)
        .unwrap();
    assert_eq!(again.counts(), base.counts());
    assert_eq!(work.journal.groups, 5);
}

#[test]
fn blob_metadata_empty_catalog_and_prepublication_limits() {
    let mut f = Fixture::new(true);
    let (base, _) = f
        .store
        .rebuild_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache())
        .unwrap();
    assert_eq!(base.counts(), BlobMetadataCounts::default());
    for case in 0..4 {
        let mut f = Fixture::new(false);
        let mut limited = limits();
        match case {
            0 => limited.admission.maximum_journal_groups = 4,
            1 => limited.admission.maximum_blobs = 1,
            2 => limited.maximum_merge_output_bytes = 1,
            _ => limited.admission.maximum_journal_encoded_bytes = 1,
        }
        assert!(matches!(
            f.store
                .rebuild_blob_metadata(&mut f.fs, limited, &mut Fixture::cache()),
            Err(StorageError::ResourceLimit)
        ));
        let scope = NamespaceRef::new(
            f.store.database,
            uste_types::NamespaceId::from_bytes([0; 16]),
        );
        assert!(
            f.store
                .load_index_root_manifests(&mut f.fs, scope, BLOB_METADATA_PROFILE_V1)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn blob_metadata_exact_rebuild_budgets_and_cold_owner_binding() {
    let mut f = Fixture::new(false);
    let (_, work) = f
        .store
        .rebuild_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache())
        .unwrap();
    let mut exact = limits();
    exact.maximum_merge_output_bytes = work.merge_output_bytes;
    exact.admission.maximum_journal_groups = 5;
    exact.admission.maximum_journal_encoded_bytes = work
        .journal
        .encoded_bytes
        .max(work.admission.journal.encoded_bytes);
    exact.admission.maximum_blobs = 2;
    exact.admission.maximum_namespaces = 2;
    exact.admission.maximum_inventories = 2;
    exact.admission.maximum_reference_bindings = 4;
    let mut f = Fixture::new(false);
    let (base, _) = f
        .store
        .rebuild_blob_metadata(&mut f.fs, exact, &mut Fixture::cache())
        .unwrap();
    let database = f.store.database;
    drop(f.store);
    f.fs.restart().unwrap();
    let (store, _) = JournalStore::open_with_disk_certificate_anchors(
        &mut f.fs,
        &entry("blob-metadata"),
        database,
        CounterEntropy::new(984_000),
        CounterEntropy::new(985_000),
        &mut TestKeyAdapter,
        exact.admission.certificates,
        |_| Ok(()),
    )
    .unwrap();
    f.store = store;
    f.fs.arm(FaultPlan::default()).unwrap();
    let reference = f.references[0];
    assert!(
        f.store
            .blob_metadata_reference(
                &mut f.fs,
                &base,
                reference.scope(),
                reference.id(),
                exact.admission.lookup,
                &mut Fixture::cache()
            )
            .is_err()
    );
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    let scope = NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0; 16]));
    let roots = f
        .store
        .load_index_root_manifests(&mut f.fs, scope, BLOB_METADATA_PROFILE_V1)
        .unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(
        f.store
            .admit_blob_metadata(
                &mut f.fs,
                roots[0].clone(),
                exact.admission,
                &mut Fixture::cache()
            )
            .unwrap()
            .0
            .counts(),
        base.counts()
    );
    let mut f = Fixture::new(false);
    exact.maximum_merge_output_bytes -= 1;
    assert!(matches!(
        f.store
            .rebuild_blob_metadata(&mut f.fs, exact, &mut Fixture::cache()),
        Err(StorageError::ResourceLimit)
    ));
    assert!(
        f.store
            .load_index_root_manifests(&mut f.fs, scope, BLOB_METADATA_PROFILE_V1)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn blob_metadata_rejects_authenticated_but_false_catalogs() {
    for case in 0..5 {
        let mut f = Fixture::new(false);
        f.store
            .rebuild_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache())
            .unwrap();
        let scope = NamespaceRef::new(
            f.store.database,
            uste_types::NamespaceId::from_bytes([0; 16]),
        );
        let root = f
            .store
            .load_index_root_manifests(&mut f.fs, scope, BLOB_METADATA_PROFILE_V1)
            .unwrap()
            .remove(0);
        let mut runs = Vec::new();
        let mut counts = Vec::new();
        for family in 1..=4 {
            let mut cursor = f
                .store
                .open_index_run_cursor(&mut f.fs, &root, family, limits().admission.run)
                .unwrap();
            let mut entries = Vec::new();
            while let Some(entry) = f
                .store
                .next_index_run_entry(&mut f.fs, &mut cursor)
                .unwrap()
            {
                entries.push(entry);
            }
            f.store.finish_index_run_cursor(cursor).unwrap();
            match (case, family) {
                (0, 2) => entries[0].value[..8].copy_from_slice(&2_u64.to_be_bytes()),
                (1, 3) => entries[0].value.copy_from_slice(&4_u64.to_be_bytes()),
                (2, 4) => entries[0].value[..4].copy_from_slice(&2_u32.to_be_bytes()),
                (3, 1) => entries[0].value[32..40].copy_from_slice(&5_u64.to_be_bytes()),
                (4, 2) => entries[0].key[16] ^= 1,
                _ => {}
            }
            if family == 1 {
                counts = entries[0].value.clone();
            }
            runs.push(
                f.store
                    .publish_index_run(
                        &mut f.fs,
                        scope,
                        root.revision(),
                        BLOB_METADATA_PROFILE_V1,
                        family,
                        entries,
                    )
                    .unwrap(),
            );
        }
        // Recompute an authentic state binding: rejection must come from independent semantic
        // correspondence, not a deliberately stale checksum or unauthenticated ciphertext.
        let mut digest = Sha256::new();
        digest.update(b"USTE-STORAGE-BLOB-STATE-V1\0");
        digest.update(root.revision().get().to_be_bytes());
        digest.update(counts);
        for run in &runs {
            digest.update([run.family()]);
            digest.update(run.entry_count().to_be_bytes());
            digest.update(run.logical_digest());
        }
        let input = IndexRootInput {
            scope,
            revision: root.revision(),
            certificate_digest: *root.certificate_digest(),
            reducer_profile: BLOB_METADATA_PROFILE_V1,
            index_profile: BLOB_METADATA_PROFILE_V1,
            logical_state_digest: digest.finalize().into(),
        };
        let false_root = f
            .store
            .publish_index_root_recovered_bounded(&mut f.fs, input, &runs, limits().admission.run)
            .unwrap();
        assert!(
            matches!(
                f.store.admit_blob_metadata(
                    &mut f.fs,
                    false_root,
                    limits().admission,
                    &mut Fixture::cache()
                ),
                Err(StorageError::IntegrityFailure)
            ),
            "case {case}"
        );
    }
}

#[test]
fn blob_metadata_rebuild_faults_preserve_admissible_durable_fallback() {
    let mut sample = Fixture::new(false);
    sample
        .store
        .rebuild_blob_metadata(&mut sample.fs, limits(), &mut Fixture::cache())
        .unwrap();
    sample.fs.arm(FaultPlan::default()).unwrap();
    sample
        .store
        .rebuild_blob_metadata(&mut sample.fs, limits(), &mut Fixture::cache())
        .unwrap();
    let operations = [
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SyncData,
        Operation::SyncAll,
        Operation::SetLen,
        Operation::RenameNoReplace,
        Operation::SyncDirectory,
        Operation::RemoveFile,
    ];
    let mut attempts = 0;
    let mut failures = 0;
    for operation in operations {
        let count = sample.fs.operation_count(operation);
        eprintln!("blob catalog rebuild {operation:?}: {count} boundaries");
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let mut f = Fixture::new(false);
                let (original, _) = f
                    .store
                    .rebuild_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache())
                    .unwrap();
                f.fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                let result =
                    f.store
                        .rebuild_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache());
                attempts += 1;
                failures += usize::from(result.is_err());
                if result.is_ok() {
                    assert_eq!(operation, Operation::RemoveFile);
                    assert_eq!(action, FaultAction::CrashAfter);
                    assert_eq!(occurrence, 1);
                }
                assert_eq!(f.fs.pending_faults(), 0);
                let database = f.store.database;
                drop(f.store);
                f.fs.restart().unwrap();
                let (store, _) = JournalStore::open_with_disk_certificate_anchors(
                    &mut f.fs,
                    &entry("blob-metadata"),
                    database,
                    CounterEntropy::new(984_000),
                    CounterEntropy::new(985_000),
                    &mut TestKeyAdapter,
                    limits().admission.certificates,
                    |_| Ok(()),
                )
                .unwrap();
                f.store = store;
                let scope =
                    NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0; 16]));
                let roots = f
                    .store
                    .load_index_root_manifests(&mut f.fs, scope, BLOB_METADATA_PROFILE_V1)
                    .unwrap();
                assert!(!roots.is_empty(), "{operation:?}/{occurrence}/{action:?}");
                for root in roots {
                    let (base, _) = f
                        .store
                        .admit_blob_metadata(
                            &mut f.fs,
                            root,
                            limits().admission,
                            &mut Fixture::cache(),
                        )
                        .unwrap();
                    assert_eq!(base.counts(), original.counts());
                    assert_eq!(base.revision(), original.revision());
                }
            }
        }
    }
    eprintln!("blob catalog rebuild: {attempts} attempts, {failures} reported failures");
    assert!(attempts > 0);
}

#[test]
fn blob_metadata_admission_read_faults_and_late_inventory_corruption_fail_closed() {
    let rooted = || {
        let mut f = Fixture::new(false);
        f.store
            .rebuild_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache())
            .unwrap();
        let scope = NamespaceRef::new(
            f.store.database,
            uste_types::NamespaceId::from_bytes([0; 16]),
        );
        let root = f
            .store
            .load_index_root_manifests(&mut f.fs, scope, BLOB_METADATA_PROFILE_V1)
            .unwrap()
            .remove(0);
        (f, root)
    };
    let (mut sample, root) = rooted();
    sample.fs.arm(FaultPlan::default()).unwrap();
    sample
        .store
        .admit_blob_metadata(
            &mut sample.fs,
            root,
            limits().admission,
            &mut Fixture::cache(),
        )
        .unwrap();
    let mut attempts = 0;
    for operation in [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
    ] {
        let count = sample.fs.operation_count(operation);
        assert!(count > 0);
        let selected = BTreeSet::from([1, count.div_ceil(2), count]);
        for occurrence in selected {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut f, root) = rooted();
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
                        .admit_blob_metadata(
                            &mut f.fs,
                            root,
                            limits().admission,
                            &mut Fixture::cache()
                        )
                        .is_err()
                );
                assert_eq!(f.fs.pending_faults(), 0);
                attempts += 1;
            }
        }
    }
    assert_eq!(attempts, 27);
    for inventory in [false, true] {
        let (mut f, root) = rooted();
        let (file, offset) = if inventory {
            let reference = f.references[1];
            let digest = BlobInventory::new(reference.scope(), [reference])
                .unwrap()
                .digest();
            let name = inventory_name(
                &f.store.vault,
                f.store.database,
                f.store.epoch,
                f.store.writer,
                digest,
            )
            .unwrap();
            (
                f.fs.open_existing(&f.store.database_directory, &name)
                    .unwrap(),
                100,
            )
        } else {
            (f.store.certificate_file, 5 * SMALL_ENVELOPE_BYTES + 100)
        };
        let mut byte = [0];
        read_exact_at(&mut f.fs, &file, offset, &mut byte).unwrap();
        byte[0] ^= 1;
        write_all_at(&mut f.fs, &file, offset, &byte).unwrap();
        assert!(
            f.store
                .admit_blob_metadata(&mut f.fs, root, limits().admission, &mut Fixture::cache())
                .is_err()
        );
    }
}
