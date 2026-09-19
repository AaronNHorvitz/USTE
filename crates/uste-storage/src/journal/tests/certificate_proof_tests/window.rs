use super::*;

#[test]
fn certificate_windows_match_independent_proofs_and_exact_shared_work() {
    let mut f = Fixture::new(false);
    f.store.certificate_anchors.clear();
    let limits = CertificateAnchorReadLimits::new(3, 3 * SMALL_ENVELOPE_BYTES).unwrap();
    for first in 1..=3 {
        for last in first..=3 {
            let reads = if first == last { 4 - last } else { 5 - first };
            let bytes = reads * SMALL_ENVELOPE_BYTES;
            f.fs.arm(FaultPlan::default()).unwrap();
            let window = f
                .store
                .authenticate_certificate_window(
                    &mut f.fs,
                    CommitRevision::new(first).unwrap(),
                    CommitRevision::new(last).unwrap(),
                    limits,
                    bytes,
                )
                .unwrap();
            assert_eq!(window.len() as u64, last - first + 1);
            assert!(!window.is_empty());
            assert_eq!(
                window.report(),
                CertificateAnchorReadReport {
                    certificates: reads,
                    encoded_bytes: bytes
                }
            );
            assert_eq!(f.fs.operation_count(Operation::ReadAt), reads);
            for (sequence, proof) in (first..=last).zip(window.into_proofs()) {
                let independent = f.proof(sequence as usize - 1).unwrap();
                assert_eq!(proof.anchor(), independent.anchor());
                assert_eq!(proof.report().encoded_bytes, bytes);
                f.store.validate_certificate_anchor_proof(&proof).unwrap();
            }
            f.fs.arm(FaultPlan::default()).unwrap();
            assert!(matches!(
                f.store.authenticate_certificate_window(
                    &mut f.fs,
                    CommitRevision::new(first).unwrap(),
                    CommitRevision::new(last).unwrap(),
                    limits,
                    bytes - 1
                ),
                Err(StorageError::ResourceLimit)
            ));
            assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
        }
    }
    assert!(f.store.certificate_anchors.is_empty());
}

#[test]
fn certificate_windows_enforce_the_fixed_residency_cap_before_io() {
    let mut f = Fixture::new(false);
    for _ in 4..=70 {
        f.store
            .append_group(
                &mut f.fs,
                CommitInput {
                    encoded_group: b"base",
                    logical_event_digest: [1; 32],
                },
            )
            .unwrap();
    }
    f.store.certificate_anchors.clear();
    let limits = CertificateAnchorReadLimits::new(70, 70 * SMALL_ENVELOPE_BYTES).unwrap();
    let rev = |n| CommitRevision::new(n).unwrap();
    let bytes = 71 * SMALL_ENVELOPE_BYTES;
    f.fs.arm(FaultPlan::default()).unwrap();
    let window = f
        .store
        .authenticate_certificate_window(&mut f.fs, rev(1), rev(64), limits, bytes)
        .unwrap();
    assert_eq!(window.len(), 64);
    assert_eq!(window.report().certificates, 71);
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 71);
    assert!(64 * size_of::<CertificateAnchorProof>() < 24 * 1024);
    f.fs.arm(FaultPlan::default()).unwrap();
    for (first, last) in [(1, 65), (1, 71), (2, 1)] {
        assert!(
            f.store
                .authenticate_certificate_window(&mut f.fs, rev(first), rev(last), limits, bytes)
                .is_err()
        );
    }
    assert!(
        f.store
            .authenticate_certificate_window(
                &mut f.fs,
                rev(1),
                rev(64),
                CertificateAnchorReadLimits::new(6, 6 * SMALL_ENVELOPE_BYTES).unwrap(),
                bytes
            )
            .is_err()
    );
    assert_eq!(f.fs.operation_count(Operation::ReadAt), 0);
    let proof = window.into_proofs().next().unwrap();
    f.store
        .append_group(
            &mut f.fs,
            CommitInput {
                encoded_group: b"next",
                logical_event_digest: [2; 32],
            },
        )
        .unwrap();
    assert!(f.store.validate_certificate_anchor_proof(&proof).is_err());
}

#[test]
fn certificate_window_faults_and_corruption_never_return_partial_receipts() {
    let limits = CertificateAnchorReadLimits::new(3, 3 * SMALL_ENVELOPE_BYTES).unwrap();
    for occurrence in 1..=4 {
        for action in [
            FaultAction::Error(AdapterErrorKind::Io),
            FaultAction::CrashBefore,
            FaultAction::CrashAfter,
        ] {
            let mut f = Fixture::new(false);
            f.store.certificate_anchors.clear();
            f.fs.arm(
                FaultPlan::new([FaultPoint {
                    operation: Operation::ReadAt,
                    occurrence,
                    action,
                }])
                .unwrap(),
            )
            .unwrap();
            assert!(
                f.store
                    .authenticate_certificate_window(
                        &mut f.fs,
                        CommitRevision::FIRST,
                        CommitRevision::new(3).unwrap(),
                        limits,
                        4 * SMALL_ENVELOPE_BYTES
                    )
                    .is_err()
            );
            assert_eq!(f.fs.pending_faults(), 0);
            let database = f.store.database;
            drop(f.store);
            f.fs.restart().unwrap();
            let (store, _) = JournalStore::open(
                &mut f.fs,
                &entry("certificate-proof"),
                database,
                CounterEntropy::new(942_000),
                CounterEntropy::new(943_000),
                &mut TestKeyAdapter,
                |_| Ok(()),
            )
            .unwrap();
            f.store = store;
            assert_eq!(
                f.store
                    .authenticate_certificate_window(
                        &mut f.fs,
                        CommitRevision::FIRST,
                        CommitRevision::new(3).unwrap(),
                        limits,
                        4 * SMALL_ENVELOPE_BYTES
                    )
                    .unwrap()
                    .len(),
                3
            );
        }
    }
    for sequence in 1..=3 {
        let mut f = Fixture::new(false);
        let offset = sequence * SMALL_ENVELOPE_BYTES + 100;
        let mut byte = [0];
        f.fs.read_at(&f.store.certificate_file, offset, &mut byte)
            .unwrap();
        f.fs.write_at(&f.store.certificate_file, offset, &[byte[0] ^ 1])
            .unwrap();
        assert!(
            f.store
                .authenticate_certificate_window(
                    &mut f.fs,
                    CommitRevision::FIRST,
                    CommitRevision::new(3).unwrap(),
                    limits,
                    4 * SMALL_ENVELOPE_BYTES
                )
                .is_err()
        );
        f.fs.write_at(&f.store.certificate_file, offset, &byte)
            .unwrap();
        assert_eq!(
            f.store
                .authenticate_certificate_window(
                    &mut f.fs,
                    CommitRevision::FIRST,
                    CommitRevision::new(3).unwrap(),
                    limits,
                    4 * SMALL_ENVELOPE_BYTES
                )
                .unwrap()
                .len(),
            3
        );
    }
}

#[test]
fn certificate_windows_reject_authentic_fork_substitution() {
    for entire_suffix in [false, true] {
        let mut f = Fixture::new(false);
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
        assert!(matches!(
            f.store.authenticate_certificate_window(
                &mut f.fs,
                CommitRevision::FIRST,
                CommitRevision::new(3).unwrap(),
                CertificateAnchorReadLimits::new(3, 3 * SMALL_ENVELOPE_BYTES).unwrap(),
                4 * SMALL_ENVELOPE_BYTES
            ),
            Err(StorageError::IntegrityFailure)
        ));
    }
}
