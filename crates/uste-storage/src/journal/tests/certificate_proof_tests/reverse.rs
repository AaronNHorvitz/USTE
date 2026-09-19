use super::*;

fn fixture(disk: bool) -> Fixture {
    let mut f = Fixture::new(false);
    if disk {
        f.store.certificate_anchors.clear();
        f.store.certificate_read_limits =
            Some(CertificateAnchorReadLimits::new(3, 3 * SMALL_ENVELOPE_BYTES).unwrap());
    }
    f
}

#[test]
fn reverse_certificate_range_exact_order_bytes_and_no_resident_history() {
    for disk in [false, true] {
        let mut f = fixture(disk);
        for first in 1..=3 {
            for last in first..=3 {
                let groups = last - first + 1;
                let bytes = (2 * groups + if disk { 4 - last } else { 0 }) * SMALL_ENVELOPE_BYTES;
                let mut seen = Vec::new();
                let report = f
                    .store
                    .visit_committed_range_reverse_report(
                        &mut f.fs,
                        CommitRevision::new(first).unwrap(),
                        CommitRevision::new(last).unwrap(),
                        groups,
                        bytes,
                        |_, group| {
                            assert_eq!(group.encoded_group, b"base");
                            assert_eq!(
                                group.certificate_digest,
                                f.commits[group.revision.get() as usize - 1].certificate_digest
                            );
                            seen.push(group.revision.get());
                            Ok(())
                        },
                    )
                    .unwrap();
                assert_eq!(seen, (first..=last).rev().collect::<Vec<_>>());
                assert_eq!(
                    report,
                    JournalRangeReadReport {
                        groups,
                        encoded_bytes: bytes
                    }
                );
                assert!(matches!(
                    f.store.visit_committed_range_reverse_report(
                        &mut f.fs,
                        CommitRevision::new(first).unwrap(),
                        CommitRevision::new(last).unwrap(),
                        groups,
                        bytes - 1,
                        |_, _| Ok(())
                    ),
                    Err(StorageError::ResourceLimit)
                ));
            }
        }
        if disk {
            assert!(f.store.certificate_anchors.is_empty());
        }
        f.fs.arm(FaultPlan::default()).unwrap();
        for (first, last, groups, bytes) in [
            (3, 2, 3, 100_000),
            (1, 4, 4, 100_000),
            (1, 3, 2, 100_000),
            (1, 3, 3, 0),
        ] {
            assert!(
                f.store
                    .visit_committed_range_reverse_report(
                        &mut f.fs,
                        CommitRevision::new(first).unwrap(),
                        CommitRevision::new(last).unwrap(),
                        groups,
                        bytes,
                        |_, _| panic!("refused range cannot expose groups")
                    )
                    .is_err()
            );
        }
        assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
        f.store.poisoned = true;
        assert!(
            f.store
                .visit_committed_range_reverse_report(
                    &mut f.fs,
                    CommitRevision::FIRST,
                    CommitRevision::new(3).unwrap(),
                    3,
                    100_000,
                    |_, _| panic!("poisoned owner")
                )
                .is_err()
        );
        assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    }
}

#[test]
fn reverse_certificate_range_rejects_authentic_forks_and_late_corruption_before_exposure() {
    for disk in [false, true] {
        for entire_suffix in [false, true] {
            let mut f = fixture(disk);
            let mut fork = Fixture::new(true);
            for sequence in 2..=if entire_suffix { 3 } else { 2 } {
                let bytes = read_bounded(
                    &mut fork.fs,
                    &fork.store.certificate_file,
                    sequence * SMALL_ENVELOPE_BYTES,
                    SMALL_ENVELOPE_BYTES,
                )
                .unwrap();
                decode_certificate(
                    &f.store.vault,
                    f.store.database,
                    f.store.epoch,
                    f.store.certificate_log_id,
                    f.store.writer,
                    CommitRevision::new(sequence).unwrap(),
                    &bytes,
                )
                .unwrap();
                write_all_at(
                    &mut f.fs,
                    &f.store.certificate_file,
                    sequence * SMALL_ENVELOPE_BYTES,
                    &bytes,
                )
                .unwrap();
            }
            let mut seen = Vec::new();
            assert!(matches!(
                f.store.visit_committed_range_reverse_report(
                    &mut f.fs,
                    CommitRevision::FIRST,
                    CommitRevision::new(3).unwrap(),
                    3,
                    100_000,
                    |_, group| {
                        seen.push(group.revision.get());
                        Ok(())
                    }
                ),
                Err(StorageError::IntegrityFailure)
            ));
            assert_eq!(seen, if entire_suffix { vec![] } else { vec![3] });
        }
        // A certificate changed by callback I/O is rechecked against the authenticated successor.
        let mut f = fixture(disk);
        let mut seen = Vec::new();
        assert!(
            f.store
                .visit_committed_range_reverse_report(
                    &mut f.fs,
                    CommitRevision::FIRST,
                    CommitRevision::new(3).unwrap(),
                    3,
                    100_000,
                    |fs, group| {
                        seen.push(group.revision.get());
                        write_all_at(
                            fs,
                            &f.store.certificate_file,
                            2 * SMALL_ENVELOPE_BYTES + 100,
                            &[0xFF],
                        )?;
                        Ok(())
                    }
                )
                .is_err()
        );
        assert_eq!(seen, vec![3]);
    }
}

#[test]
fn reverse_certificate_range_every_read_fault_is_terminal_and_restart_preserves_commits() {
    let mut baseline = fixture(true);
    baseline.fs.arm(FaultPlan::default()).unwrap();
    baseline
        .store
        .visit_committed_range_reverse_report(
            &mut baseline.fs,
            CommitRevision::FIRST,
            CommitRevision::new(3).unwrap(),
            3,
            100_000,
            |_, _| Ok(()),
        )
        .unwrap();
    let mut attempts = 0;
    for operation in [
        Operation::ReadAt,
        Operation::OpenExisting,
        Operation::Metadata,
    ] {
        for occurrence in 1..=baseline.fs.operation_count(operation) {
            for action in [
                FaultAction::Error(AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let mut f = fixture(true);
                f.fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                let mut seen = Vec::new();
                assert!(
                    f.store
                        .visit_committed_range_reverse_report(
                            &mut f.fs,
                            CommitRevision::FIRST,
                            CommitRevision::new(3).unwrap(),
                            3,
                            100_000,
                            |_, group| {
                                seen.push(group.revision.get());
                                Ok(())
                            }
                        )
                        .is_err()
                );
                assert_eq!(f.fs.pending_faults(), 0);
                assert!([3, 2, 1].starts_with(&seen));
                let database = f.store.database;
                drop(f.store);
                f.fs.restart().unwrap();
                let (store, _) = JournalStore::open_with_disk_certificate_anchors(
                    &mut f.fs,
                    &entry("certificate-proof"),
                    database,
                    CounterEntropy::new(944_000),
                    CounterEntropy::new(945_000),
                    &mut TestKeyAdapter,
                    CertificateAnchorReadLimits::new(3, 3 * SMALL_ENVELOPE_BYTES).unwrap(),
                    |_| Ok(()),
                )
                .unwrap();
                assert_eq!(store.frontier().unwrap().get(), 3);
                attempts += 1;
            }
        }
    }
    println!("reverse authenticated range fault attempts: {attempts}");
}
