use super::*;
#[path = "packed_authorized_reads.rs"]
mod authorized;
use uste_txn::{CommittedBlobUsage, PackedBlobAccountingLimits};
fn accounting_limit(owners: u64) -> PackedBlobAccountingLimits {
    PackedBlobAccountingLimits {
        lookup: reads(),
        maximum_total_owners: owners,
    }
}

#[test]
fn packed_live_reads_match_base_overlay_failed_rebase_and_completed_rebase() {
    let (mut fs, _, mut live, transactions) = install(parts(3), 2, 1);
    for (index, transaction) in transactions.iter().enumerate() {
        let owner = if index == 1 {
            principal(1)
        } else {
            principal(0)
        };
        assert_eq!(
            live.outcome(
                &mut fs,
                owner,
                IdempotencyKey::from_bytes([index as u8 + 1; 16]),
                instant(7),
                reads()
            )
            .unwrap(),
            Some(transaction.outcome())
        );
        assert_eq!(
            live.transaction_outcome(
                &mut fs,
                owner,
                transaction.outcome().transaction_id,
                instant(7),
                reads()
            )
            .unwrap(),
            Some(transaction.outcome())
        );
        assert_eq!(
            live.transaction_outcome(
                &mut fs,
                PrincipalDigest::from_bytes([99; 32]),
                transaction.outcome().transaction_id,
                instant(40),
                reads()
            )
            .unwrap(),
            None
        );
        assert_eq!(
            live.transaction_outcome(
                &mut fs,
                owner,
                transaction.outcome().transaction_id,
                transaction.outcome().expires_at,
                reads()
            ),
            Err(TransactionError::IdempotencyExpired)
        );
    }
    let usage = live
        .committed_blob_usage(&mut fs, principal(0), accounting_limit(2))
        .unwrap();
    assert_eq!(
        usage,
        CommittedBlobUsage {
            namespace_bytes: 5,
            principal_bytes: 5,
            owners: 2
        }
    );
    let (_, outcomes) = append_pair(&mut fs, &mut live, 3);
    for phase in 0..3 {
        if phase == 1 {
            fs.arm(
                FaultPlan::new([FaultPoint {
                    operation: Operation::SyncAll,
                    occurrence: 1,
                    action: FaultAction::Error(AdapterErrorKind::Io),
                }])
                .unwrap(),
            )
            .unwrap();
            assert!(live.rebase_metadata(&mut fs, rebase_limits()).is_err());
            fs.arm(FaultPlan::default()).unwrap();
            assert!(live.rebase_required());
        } else if phase == 2 {
            live.rebase_metadata(&mut fs, rebase_limits())
                .unwrap()
                .unwrap();
        }
        for owner in [
            principal(0),
            principal(1),
            PrincipalDigest::from_bytes([99; 32]),
        ] {
            assert_eq!(
                live.committed_blob_usage(&mut fs, owner, accounting_limit(3))
                    .unwrap(),
                CommittedBlobUsage {
                    namespace_bytes: 10,
                    principal_bytes: if owner == principal(0) { 10 } else { 0 },
                    owners: 3
                }
            );
        }
        for offset in [0_u8, 1] {
            assert_eq!(
                live.outcome(
                    &mut fs,
                    principal(offset),
                    IdempotencyKey::from_bytes([23 + offset; 16]),
                    instant(7),
                    reads()
                )
                .unwrap(),
                Some(outcomes[usize::from(offset)])
            );
            assert_eq!(
                live.transaction_outcome(
                    &mut fs,
                    principal(offset),
                    outcomes[usize::from(offset)].transaction_id,
                    instant(7),
                    reads()
                )
                .unwrap(),
                Some(outcomes[usize::from(offset)])
            );
        }
        fs.arm(FaultPlan::default()).unwrap();
        assert_eq!(
            live.committed_blob_usage(&mut fs, principal(0), accounting_limit(2)),
            Err(TransactionError::ResourceLimit)
        );
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    }
}

#[test]
fn packed_live_metadata_every_observed_read_fault_returns_no_partial_usage_or_outcome() {
    for query in 0..3 {
        let (mut observed_fs, _, observed, _) = install(parts(3), 0, 0);
        let read = |live: &Live, fs: &mut Fs| -> Result<(), TransactionError> {
            match query {
                0 => {
                    live.outcome(
                        fs,
                        principal(0),
                        IdempotencyKey::from_bytes([3; 16]),
                        instant(7),
                        reads(),
                    )?;
                }
                1 => {
                    live.transaction_outcome(
                        fs,
                        principal(0),
                        TransactionId::from_bytes([13; 16]),
                        instant(7),
                        reads(),
                    )?;
                }
                _ => {
                    live.committed_blob_usage(fs, principal(0), accounting_limit(2))?;
                }
            }
            Ok(())
        };
        observed_fs.arm(FaultPlan::default()).unwrap();
        read(&observed, &mut observed_fs).unwrap();
        let boundaries = [
            Operation::OpenExisting,
            Operation::Metadata,
            Operation::ReadAt,
        ]
        .map(|op| (op, observed_fs.operation_count(op)));
        let mut cases = 0;
        for (operation, count) in boundaries {
            for occurrence in 1..=count {
                for action in [
                    FaultAction::Error(AdapterErrorKind::Io),
                    FaultAction::CrashBefore,
                    FaultAction::CrashAfter,
                ] {
                    let (mut fs, _, live, _) = install(parts(3), 0, 0);
                    fs.arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                    assert!(read(&live, &mut fs).is_err());
                    assert_eq!(fs.pending_faults(), 0);
                    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
                    cases += 1;
                }
            }
        }
        assert_eq!(cases, [36, 36, 45][query]);
    }
}
