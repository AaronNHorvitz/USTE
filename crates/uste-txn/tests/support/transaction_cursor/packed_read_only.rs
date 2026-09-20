use super::*;
#[path = "packed_read_only/cache.rs"]
mod cache;
use uste_storage::journal::CertifiedPackedTreeStage;

fn fixture_reader() -> (Fs, Live, CertifiedPackedTreeStage) {
    let mut parts = parts(3);
    let stage = parts
        .recovery
        .packed_indexes_with_io(&mut parts.fs, &parts.transactions[2], certificate_limits())
        .unwrap()
        .stage(
            &mut parts.fs,
            [97; 32],
            1,
            None,
            &[
                IndexDelta::new(b"a".to_vec(), None, Some(b"first".to_vec())).unwrap(),
                IndexDelta::new(b"b".to_vec(), None, Some(b"second".to_vec())).unwrap(),
            ],
            batches(),
        )
        .unwrap();
    let (mut fs, _, live, _) = install(parts, 2, 2);
    fs.arm(FaultPlan::default()).unwrap();
    (fs, live, stage)
}

#[test]
fn packed_read_only_lookup_traversal_limits_and_foreign_owner() {
    let (mut fs, live, stage) = fixture_reader();
    let reader = live.packed_index_reader().unwrap();
    assert_eq!(reader.anchor(), live.base_anchor());
    reader.validate_tree_binding(stage.tree()).unwrap();
    let got = reader.get(&mut fs, stage.tree(), b"a", reads()).unwrap();
    assert_eq!(got.value.as_ref().unwrap().as_slice(), b"first");
    let mut exact = reads();
    exact.maximum_pages = got.report.pages;
    exact.maximum_encoded_bytes = got.report.encoded_bytes;
    exact.maximum_value_bytes = 5;
    assert_eq!(
        reader
            .get(&mut fs, stage.tree(), b"a", exact)
            .unwrap()
            .report,
        got.report
    );
    for narrow in 0..3 {
        let mut limit = exact;
        match narrow {
            0 => limit.maximum_pages -= 1,
            1 => limit.maximum_encoded_bytes -= 1,
            _ => limit.maximum_value_bytes -= 1,
        }
        assert!(reader.get(&mut fs, stage.tree(), b"a", limit).is_err());
    }
    assert!(
        reader
            .get(&mut fs, stage.tree(), b"absent", reads())
            .unwrap()
            .value
            .is_none()
    );
    let mut forward = reader.cursor(stage.tree(), b"", None, cursors()).unwrap();
    for expected in [b"a", b"b"] {
        assert_eq!(
            reader.next(&mut fs, &mut forward).unwrap().unwrap().key(),
            expected
        );
    }
    assert!(reader.next(&mut fs, &mut forward).unwrap().is_none());
    let mut reverse = reader
        .reverse_cursor(stage.tree(), b"", None, cursors())
        .unwrap();
    for expected in [b"b", b"a"] {
        assert_eq!(
            reader.next(&mut fs, &mut reverse).unwrap().unwrap().key(),
            expected
        );
    }
    assert!(reader.next(&mut fs, &mut reverse).unwrap().is_none());
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);

    let (mut foreign_fs, foreign, _) = fixture_reader();
    let other = foreign.packed_index_reader().unwrap();
    assert!(other.validate_tree_binding(stage.tree()).is_err());
    assert!(
        other
            .get(&mut foreign_fs, stage.tree(), b"a", reads())
            .is_err()
    );
    assert!(other.cursor(stage.tree(), b"", None, cursors()).is_err());
    assert!(
        other
            .reverse_cursor(stage.tree(), b"", None, cursors())
            .is_err()
    );
    // End-of-stream does not bypass live owner checking, and failure remains sticky.
    assert!(other.next(&mut foreign_fs, &mut forward).is_err());
    assert!(other.next(&mut foreign_fs, &mut reverse).is_err());
    assert!(matches!(
        reader.next(&mut fs, &mut forward),
        Err(TransactionError::Storage(StorageError::NeedsRecovery))
    ));
    assert_eq!(foreign_fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(foreign_fs.operation_count(Operation::WriteAt), 0);
}

#[test]
fn packed_read_only_every_observed_read_fault_fails_and_cursor_stays_failed() {
    let operations = [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
    ];
    let (mut fs, live, stage) = fixture_reader();
    let reader = live.packed_index_reader().unwrap();
    let mut cursor = reader.cursor(stage.tree(), b"", None, cursors()).unwrap();
    while reader.next(&mut fs, &mut cursor).unwrap().is_some() {}
    let counts = operations.map(|operation| fs.operation_count(operation));
    let mut cases = 0;
    for (operation, count) in operations.into_iter().zip(counts) {
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut fs, live, stage) = fixture_reader();
                let reader = live.packed_index_reader().unwrap();
                let mut cursor = reader.cursor(stage.tree(), b"", None, cursors()).unwrap();
                fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                loop {
                    match reader.next(&mut fs, &mut cursor) {
                        Ok(Some(_)) => {}
                        Ok(None) => panic!("unconsumed {operation:?}/{occurrence}/{action:?}"),
                        Err(_) => break,
                    }
                }
                assert_eq!(fs.pending_faults(), 0);
                assert!(matches!(
                    reader.next(&mut fs, &mut cursor),
                    Err(TransactionError::Storage(StorageError::NeedsRecovery))
                ));
                assert_eq!(fs.operation_count(Operation::CreateNew), 0);
                assert_eq!(fs.operation_count(Operation::WriteAt), 0);
                cases += 1;
            }
        }
    }
    assert!(cases > 0);
    eprintln!("packed read-only cursor fault cases: {cases}");
}
