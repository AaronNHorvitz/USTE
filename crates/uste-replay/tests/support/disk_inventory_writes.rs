use super::*;
use uste_storage::fault::{FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation};
use uste_storage::journal::{
    BlobCatalogRecovery, BlobMetadataAdmissionLimits, BlobMetadataRebuildLimits,
    BlobRecoveryLimits, CertificateAnchorReadLimits, DiskBlobAppendLimits,
};
use uste_storage::{IndexGetLimits, IndexRunMergeLimits, IndexRunReadLimits, PageCache};

type Fs = FaultFileSystem<MemoryFileSystem>;

impl uste_txn::ExternallyPreparedTransactionState for CounterState {
    fn validate_external_prepared(
        &self,
        request: &[u8],
        inventory: Option<&BlobInventory>,
        revision: CommitRevision,
        prepared: &Self::Prepared,
    ) -> Result<(), ApplyError> {
        if self.prepare(request, inventory, revision)? == *prepared {
            Ok(())
        } else {
            Err(ApplyError::Conflict)
        }
    }
}

#[test]
fn disk_inventory_write_external_preparation_is_validated_before_publication() {
    let (mut fs, _, mut disk, _, inventory) = fixture();
    let bytes = 3_u64.to_be_bytes();
    let raw = TransactionRequest {
        blob_inventory: Some(&inventory),
        ..request(3, &bytes)
    };
    let prepared = disk
        .state()
        .unwrap()
        .prepare(&bytes, Some(&inventory), CommitRevision::new(3).unwrap())
        .unwrap();
    let mut wrong = prepared.clone();
    wrong.value += 1;
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        disk.commit_prepared_with_disk_inventory(
            &mut fs,
            raw,
            wrong,
            &mut TestClock(20),
            &NeverCancel,
            lookup(),
            append_limits(),
            &mut cache()
        ),
        Err(TransactionError::Conflict)
    );
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
    let outcome = disk
        .commit_prepared_with_disk_inventory(
            &mut fs,
            raw,
            prepared,
            &mut TestClock(20),
            &NeverCancel,
            lookup(),
            append_limits(),
            &mut cache(),
        )
        .unwrap();
    assert_eq!(outcome.revision.get(), 3);
    assert_eq!(disk.state().unwrap().value, 6);
    // Exact retry wins even when the supplied preparation is unusable for a fresh transaction.
    let wrong = CounterState::new(scope());
    assert_eq!(
        disk.commit_prepared_with_disk_inventory(
            &mut fs,
            raw,
            wrong,
            &mut TestClock(20),
            &AlwaysCancel,
            lookup(),
            append_limits(),
            &mut cache()
        )
        .unwrap(),
        outcome
    );
}

pub(super) fn storage_limits() -> BlobRecoveryLimits {
    let run = IndexRunReadLimits::new(32, 32, 8192).unwrap();
    BlobRecoveryLimits {
        catalog: BlobMetadataRebuildLimits {
            admission: BlobMetadataAdmissionLimits {
                maximum_blobs: 4,
                maximum_namespaces: 1,
                maximum_inventories: 4,
                maximum_reference_bindings: 16,
                run,
                lookup: lookup(),
                certificates: CertificateAnchorReadLimits::new(4, 4 * 4161).unwrap(),
                maximum_journal_groups: 4,
                maximum_journal_encoded_bytes: 1_000_000,
            },
            merge: IndexRunMergeLimits::new(run, 32, 8192, 32, 8192).unwrap(),
            maximum_merge_output_bytes: 8192,
        },
        catalog_recovery: BlobCatalogRecovery::AdmitOrRebuild,
        maximum_verified_blob_bytes_per_pass: 1024,
        maximum_uncommitted_segment_tails: 4,
    }
}

fn append_limits() -> DiskBlobAppendLimits {
    DiskBlobAppendLimits {
        maximum_pending_blobs: 1,
        maximum_pending_inventories: 1,
        maximum_pending_namespaces: 1,
        maximum_inventory_references: 3,
        maximum_verified_blob_bytes: 128,
        lookup: lookup(),
    }
}
fn lookup() -> IndexGetLimits {
    IndexGetLimits::new(32, 136).unwrap()
}
fn cache() -> PageCache {
    PageCache::new(64 * 1024).unwrap()
}
fn suffix(groups: u64) -> uste_txn::CoordinatorFirstReferenceLimits {
    uste_txn::CoordinatorFirstReferenceLimits {
        maximum_owners: 1,
        maximum_groups: groups,
        maximum_encoded_bytes: 1_000_000,
    }
}

