use super::*;
use uste_storage::MIN_INDEX_CACHE_BYTES;

#[test]
fn packed_buffered_maintenance_staging_matches_uncached_and_rejects_foreign_future_and_budget() {
    let (mut fs, name, outcomes, _) = fixture();
    let (mut recovery, _) = open_disk(&mut fs, &name);
    let mut cursor = recovery
        .open_transaction_cursor(
            CommitRevision::FIRST,
            outcomes[2].revision,
            3,
            RANGE_BYTES + 6 * 4161,
        )
        .unwrap();
    let mut transactions = Vec::new();
    while let Some(transaction) = recovery
        .next_recovered_transaction(&mut fs, &mut cursor)
        .unwrap()
    {
        transactions.push(transaction);
    }
    recovery.finish_transaction_cursor(cursor).unwrap();
    let base = {
        let mut maintenance = recovery
            .packed_indexes_with_io(&mut fs, &transactions[0], certificate_limits())
            .unwrap();
        let deltas = [b"a".as_slice(), b"ab", b"b"]
            .map(|key| IndexDelta::new(key.to_vec(), None, Some(key.to_vec())).unwrap());
        let (base, cache) = maintenance
            .stage_buffered(
                &mut fs,
                [7; 32],
                1,
                None,
                &deltas,
                batches(),
                MIN_INDEX_CACHE_BYTES,
            )
            .unwrap();
        assert_eq!(cache.hits + cache.misses, 0);
        base
    };
    let changed = {
        let mut maintenance = recovery
            .packed_indexes_with_io(&mut fs, &transactions[1], certificate_limits())
            .unwrap();
        let deltas = [
            IndexDelta::new(
                b"a".to_vec(),
                Some(b"a".to_vec()),
                Some(b"changed".to_vec()),
            )
            .unwrap(),
            IndexDelta::new(b"ab".to_vec(), Some(b"ab".to_vec()), None).unwrap(),
            IndexDelta::new(b"ba".to_vec(), None, Some(b"new".to_vec())).unwrap(),
        ];
        let expected = maintenance
            .stage(&mut fs, [7; 32], 1, Some(base.tree()), &deltas, batches())
            .unwrap();
        let (actual, cache) = maintenance
            .stage_buffered(
                &mut fs,
                [7; 32],
                1,
                Some(base.tree()),
                &deltas,
                batches(),
                MIN_INDEX_CACHE_BYTES,
            )
            .unwrap();
        assert_eq!(actual.report(), expected.report());
        assert_eq!(
            actual.tree().family_descriptor().commitment,
            expected.tree().family_descriptor().commitment
        );
        assert!(cache.hits > 0 && cache.misses > 0);
        assert_eq!(cache.hits + cache.misses, actual.report().read_pages);
        assert!(cache.accounted_bytes <= MIN_INDEX_CACHE_BYTES);
        let (_, repeated) = maintenance
            .stage_buffered(
                &mut fs,
                [7; 32],
                1,
                Some(base.tree()),
                &deltas,
                batches(),
                MIN_INDEX_CACHE_BYTES,
            )
            .unwrap();
        assert_eq!(cache, repeated);
        assert_eq!(
            maintenance
                .get(&mut fs, actual.tree(), b"a", reads())
                .unwrap()
                .value
                .unwrap()
                .as_slice(),
            b"changed"
        );
        assert!(
            maintenance
                .get(&mut fs, actual.tree(), b"ab", reads())
                .unwrap()
                .value
                .is_none()
        );
        actual
    };
    fs.arm(FaultPlan::default()).unwrap();
    {
        let mut maintenance = recovery
            .packed_indexes_with_io(&mut fs, &transactions[0], certificate_limits())
            .unwrap();
        assert!(matches!(
            maintenance.stage_buffered(
                &mut fs,
                [7; 32],
                1,
                Some(changed.tree()),
                &[],
                batches(),
                MIN_INDEX_CACHE_BYTES
            ),
            Err(TransactionError::InvalidRequest)
        ));
        assert!(matches!(
            maintenance.stage_buffered(
                &mut fs,
                [7; 32],
                1,
                Some(base.tree()),
                &[],
                batches(),
                MIN_INDEX_CACHE_BYTES - 1
            ),
            Err(TransactionError::Storage(StorageError::ResourceLimit))
        ));
    }
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    let (mut foreign_fs, foreign_name, _, _) = fixture();
    let (mut foreign, frontier) = open_disk(&mut foreign_fs, &foreign_name);
    let mut maintenance = foreign
        .packed_indexes_with_io(&mut foreign_fs, &frontier, certificate_limits())
        .unwrap();
    foreign_fs.arm(FaultPlan::default()).unwrap();
    assert!(
        maintenance
            .stage_buffered(
                &mut foreign_fs,
                [7; 32],
                1,
                Some(base.tree()),
                &[],
                batches(),
                MIN_INDEX_CACHE_BYTES
            )
            .is_err()
    );
    assert_eq!(foreign_fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(foreign_fs.operation_count(Operation::CreateNew), 0);
}
