use super::*;
use uste_storage::{MAX_INDEX_CACHE_BYTES, MIN_INDEX_CACHE_BYTES};
use uste_txn::{PackedQuotaPrefix, stage_packed_quota_prefix};

#[test]
fn packed_buffered_metadata_staging_matches_reference_retries_first_owners_and_usage() {
    for budget in [MIN_INDEX_CACHE_BYTES, 1024 * 1024] {
        let (mut fs, name, outcomes, references) = populated(32);
        let (mut recovery, transactions) = transactions(&mut fs, &name, 32);
        let mut primary: Option<PackedCoordinatorPrefix> = None;
        let mut quota: Option<PackedQuotaPrefix> = None;
        let selected = PackedCoordinatorLimits {
            staging_cache_bytes: Some(budget),
            ..limits()
        };
        let mut hits = 0;
        for (index, transaction) in transactions.iter().enumerate() {
            let (expected, work) = stage_packed_coordinator_prefix(
                &mut recovery,
                &mut fs,
                primary.as_ref(),
                transaction,
                limits(),
            )
            .unwrap();
            let (next, report) = stage_packed_coordinator_prefix(
                &mut recovery,
                &mut fs,
                primary.as_ref(),
                transaction,
                selected,
            )
            .unwrap();
            assert_eq!(
                next.families().map(|f| f.commitment),
                expected.families().map(|f| f.commitment)
            );
            assert_eq!(report.batches, work.batches);
            assert_eq!(
                (
                    report.new_owners,
                    report.owner_lookup_pages,
                    report.owner_lookup_bytes
                ),
                (
                    work.new_owners,
                    work.owner_lookup_pages,
                    work.owner_lookup_bytes
                )
            );
            for (cache, work) in report.staging_caches.into_iter().zip(report.batches) {
                let cache = cache.unwrap();
                assert_eq!(cache.hits + cache.misses, work.read_pages);
                assert!(cache.accounted_bytes <= budget);
                hits += cache.hits;
            }
            let (expected_quota, work) = stage_packed_quota_prefix(
                &mut recovery,
                &mut fs,
                quota.as_ref(),
                &next,
                transaction,
                limits(),
            )
            .unwrap();
            let (next_quota, report) = stage_packed_quota_prefix(
                &mut recovery,
                &mut fs,
                quota.as_ref(),
                &next,
                transaction,
                selected,
            )
            .unwrap();
            assert_eq!(
                next_quota.families().map(|f| f.commitment),
                expected_quota.families().map(|f| f.commitment)
            );
            assert_eq!(report.batches, work.batches);
            assert_eq!(
                (
                    report.new_owners,
                    report.charged_bytes,
                    report.primary_lookup_pages,
                    report.primary_lookup_bytes,
                    report.principal_lookup
                ),
                (
                    work.new_owners,
                    work.charged_bytes,
                    work.primary_lookup_pages,
                    work.primary_lookup_bytes,
                    work.principal_lookup
                )
            );
            for (cache, work) in report.staging_caches.into_iter().zip(report.batches) {
                let cache = cache.unwrap();
                assert_eq!(cache.hits + cache.misses, work.read_pages);
                assert!(cache.accounted_bytes <= budget);
                hits += cache.hits;
            }
            let maintenance = recovery
                .packed_indexes_with_io(&mut fs, transaction, limits().certificates)
                .unwrap();
            assert_eq!(
                next.retry(
                    &maintenance,
                    &mut fs,
                    principal(index as u8),
                    IdempotencyKey::from_bytes([index as u8 + 1; 16]),
                    reads()
                )
                .unwrap()
                .0,
                Some(outcomes[index])
            );
            assert_eq!(
                next.transaction(
                    &maintenance,
                    &mut fs,
                    outcomes[index].transaction_id,
                    reads()
                )
                .unwrap()
                .0,
                Some((principal(index as u8), outcomes[index]))
            );
            for (owner, reference) in references
                .iter()
                .take(if index == 0 { 1 } else { 2 })
                .enumerate()
            {
                assert_eq!(
                    next.owner(&maintenance, &mut fs, reference.id(), reads())
                        .unwrap()
                        .0,
                    Some((*reference, principal(owner as u8)))
                );
                assert_eq!(
                    next.first_revision(&maintenance, &mut fs, reference.id(), reads())
                        .unwrap()
                        .0,
                    Some(CommitRevision::new(owner as u64 + 1).unwrap())
                );
            }
            let usage = next_quota
                .usage(&maintenance, &mut fs, &next, principal(0), reads())
                .unwrap()
                .0;
            assert_eq!(usage.namespace_bytes, if index == 0 { 5 } else { 11 });
            assert_eq!(usage.principal_bytes, 5);
            primary = Some(next);
            quota = Some(next_quota);
        }
        assert!(hits > 0);
    }
}

#[test]
fn packed_buffered_metadata_invalid_cache_admission_precedes_io() {
    let (mut fs, name, _, _) = populated(3);
    let (mut recovery, transactions) = transactions(&mut fs, &name, 3);
    let (primary, _) =
        stage_packed_coordinator_prefix(&mut recovery, &mut fs, None, &transactions[0], limits())
            .unwrap();
    for budget in [0, MIN_INDEX_CACHE_BYTES - 1, MAX_INDEX_CACHE_BYTES + 1] {
        let selected = PackedCoordinatorLimits {
            staging_cache_bytes: Some(budget),
            ..limits()
        };
        fs.arm(FaultPlan::default()).unwrap();
        assert!(matches!(
            stage_packed_coordinator_prefix(
                &mut recovery,
                &mut fs,
                None,
                &transactions[0],
                selected
            ),
            Err(TransactionError::ResourceLimit)
        ));
        assert!(matches!(
            stage_packed_quota_prefix(
                &mut recovery,
                &mut fs,
                None,
                &primary,
                &transactions[0],
                selected
            ),
            Err(TransactionError::ResourceLimit)
        ));
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    }
}
