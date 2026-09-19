use super::*;

fn empty_prefix() -> Fixture {
    let mut f = Fixture::new(true);
    for number in 2_u8..=5 {
        f.store
            .append_group(
                &mut f.fs,
                CommitInput {
                    encoded_group: b"empty prefix",
                    logical_event_digest: [number; 32],
                },
            )
            .unwrap();
    }
    assert_eq!(f.store.committed_blob_reference_bindings, 0);
    f
}

#[test]
fn empty_blob_rebuild_stages_only_the_frontier_but_admits_every_group() {
    let mut f = empty_prefix();
    let mut exact = limits();
    exact.admission.maximum_journal_encoded_bytes = 11 * SMALL_ENVELOPE_BYTES;
    exact.maximum_merge_output_bytes = 68;
    let (base, work) = f
        .store
        .rebuild_blob_metadata(&mut f.fs, exact, &mut Fixture::cache())
        .unwrap();
    assert_eq!(base.revision().get(), 5);
    assert_eq!(base.counts(), BlobMetadataCounts::default());
    assert_eq!(work.journal.groups, 1);
    assert_eq!(work.journal.encoded_bytes, 3 * SMALL_ENVELOPE_BYTES);
    assert_eq!(work.certificate_bytes, SMALL_ENVELOPE_BYTES);
    assert_eq!(work.merge_output_bytes, 68);
    assert_eq!(work.admission.journal.groups, 5);
    assert_eq!(
        work.admission.journal.encoded_bytes,
        11 * SMALL_ENVELOPE_BYTES
    );
    assert_eq!(work.admission.run_entries, 1);

    // A small staging pass must not turn the full-prefix admission allowance into a suffix cap.
    for case in 0..3 {
        let mut f = empty_prefix();
        let mut short = exact;
        match case {
            0 => short.admission.maximum_journal_encoded_bytes -= 1,
            1 => short.admission.maximum_journal_groups = 4,
            _ => short.maximum_merge_output_bytes -= 1,
        }
        assert!(matches!(
            f.store
                .rebuild_blob_metadata(&mut f.fs, short, &mut Fixture::cache()),
            Err(StorageError::ResourceLimit)
        ));
        assert_no_catalog(&mut f);
    }
}

fn assert_no_catalog(f: &mut Fixture) {
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

#[test]
fn empty_blob_rebuild_does_not_trust_a_false_zero_binding_hint() {
    let mut f = Fixture::new(false);
    assert_eq!(f.store.committed_blob_reference_bindings, 4);
    // Simulate an internal hint defect: admission must still discover the earlier inventories.
    f.store.committed_blob_reference_bindings = 0;
    assert!(matches!(
        f.store
            .rebuild_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache()),
        Err(StorageError::IntegrityFailure)
    ));
    assert_no_catalog(&mut f);
}

#[test]
fn empty_blob_rebuild_authenticates_skipped_prefix_certificates() {
    for revision in 1..=5 {
        let mut f = empty_prefix();
        let offset = revision * SMALL_ENVELOPE_BYTES + 100;
        let mut byte = [0];
        read_exact_at(&mut f.fs, &f.store.certificate_file, offset, &mut byte).unwrap();
        byte[0] ^= 1;
        write_all_at(&mut f.fs, &f.store.certificate_file, offset, &byte).unwrap();
        assert!(matches!(
            f.store
                .rebuild_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache()),
            Err(StorageError::IntegrityFailure)
        ));
        assert_no_catalog(&mut f);
    }
}

#[test]
fn empty_blob_rebuild_authenticates_skipped_prefix_group_bytes() {
    for revision in 1..=5 {
        let mut f = empty_prefix();
        // Reopen retained this segment: its header precedes five single-envelope groups.
        assert_eq!(f.store.current_segment_offset, 6 * SMALL_ENVELOPE_BYTES);
        let offset = revision * SMALL_ENVELOPE_BYTES + 100;
        let mut byte = [0];
        read_exact_at(&mut f.fs, &f.store.current_segment_file, offset, &mut byte).unwrap();
        byte[0] ^= 1;
        write_all_at(&mut f.fs, &f.store.current_segment_file, offset, &byte).unwrap();
        assert!(matches!(
            f.store
                .rebuild_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache()),
            Err(StorageError::IntegrityFailure)
        ));
        assert_no_catalog(&mut f);
    }
}

#[test]
fn empty_blob_rebuild_selected_io_faults_restart_without_false_catalogs() {
    let mut sample = empty_prefix();
    sample.fs.arm(FaultPlan::default()).unwrap();
    sample
        .store
        .rebuild_blob_metadata(&mut sample.fs, limits(), &mut Fixture::cache())
        .unwrap();
    let mut attempts = 0;
    for operation in [
        Operation::ReadAt,
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SetLen,
        Operation::SyncAll,
        Operation::SyncDirectory,
    ] {
        let count = sample.fs.operation_count(operation);
        assert!(count > 0);
        for occurrence in BTreeSet::from([1, count.div_ceil(2), count]) {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let mut f = empty_prefix();
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
                        .rebuild_blob_metadata(&mut f.fs, limits(), &mut Fixture::cache())
                        .is_err(),
                    "{operation:?}/{occurrence}/{action:?}"
                );
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
                assert_eq!(f.store.frontier.unwrap().get(), 5);
                let scope =
                    NamespaceRef::new(database, uste_types::NamespaceId::from_bytes([0; 16]));
                let roots = f
                    .store
                    .load_index_root_manifests(&mut f.fs, scope, BLOB_METADATA_PROFILE_V1)
                    .unwrap();
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
                    assert_eq!(base.revision().get(), 5);
                    assert_eq!(base.counts(), BlobMetadataCounts::default());
                }
                attempts += 1;
            }
        }
    }
    eprintln!("empty catalog selected faults: {attempts} attempts");
    assert!(attempts > 0);
}