fn fixture() -> (Fs, EntryName, FaultDiskCounter, CounterState, BlobInventory) {
    let (mut fs, name, mut disk, _, previous) = first_reference_rebase::fixture(true);
    disk.rebase_metadata_with_first_references(&mut fs, metadata_rebase_limits(), suffix(1))
        .unwrap();
    let state = disk.state().unwrap().clone();
    drop(disk);
    fs.restart().unwrap();
    let mut disk = first_reference_rebase::reopen_blob_mode(&mut fs, &name, state.clone());
    assert_eq!(disk.certificate_anchor_residency(), (false, 0));
    assert_eq!(disk.blob_metadata_residency(), (false, 0, 0, 0));
    let mut upload = disk.start_blob_upload(scope()).unwrap();
    disk.write_blob_upload(&mut fs, &mut upload, b"third")
        .unwrap();
    let reference = disk.finish_blob_upload(&mut fs, &mut upload).unwrap();
    let inventory = BlobInventory::new(
        scope(),
        previous.references().iter().copied().chain([reference]),
    )
    .unwrap();
    (fs, name, disk, state, inventory)
}

fn commit(
    disk: &mut FaultDiskCounter,
    fs: &mut Fs,
    inventory: &BlobInventory,
) -> Result<uste_txn::TransactionOutcome, TransactionError> {
    disk.commit_with_disk_inventory(
        fs,
        TransactionRequest {
            blob_inventory: Some(inventory),
            ..request(3, &3_u64.to_be_bytes())
        },
        &mut TestClock(20),
        &NeverCancel,
        lookup(),
        append_limits(),
        &mut cache(),
    )
}

#[test]
fn disk_inventory_write_storage_limits_and_wrong_mode_do_not_publish() {
    let (mut fs, _, mut disk, _, inventory) = fixture();
    let bytes = 3_u64.to_be_bytes();
    let raw = TransactionRequest {
        blob_inventory: Some(&inventory),
        ..request(3, &bytes)
    };
    let mut bounded = append_limits();
    bounded.maximum_verified_blob_bytes = 0;
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        disk.commit_with_disk_inventory(
            &mut fs,
            raw,
            &mut TestClock(20),
            &NeverCancel,
            lookup(),
            bounded,
            &mut cache()
        ),
        Err(TransactionError::ResourceLimit)
    );
    assert_eq!(disk.state().unwrap().value, 3);
    assert_eq!(disk.overlay_counts(), (0, 0));
    assert_eq!(disk.disk_blob_pending_residency(), Some((0, 0, 0)));
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
    assert_eq!(
        commit(&mut disk, &mut fs, &inventory)
            .unwrap()
            .revision
            .get(),
        3
    );

    let (mut fs, _, mut legacy, _, inventory) = first_reference_rebase::fixture(true);
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        commit(&mut legacy, &mut fs, &inventory),
        Err(TransactionError::InvalidRequest)
    );
    assert_eq!(legacy.state().unwrap().value, 3);
    assert_eq!(fs.operation_count(Operation::WriteAt), 0);
    // An already-certified retry requires no fresh storage capability.
    let bytes = 2_u64.to_be_bytes();
    let retry = TransactionRequest {
        blob_inventory: Some(&inventory),
        ..request(2, &bytes)
    };
    assert_eq!(
        legacy
            .commit_with_disk_inventory(
                &mut fs,
                retry,
                &mut TestClock(20),
                &AlwaysCancel,
                lookup(),
                bounded,
                &mut cache()
            )
            .unwrap()
            .revision
            .get(),
        2
    );
}

