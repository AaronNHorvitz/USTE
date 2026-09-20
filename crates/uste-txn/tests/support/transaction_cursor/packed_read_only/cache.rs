use super::*;
use uste_storage::packed_page_cache::PackedPageCache;

#[test]
fn packed_read_only_cached_paths_preserve_owner_binding_and_sticky_cursor_failure() {
    let (mut fs, live, stage) = fixture_reader();
    let reader = live.packed_index_reader().unwrap();
    let expected = reader.get(&mut fs, stage.tree(), b"a", reads()).unwrap();
    let mut cache = PackedPageCache::new(64 * 1024).unwrap();
    let got = reader
        .get_cached(&mut fs, stage.tree(), b"a", reads(), &mut cache)
        .unwrap();
    assert_eq!(got.report, expected.report);
    assert_eq!(
        got.value.unwrap().as_slice(),
        expected.value.unwrap().as_slice()
    );
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
        reader
            .get_cached(&mut fs, stage.tree(), b"a", reads(), &mut cache)
            .is_ok()
    );
    assert_eq!(fs.pending_faults(), 1);
    cache.clear();
    assert!(
        reader
            .get_cached(&mut fs, stage.tree(), b"a", reads(), &mut cache)
            .is_err()
    );
    assert_eq!(fs.pending_faults(), 0);
    let mut cursor = reader.cursor(stage.tree(), b"", None, cursors()).unwrap();
    assert_eq!(
        reader
            .next_cached(&mut fs, &mut cursor, &mut cache)
            .unwrap()
            .unwrap()
            .key(),
        b"a"
    );
    let (mut other_fs, other, other_stage) = fixture_reader();
    let foreign = other.packed_index_reader().unwrap();
    assert!(
        foreign
            .get_cached(&mut other_fs, other_stage.tree(), b"a", reads(), &mut cache)
            .is_err()
    );
    assert!(
        foreign
            .next_cached(&mut other_fs, &mut cursor, &mut cache)
            .is_err()
    );
    assert_eq!(other_fs.operation_count(Operation::ReadAt), 0);
    assert!(matches!(
        reader.next_cached(&mut fs, &mut cursor, &mut cache),
        Err(TransactionError::Storage(StorageError::NeedsRecovery))
    ));
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
}

#[test]
fn packed_read_only_cached_future_binding_refuses_before_cache_or_io() {
    let mut parts = parts(3);
    let staged = parts
        .recovery
        .packed_indexes_with_io(&mut parts.fs, &parts.transactions[2], certificate_limits())
        .unwrap()
        .stage(
            &mut parts.fs,
            [98; 32],
            1,
            None,
            &[IndexDelta::new(b"a".to_vec(), None, Some(b"future".to_vec())).unwrap()],
            batches(),
        )
        .unwrap();
    let mut cache = PackedPageCache::new(64 * 1024).unwrap();
    let mut cursor = {
        let maintenance = parts
            .recovery
            .packed_indexes_with_io(&mut parts.fs, &parts.transactions[2], certificate_limits())
            .unwrap();
        maintenance
            .as_reader()
            .cursor(staged.tree(), b"", None, cursors())
            .unwrap()
    };
    let maintenance = parts
        .recovery
        .packed_indexes_with_io(&mut parts.fs, &parts.transactions[0], certificate_limits())
        .unwrap();
    let reader = maintenance.as_reader();
    parts.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        reader
            .get_cached(&mut parts.fs, staged.tree(), b"a", reads(), &mut cache)
            .is_err()
    );
    assert!(
        reader
            .next_cached(&mut parts.fs, &mut cursor, &mut cache)
            .is_err()
    );
    assert_eq!(cache.report().unwrap().misses, 0);
    assert_eq!(parts.fs.operation_count(Operation::ReadAt), 0);
}
