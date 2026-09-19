use super::*;
use uste_storage::fault::{FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation};
use uste_txn::DiskCommitCheck;

#[test]
fn disk_commit_preflight_shares_retry_collision_and_admission_without_publication() {
    let scope = scope();
    let name = EntryName::new("disk-commit-check").unwrap();
    let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let vault = KeyVault::create(
        scope.database(),
        &mut TestKeyAdapter,
        CounterEntropy::new(700_000),
    )
    .unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope,
        RetentionDays::new(30).unwrap(),
        name.clone(),
        vault,
        CounterEntropy::new(710_000),
        CounterState::new(scope),
    )
    .unwrap();
    let first = 5_u64.to_be_bytes();
    let second = 7_u64.to_be_bytes();
    let mut clock = TestClock(20);
    let original = coordinator
        .commit(
            &mut filesystem,
            request(1, &first),
            &mut clock,
            &NeverCancel,
        )
        .unwrap();
    let base_state = coordinator.read_view().unwrap().state().clone();
    publish_coordinator_metadata_root(&mut coordinator, &mut filesystem).unwrap();
    uste_txn::publish_coordinator_transaction_index(&mut coordinator, &mut filesystem).unwrap();
    drop(coordinator);
    filesystem.restart().unwrap();
    let (mut disk, _) = reopen_fault_disk_counter(&mut filesystem, &name, base_state.clone());
    let lookup = uste_storage::IndexGetLimits::new(64, 136).unwrap();
    let mut cache = uste_storage::PageCache::new(64 * 1024).unwrap();
    let mut upload = disk.start_blob_upload(scope).unwrap();
    disk.write_blob_upload(&mut filesystem, &mut upload, b"uncommitted new owner")
        .unwrap();
    let reference = disk
        .finish_blob_upload(&mut filesystem, &mut upload)
        .unwrap();
    let inventory = BlobInventory::new(scope, [reference]).unwrap();
    filesystem
        .arm(
            FaultPlan::new([FaultPoint {
                operation: Operation::WriteAt,
                occurrence: 1,
                action: FaultAction::Error(uste_storage::AdapterErrorKind::Io),
            }])
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        disk.check_commit(
            &mut filesystem,
            request(2, &second),
            &mut clock,
            &NeverCancel,
            lookup,
            &mut cache
        )
        .unwrap(),
        DiskCommitCheck::Ready {
            revision: CommitRevision::new(2).unwrap()
        }
    );
    assert_eq!(
        disk.check_commit(
            &mut filesystem,
            request(1, &first),
            &mut clock,
            &AlwaysCancel,
            lookup,
            &mut cache
        )
        .unwrap(),
        DiskCommitCheck::Retry(original)
    );
    for conflicting in [
        request(1, &second),
        TransactionRequest {
            transaction_id: original.transaction_id,
            ..request(2, &second)
        },
    ] {
        assert_eq!(
            disk.check_commit(
                &mut filesystem,
                conflicting,
                &mut clock,
                &AlwaysCancel,
                lookup,
                &mut cache
            ),
            Err(TransactionError::Conflict)
        );
    }
    assert_eq!(
        disk.check_commit(
            &mut filesystem,
            request(1, &first),
            &mut TestClock(original.expires_at.seconds()),
            &NeverCancel,
            lookup,
            &mut cache
        ),
        Err(TransactionError::IdempotencyExpired)
    );
    assert_eq!(
        disk.check_commit(
            &mut filesystem,
            request(2, &second),
            &mut clock,
            &AlwaysCancel,
            lookup,
            &mut cache
        ),
        Err(TransactionError::Cancelled)
    );
    assert_eq!(
        disk.check_commit(
            &mut filesystem,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(2, &second)
            },
            &mut clock,
            &NeverCancel,
            lookup,
            &mut cache
        ),
        Err(TransactionError::ResourceLimit)
    );
    assert_eq!(filesystem.pending_faults(), 1);
    assert_eq!(filesystem.operation_count(Operation::WriteAt), 0);
    assert_eq!(disk.state().unwrap(), &base_state);
    assert_eq!(disk.overlay_counts(), (0, 0));
    assert_eq!(
        disk.checkpoint_anchor().unwrap().unwrap().0,
        original.revision
    );
    // The still-armed write fault must fire on actual publication, not be silently discarded.
    assert_eq!(
        disk.commit(
            &mut filesystem,
            request(2, &second),
            &mut clock,
            &NeverCancel,
            lookup,
            &mut cache
        ),
        Err(TransactionError::OutcomeUnknown)
    );
    assert_eq!(filesystem.pending_faults(), 0);
    assert_eq!(
        disk.check_commit(
            &mut filesystem,
            request(1, &first),
            &mut clock,
            &NeverCancel,
            lookup,
            &mut cache
        ),
        Err(TransactionError::OutcomeUnknown)
    );
    drop(disk);
    filesystem.restart().unwrap();
    let (mut disk, frontier) =
        reopen_fault_disk_counter(&mut filesystem, &name, base_state.clone());
    assert_eq!(frontier, 1);
    let mut cache = uste_storage::PageCache::new(64 * 1024).unwrap();
    let committed = disk
        .commit(
            &mut filesystem,
            request(2, &second),
            &mut clock,
            &NeverCancel,
            lookup,
            &mut cache,
        )
        .unwrap();
    assert_eq!(
        disk.check_commit(
            &mut filesystem,
            request(2, &second),
            &mut clock,
            &AlwaysCancel,
            lookup,
            &mut cache
        )
        .unwrap(),
        DiskCommitCheck::Retry(committed)
    );
    assert_eq!(
        disk.check_commit(
            &mut filesystem,
            request(3, &second),
            &mut clock,
            &NeverCancel,
            lookup,
            &mut cache
        )
        .unwrap(),
        DiskCommitCheck::Ready {
            revision: CommitRevision::new(3).unwrap()
        }
    );
    filesystem
        .arm(
            FaultPlan::new([FaultPoint {
                operation: Operation::WriteAt,
                occurrence: 1,
                action: FaultAction::Error(uste_storage::AdapterErrorKind::Io),
            }])
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        disk.commit(
            &mut filesystem,
            request(3, &second),
            &mut clock,
            &NeverCancel,
            lookup,
            &mut cache
        ),
        Err(TransactionError::OutcomeUnknown)
    );
    assert_eq!(filesystem.pending_faults(), 0);
    assert_eq!(
        disk.check_commit(
            &mut filesystem,
            request(2, &second),
            &mut clock,
            &NeverCancel,
            lookup,
            &mut cache
        ),
        Err(TransactionError::OutcomeUnknown)
    );
    drop(disk);
    filesystem.restart().unwrap();
    let (disk, frontier) = reopen_fault_disk_counter(&mut filesystem, &name, base_state);
    assert_eq!(frontier, 2);
    assert_eq!(disk.state().unwrap().value, 12);
}