#[test]
fn disk_inventory_write_retry_collision_first_owner_and_independent_rebases() {
    let (mut fs, name, mut disk, _, inventory) = fixture();
    let outcome = commit(&mut disk, &mut fs, &inventory).unwrap();
    assert_eq!(outcome.revision.get(), 3);
    assert_eq!(disk.state().unwrap().value, 6);
    assert_eq!(disk.overlay_counts(), (1, 1));
    assert_eq!(disk.disk_blob_pending_residency(), Some((1, 1, 1)));
    let none = DiskBlobAppendLimits {
        maximum_pending_blobs: 0,
        maximum_pending_inventories: 0,
        maximum_pending_namespaces: 0,
        maximum_inventory_references: 0,
        maximum_verified_blob_bytes: 0,
        lookup: lookup(),
    };
    let bytes = 3_u64.to_be_bytes();
    let raw = TransactionRequest {
        blob_inventory: Some(&inventory),
        ..request(3, &bytes)
    };
    assert_eq!(
        disk.commit_with_disk_inventory(
            &mut fs,
            raw,
            &mut TestClock(20),
            &AlwaysCancel,
            lookup(),
            none,
            &mut cache()
        )
        .unwrap(),
        outcome
    );
    let collision = TransactionRequest {
        transaction_id: raw.transaction_id,
        ..request(4, &bytes)
    };
    assert_eq!(
        disk.commit_with_disk_inventory(
            &mut fs,
            collision,
            &mut TestClock(20),
            &NeverCancel,
            lookup(),
            none,
            &mut cache()
        ),
        Err(TransactionError::Conflict)
    );
    assert_eq!(
        disk.commit_with_disk_inventory(
            &mut fs,
            raw,
            &mut TestClock(2_592_020),
            &NeverCancel,
            lookup(),
            none,
            &mut cache()
        ),
        Err(TransactionError::IdempotencyExpired)
    );
    for reference in inventory.references() {
        let owner = if reference.byte_len() == 5 {
            3
        } else if reference.byte_len() == b"first owner remains principal one".len() as u64 {
            1
        } else {
            2
        };
        assert_eq!(
            disk.committed_blob_owner(&mut fs, *reference, lookup(), &mut cache())
                .unwrap(),
            Some(PrincipalDigest::from_bytes([owner; 32]))
        );
    }
    let fourth = disk
        .commit_with_disk_inventory(
            &mut fs,
            TransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(4, &4_u64.to_be_bytes())
            },
            &mut TestClock(20),
            &NeverCancel,
            lookup(),
            append_limits(),
            &mut cache(),
        )
        .unwrap();
    assert_eq!(fourth.revision.get(), 4);
    assert_eq!(disk.state().unwrap().value, 10);
    let usage = disk
        .committed_blob_usage(
            &mut fs,
            PrincipalDigest::from_bytes([3; 32]),
            uste_txn::DiskBlobAccountingLimits {
                base: IndexRunReadLimits::new(32, 4, 8192).unwrap(),
                maximum_total_owners: 3,
            },
        )
        .unwrap();
    assert_eq!(usage.owners, 3);
    assert_eq!(usage.principal_bytes, 5);
    assert_eq!(
        usage.namespace_bytes,
        inventory.references().iter().map(|r| r.byte_len()).sum()
    );
    disk.refresh_disk_blob_metadata(&mut fs, storage_limits().catalog, &mut cache())
        .unwrap();
    assert_eq!(disk.disk_blob_pending_residency(), Some((0, 0, 0)));
    assert_eq!(disk.overlay_counts(), (2, 1));
    disk.rebase_metadata_with_first_references(&mut fs, metadata_rebase_limits(), suffix(2))
        .unwrap();
    assert_eq!(disk.overlay_counts(), (0, 0));
    assert_eq!(disk.blob_metadata_residency(), (false, 0, 0, 0));
    let state = disk.state().unwrap().clone();
    drop(disk);
    fs.restart().unwrap();
    let mut disk = first_reference_rebase::reopen_blob_mode(&mut fs, &name, state);
    assert_eq!(commit(&mut disk, &mut fs, &inventory).unwrap(), outcome);
    assert_eq!(
        disk.transaction_outcome(
            &mut fs,
            PrincipalDigest::from_bytes([3; 32]),
            outcome.transaction_id,
            UtcInstant::new(20, 0).unwrap(),
            lookup(),
            &mut cache()
        )
        .unwrap(),
        Some(outcome)
    );
    assert_eq!(
        disk.transaction_outcome(
            &mut fs,
            PrincipalDigest::from_bytes([4; 32]),
            outcome.transaction_id,
            UtcInstant::new(20, 0).unwrap(),
            lookup(),
            &mut cache()
        )
        .unwrap(),
        None
    );
    assert_eq!(disk.overlay_counts(), (0, 0));
    assert_eq!(disk.disk_blob_pending_residency(), Some((0, 0, 0)));
}

#[test]
fn disk_inventory_write_faults_recover_exact_retry_without_history_maps() {
    let (mut sample_fs, _, mut sample, _, inventory) = fixture();
    sample_fs.arm(FaultPlan::default()).unwrap();
    commit(&mut sample, &mut sample_fs, &inventory).unwrap();
    let mut attempts = 0;
    for operation in [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SetLen,
        Operation::SyncAll,
        Operation::SyncDirectory,
        Operation::SyncData,
    ] {
        let count = sample_fs.operation_count(operation);
        eprintln!("disk coordinator inventory {operation:?}: {count} boundaries");
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut fs, name, mut disk, state, inventory) = fixture();
                fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                let result = commit(&mut disk, &mut fs, &inventory);
                assert_eq!(fs.pending_faults(), 0);
                if result == Err(TransactionError::OutcomeUnknown) {
                    assert_eq!(disk.state(), Err(TransactionError::OutcomeUnknown));
                    assert_eq!(
                        commit(&mut disk, &mut fs, &inventory),
                        Err(TransactionError::OutcomeUnknown)
                    );
                } else if result.is_err() {
                    assert_eq!(disk.state().unwrap().value, 3);
                } else {
                    assert_eq!(operation, Operation::OpenExisting);
                    assert_eq!(action, FaultAction::CrashAfter);
                }
                drop(disk);
                fs.restart().unwrap();
                let mut disk = first_reference_rebase::reopen_blob_mode(&mut fs, &name, state);
                let recovered = disk.state().unwrap().revision.unwrap().get();
                assert!((2..=3).contains(&recovered));
                if result.is_ok()
                    || (operation == Operation::SyncData
                        && occurrence == 2
                        && action == FaultAction::CrashAfter)
                {
                    assert_eq!(recovered, 3);
                }
                let outcome = commit(&mut disk, &mut fs, &inventory).unwrap();
                assert_eq!(outcome.revision.get(), 3);
                assert_eq!(disk.state().unwrap().value, 6);
                assert_eq!(disk.overlay_counts(), (1, 1));
                assert_eq!(disk.blob_metadata_residency(), (false, 0, 0, 0));
                attempts += 1;
            }
        }
    }
    eprintln!("disk coordinator inventory: {attempts} fault attempts");
}
