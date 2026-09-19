use super::*;

#[test]
fn certificate_window_cursor_exact_budgets_rollover_and_owned_inventory() {
    for disk in [false, true] {
        let (mut fs, name, outcomes, inventory) = fixture();
        let recovery = if disk {
            open_disk(&mut fs, &name).0
        } else {
            open(&mut fs, &name, scope())
        };
        let last = outcomes[2].revision;
        fs.arm(FaultPlan::default()).unwrap();
        for invalid in [0, 65, u64::MAX] {
            assert!(
                recovery
                    .open_transaction_cursor_with_certificate_window(
                        CommitRevision::FIRST,
                        last,
                        3,
                        100_000,
                        invalid
                    )
                    .is_err()
            );
        }
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        for (size, proof_reads) in [(1, 6), (2, 5), (3, 4), (64, 4)] {
            let bytes = RANGE_BYTES + if disk { proof_reads * 4161 } else { 0 };
            let mut cursor = recovery
                .open_transaction_cursor_with_certificate_window(
                    CommitRevision::FIRST,
                    last,
                    3,
                    bytes,
                    size,
                )
                .unwrap();
            for (index, outcome) in outcomes.iter().enumerate() {
                let transaction = recovery
                    .next_recovered_transaction(&mut fs, &mut cursor)
                    .unwrap()
                    .unwrap();
                assert_eq!(transaction.outcome(), *outcome);
                assert_eq!(
                    transaction.blob_inventory(),
                    (index == 2).then_some(&inventory)
                );
            }
            assert!(
                recovery
                    .next_recovered_transaction(&mut fs, &mut cursor)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                recovery.finish_transaction_cursor(cursor).unwrap(),
                JournalRangeReadReport {
                    groups: 3,
                    encoded_bytes: bytes
                }
            );
            let mut short = recovery
                .open_transaction_cursor_with_certificate_window(
                    CommitRevision::FIRST,
                    last,
                    3,
                    bytes - 1,
                    size,
                )
                .unwrap();
            loop {
                match recovery.next_recovered_transaction(&mut fs, &mut short) {
                    Ok(Some(_)) => {}
                    Ok(None) => panic!("exact minus one must refuse"),
                    Err(_) => break,
                }
            }
            fs.arm(FaultPlan::default()).unwrap();
            assert!(
                recovery
                    .next_recovered_transaction(&mut fs, &mut short)
                    .is_err()
            );
            assert_eq!(fs.operation_count(Operation::ReadAt), 0);
            assert!(recovery.finish_transaction_cursor(short).is_err());
        }
    }
}

#[test]
fn certificate_window_cursor_rechecks_selected_bytes_and_owner_after_lookahead() {
    let (mut fs, name, outcomes, _) = fixture();
    let (recovery, _) = open_disk(&mut fs, &name);
    let mut cursor = recovery
        .open_transaction_cursor_with_certificate_window(
            CommitRevision::FIRST,
            outcomes[2].revision,
            3,
            100_000,
            64,
        )
        .unwrap();
    recovery
        .next_recovered_transaction(&mut fs, &mut cursor)
        .unwrap()
        .unwrap();
    let (mut foreign_fs, foreign_name, _, _) = fixture();
    let (foreign, _) = open_disk(&mut foreign_fs, &foreign_name);
    foreign_fs.arm(FaultPlan::default()).unwrap();
    assert!(
        foreign
            .next_recovered_transaction(&mut foreign_fs, &mut cursor)
            .is_err()
    );
    assert_eq!(foreign_fs.operation_count(Operation::ReadAt), 0);
    assert!(recovery.finish_transaction_cursor(cursor).is_err());

    let mut cursor = recovery
        .open_transaction_cursor_with_certificate_window(
            CommitRevision::FIRST,
            outcomes[2].revision,
            3,
            100_000,
            64,
        )
        .unwrap();
    recovery
        .next_recovered_transaction(&mut fs, &mut cursor)
        .unwrap()
        .unwrap();
    let directory = fs.open_directory(&fs.root(), &name).unwrap();
    let certificates = fs
        .open_existing(&directory, &EntryName::new("CERTIFICATES").unwrap())
        .unwrap();
    let offset = 2 * 4161 + 100;
    let mut byte = [0];
    fs.read_at(&certificates, offset, &mut byte).unwrap();
    fs.write_at(&certificates, offset, &[byte[0] ^ 1]).unwrap();
    assert!(
        recovery
            .next_recovered_transaction(&mut fs, &mut cursor)
            .is_err()
    );
    fs.write_at(&certificates, offset, &byte).unwrap();
    assert!(
        recovery
            .next_recovered_transaction(&mut fs, &mut cursor)
            .is_err()
    );
    assert!(recovery.finish_transaction_cursor(cursor).is_err());
}

#[test]
fn certificate_window_cursor_every_read_error_and_crash_preserves_restart() {
    for size in [2, 64] {
        let (mut fs, name, outcomes, _) = fixture();
        let (recovery, _) = open_disk(&mut fs, &name);
        let last = outcomes[2].revision;
        let mut cursor = recovery
            .open_transaction_cursor_with_certificate_window(
                CommitRevision::FIRST,
                last,
                3,
                100_000,
                size,
            )
            .unwrap();
        fs.arm(FaultPlan::default()).unwrap();
        while recovery
            .next_recovered_transaction(&mut fs, &mut cursor)
            .unwrap()
            .is_some()
        {}
        let reads = fs.operation_count(Operation::ReadAt);
        recovery.finish_transaction_cursor(cursor).unwrap();
        assert!(reads > 4);
        for occurrence in 1..=reads {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut fs, name, outcomes, _) = fixture();
                let (recovery, _) = open_disk(&mut fs, &name);
                let mut cursor = recovery
                    .open_transaction_cursor_with_certificate_window(
                        CommitRevision::FIRST,
                        last,
                        3,
                        100_000,
                        size,
                    )
                    .unwrap();
                fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation: Operation::ReadAt,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                loop {
                    match recovery.next_recovered_transaction(&mut fs, &mut cursor) {
                        Ok(Some(_)) => {}
                        Ok(None) => panic!("fault must reject the range"),
                        Err(_) => break,
                    }
                }
                assert_eq!(fs.pending_faults(), 0);
                assert!(recovery.finish_transaction_cursor(cursor).is_err());
                drop(recovery);
                fs.restart().unwrap();
                fs.arm(FaultPlan::default()).unwrap();
                let (restarted, _) = open_disk(&mut fs, &name);
                let mut cursor = restarted
                    .open_transaction_cursor_with_certificate_window(
                        CommitRevision::FIRST,
                        last,
                        3,
                        100_000,
                        size,
                    )
                    .unwrap();
                for outcome in outcomes {
                    assert_eq!(
                        restarted
                            .next_recovered_transaction(&mut fs, &mut cursor)
                            .unwrap()
                            .unwrap()
                            .outcome(),
                        outcome
                    );
                }
                restarted.finish_transaction_cursor(cursor).unwrap();
            }
        }
    }
}
