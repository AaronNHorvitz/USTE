use super::*;
use uste_storage::fault::{FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation};
use uste_storage::{IndexGetLimits, IndexRunReadLimits, PageCache};
use uste_txn::{COORDINATOR_FIRST_REFERENCE_PROFILE_V1, CoordinatorFirstReferenceLimits};

#[path = "first_reference_rebase/reverse.rs"]
mod reverse;
#[path = "first_reference_rebase/usage.rs"]
mod usage;

type Fs = FaultFileSystem<MemoryFileSystem>;

fn suffix() -> CoordinatorFirstReferenceLimits {
    CoordinatorFirstReferenceLimits {
        maximum_owners: 1,
        maximum_groups: 1,
        maximum_encoded_bytes: 1_000_000,
    }
}

fn reopen(fs: &mut Fs, name: &EntryName, state: CounterState) -> FaultDiskCounter {
    reopen_certificate_mode(fs, name, state, false)
}

fn reopen_certificate_mode(
    fs: &mut Fs,
    name: &EntryName,
    state: CounterState,
    disk_certificates: bool,
) -> FaultDiskCounter {
    reopen_with_admission_passes(fs, name, state, disk_certificates, 1)
}

fn reopen_with_admission_passes(
    fs: &mut Fs,
    name: &EntryName,
    state: CounterState,
    disk_certificates: bool,
    admission_passes: u64,
) -> FaultDiskCounter {
    reopen_storage_mode(
        fs,
        name,
        state,
        if disk_certificates {
            StorageMode::DiskCertificates
        } else {
            StorageMode::Legacy
        },
        admission_passes,
    )
}

enum StorageMode {
    Legacy,
    DiskCertificates,
    DiskBlobs,
}

pub(super) fn reopen_blob_mode(
    fs: &mut Fs,
    name: &EntryName,
    state: CounterState,
) -> FaultDiskCounter {
    reopen_storage_mode(fs, name, state, StorageMode::DiskBlobs, 1)
}

fn reopen_storage_mode(
    fs: &mut Fs,
    name: &EntryName,
    state: CounterState,
    mode: StorageMode,
    admission_passes: u64,
) -> FaultDiskCounter {
    static ENTROPY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(2_000_000);
    let entropy = ENTROPY.fetch_add(10_000, std::sync::atomic::Ordering::Relaxed);
    let revision = state.revision.unwrap();
    let (recovery, _) = if matches!(mode, StorageMode::DiskBlobs) {
        uste_txn::AuthenticatedIndexRecovery::open_with_disk_blob_metadata(
            fs,
            name,
            scope(),
            CounterEntropy::new(entropy),
            CounterEntropy::new(entropy + 5_000),
            &mut TestKeyAdapter,
            disk_inventory_writes::storage_limits(),
            &mut PageCache::new(64 * 1024).unwrap(),
        )
        .map(|(recovery, report, _)| (recovery, report))
    } else if matches!(mode, StorageMode::DiskCertificates) {
        uste_txn::AuthenticatedIndexRecovery::open_with_disk_certificate_anchors(
            fs,
            name,
            scope(),
            CounterEntropy::new(entropy),
            CounterEntropy::new(entropy + 5_000),
            &mut TestKeyAdapter,
            uste_storage::journal::CertificateAnchorReadLimits::new(4, 4 * 4161).unwrap(),
        )
        .map(|(recovery, report, _)| (recovery, report))
    } else {
        uste_txn::AuthenticatedIndexRecovery::open(
            fs,
            name,
            scope(),
            CounterEntropy::new(entropy),
            CounterEntropy::new(entropy + 5_000),
            &mut TestKeyAdapter,
        )
    }
    .unwrap();
    let lookup = IndexGetLimits::new(16, 136).unwrap();
    let run = IndexRunReadLimits::new(16, 10, 4096).unwrap();
    let mut cache = PageCache::new(64 * 1024).unwrap();
    let transaction_root = recovery
        .load_index_root_manifests(fs, uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1)
        .unwrap()
        .into_iter()
        .find(|root| root.revision() == revision)
        .unwrap();
    let transactions = uste_txn::admit_coordinator_transaction_index_for_recovery(
        &recovery,
        fs,
        transaction_root,
        uste_txn::CoordinatorTransactionAdmissionLimits {
            run,
            lookup,
            maximum_groups: revision.get(),
            maximum_encoded_bytes: 1_000_000,
        },
        &mut cache,
    )
    .unwrap();
    let candidate =
        uste_txn::load_coordinator_metadata_candidates_for_recovery::<CounterState, _, _, _, _>(
            &recovery, fs,
        )
        .unwrap()
        .into_iter()
        .find(|root| root.revision() == revision)
        .unwrap();
    let first = recovery
        .load_index_root_manifests(fs, COORDINATOR_FIRST_REFERENCE_PROFILE_V1)
        .unwrap()
        .into_iter()
        .find(|root| root.anchor() == candidate.anchor());
    let maximum_owners = if matches!(mode, StorageMode::DiskBlobs) {
        3
    } else {
        2
    };
    let limits = uste_txn::CoordinatorDiskAdmissionLimits {
        metadata: CoordinatorMetadataLoadLimits::new(
            revision.get(),
            maximum_owners,
            revision.get() + maximum_owners + 1,
            32,
            4096,
        )
        .unwrap(),
        lookup,
        maximum_total_journal_groups: revision.get() * admission_passes,
        maximum_encoded_bytes_per_pass: 1_000_000,
    };
    let has_first = first.is_some();
    let mut base = if let Some(first) = first {
        uste_txn::admit_coordinator_disk_base_with_first_references(
            &recovery,
            fs,
            candidate,
            transactions,
            first,
            run,
            limits,
            &mut cache,
        )
    } else {
        uste_txn::admit_coordinator_disk_base(
            &recovery,
            fs,
            candidate,
            transactions,
            limits,
            &mut cache,
        )
    }
    .unwrap();
    assert_eq!(base.has_first_reference_evidence(), has_first);
    if let Some(root) = recovery
        .load_index_root_manifests(fs, uste_txn::COORDINATOR_BLOB_USAGE_PROFILE_V1)
        .unwrap()
        .into_iter()
        .find(|root| root.anchor() == base.anchor())
    {
        base.admit_blob_usage_index(&recovery, fs, root, usage::limits(), &mut cache)
            .unwrap();
    }
    uste_txn::DiskCommitCoordinator::recover_from_admitted_base(
        recovery,
        fs,
        base,
        state,
        RetentionDays::new(30).unwrap(),
        uste_txn::DiskCoordinatorRecoveryLimits {
            overlay: uste_txn::CoordinatorRecoveryLimits::new(2, 1).unwrap(),
            lookup,
            maximum_encoded_bytes: 1_000_000,
        },
        &mut cache,
    )
    .unwrap()
}

pub(super) fn fixture(
    initial_owner: bool,
) -> (Fs, EntryName, FaultDiskCounter, CounterState, BlobInventory) {
    let name = EntryName::new("first-reference-rebase").unwrap();
    let mut fs = FaultFileSystem::new(MemoryFileSystem::default(), FaultPlan::default());
    let vault = KeyVault::create(
        scope().database(),
        &mut TestKeyAdapter,
        CounterEntropy::new(950_000),
    )
    .unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        vault,
        CounterEntropy::new(960_000),
        CounterState::new(scope()),
    )
    .unwrap();
    let mut upload = coordinator.start_blob_upload(scope()).unwrap();
    coordinator
        .write_blob_upload(&mut fs, &mut upload, b"first owner remains principal one")
        .unwrap();
    let old = coordinator
        .finish_blob_upload(&mut fs, &mut upload)
        .unwrap();
    let inventory = BlobInventory::new(scope(), [old]).unwrap();
    coordinator
        .commit(
            &mut fs,
            TransactionRequest {
                blob_inventory: initial_owner.then_some(&inventory),
                ..request(1, &1_u64.to_be_bytes())
            },
            &mut TestClock(20),
            &NeverCancel,
        )
        .unwrap();
    // Occupy both fallback slots so removal faults target actual replacement, not NotFound.
    for _ in 0..2 {
        publish_coordinator_metadata_root(&mut coordinator, &mut fs).unwrap();
        uste_txn::publish_coordinator_transaction_index(&mut coordinator, &mut fs).unwrap();
        if initial_owner {
            uste_txn::publish_coordinator_first_reference_index(
                &mut coordinator,
                &mut fs,
                suffix(),
            )
            .unwrap();
        }
    }
    let base_state = coordinator.read_view().unwrap().state().clone();
    drop(coordinator);
    fs.restart().unwrap();
    let mut disk = reopen(&mut fs, &name, base_state.clone());
    let mut upload = disk.start_blob_upload(scope()).unwrap();
    disk.write_blob_upload(&mut fs, &mut upload, b"new owner is principal two")
        .unwrap();
    let new = disk.finish_blob_upload(&mut fs, &mut upload).unwrap();
    let inventory = BlobInventory::new(
        scope(),
        if initial_owner {
            vec![old, new]
        } else {
            vec![new]
        },
    )
    .unwrap();
    disk.commit(
        &mut fs,
        TransactionRequest {
            blob_inventory: Some(&inventory),
            ..request(2, &2_u64.to_be_bytes())
        },
        &mut TestClock(20),
        &NeverCancel,
        IndexGetLimits::new(16, 136).unwrap(),
        &mut PageCache::new(64 * 1024).unwrap(),
    )
    .unwrap();
    (fs, name, disk, base_state, inventory)
}

#[test]
fn first_reference_rebase_preserves_three_roots_through_every_publication_fault() {
    let operations = [
        Operation::CreateNew,
        Operation::WriteAt,
        Operation::SetLen,
        Operation::SyncAll,
        Operation::SyncDirectory,
        Operation::RemoveFile,
    ];
    let (mut fs, _, mut disk, _, _) = fixture(true);
    assert!(matches!(
        disk.rebase_metadata(&mut fs, metadata_rebase_limits()),
        Err(TransactionError::InvalidRequest)
    ));
    for limits in [
        CoordinatorFirstReferenceLimits {
            maximum_owners: 0,
            ..suffix()
        },
        CoordinatorFirstReferenceLimits {
            maximum_groups: 0,
            ..suffix()
        },
        CoordinatorFirstReferenceLimits {
            maximum_encoded_bytes: 1,
            ..suffix()
        },
    ] {
        assert!(
            disk.rebase_metadata_with_first_references(&mut fs, metadata_rebase_limits(), limits)
                .is_err()
        );
        assert_eq!(disk.overlay_counts(), (1, 1));
    }
    fs.arm(FaultPlan::default()).unwrap();
    disk.rebase_metadata_with_first_references(&mut fs, metadata_rebase_limits(), suffix())
        .unwrap();
    let counts = operations.map(|op| (op, fs.operation_count(op)));
    eprintln!("first-reference rebase publication boundaries: {counts:?}");
    drop(disk);
    for (operation, count) in counts {
        assert!(count > 0);
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut fs, name, mut disk, base_state, inventory) = fixture(true);
                let current_state = disk.state().unwrap().clone();
                fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                assert!(
                    disk.rebase_metadata_with_first_references(
                        &mut fs,
                        metadata_rebase_limits(),
                        suffix()
                    )
                    .is_err(),
                    "{operation:?}/{occurrence}/{action:?}"
                );
                assert_eq!(fs.pending_faults(), 0);
                assert_eq!(disk.overlay_counts(), (1, 1));
                assert!(disk.rebase_required());
                if fs.is_crashed() {
                    drop(disk);
                    fs.restart().unwrap();
                    disk = reopen(&mut fs, &name, base_state);
                }
                let mut cache = PageCache::new(64 * 1024).unwrap();
                let retry = disk
                    .commit(
                        &mut fs,
                        TransactionRequest {
                            blob_inventory: Some(&inventory),
                            ..request(2, &2_u64.to_be_bytes())
                        },
                        &mut TestClock(20),
                        &NeverCancel,
                        IndexGetLimits::new(16, 136).unwrap(),
                        &mut cache,
                    )
                    .unwrap();
                assert_eq!(retry.revision.get(), 2);
                disk.rebase_metadata_with_first_references(
                    &mut fs,
                    metadata_rebase_limits(),
                    suffix(),
                )
                .unwrap();
                assert_eq!(disk.overlay_counts(), (0, 0));
                assert!(!disk.rebase_required());
                drop(disk);
                fs.restart().unwrap();
                let disk = reopen(&mut fs, &name, current_state);
                assert_eq!(disk.overlay_counts(), (0, 0));
                let usage = disk
                    .committed_blob_usage(
                        &mut fs,
                        PrincipalDigest::from_bytes([1; 32]),
                        uste_txn::DiskBlobAccountingLimits {
                            base: IndexRunReadLimits::new(16, 10, 4096).unwrap(),
                            maximum_total_owners: 2,
                        },
                    )
                    .unwrap();
                assert_eq!(usage.owners, 2);
                assert_eq!(
                    usage.principal_bytes,
                    b"first owner remains principal one".len() as u64
                );
            }
        }
    }
}

fn blob_read_limits(groups: u64) -> uste_txn::DiskBlobReadLimits {
    uste_txn::DiskBlobReadLimits {
        lookup: IndexGetLimits::new(16, 136).unwrap(),
        certificate: uste_storage::journal::CertificateAnchorReadLimits::new(4, 4 * 4161).unwrap(),
        inventory: uste_storage::journal::BlobReferenceProofLimits::new(2, 2 * 4161).unwrap(),
        maximum_discovery_groups: groups,
        maximum_discovery_encoded_bytes: 1_000_000,
    }
}

#[test]
fn disk_blob_reads_use_base_first_reference_or_bounded_overlay_and_survive_rebase() {
    let (mut fs, name, mut disk, _, inventory) = fixture(true);
    let old_bytes = b"first owner remains principal one";
    let new_bytes = b"new owner is principal two";
    let old = *inventory
        .references()
        .iter()
        .find(|r| r.byte_len() == old_bytes.len() as u64)
        .unwrap();
    let new = *inventory
        .references()
        .iter()
        .find(|r| r.byte_len() == new_bytes.len() as u64)
        .unwrap();
    let mut cache = PageCache::new(64 * 1024).unwrap();
    let mut output = [0x55; 64];
    let read = disk
        .read_blob_range(
            &mut fs,
            old,
            0,
            &mut output,
            blob_read_limits(0),
            &mut cache,
        )
        .unwrap();
    assert_eq!(&output[..read], old_bytes);
    output.fill(0x55);
    assert!(
        disk.read_blob_range(
            &mut fs,
            new,
            0,
            &mut output,
            blob_read_limits(0),
            &mut cache
        )
        .is_err()
    );
    assert_eq!(output, [0x55; 64]);
    let read = disk
        .read_blob_range(
            &mut fs,
            new,
            0,
            &mut output,
            blob_read_limits(1),
            &mut cache,
        )
        .unwrap();
    assert_eq!(&output[..read], new_bytes);
    disk.rebase_metadata_with_first_references(&mut fs, metadata_rebase_limits(), suffix())
        .unwrap();
    assert_eq!(disk.overlay_counts(), (0, 0));
    let state = disk.state().unwrap().clone();
    drop(disk);
    fs.restart().unwrap();
    let disk = reopen_certificate_mode(&mut fs, &name, state, true);
    assert_eq!(disk.certificate_anchor_residency(), (false, 0));
    for (reference, bytes) in [(old, old_bytes.as_slice()), (new, new_bytes.as_slice())] {
        let read = disk
            .read_blob_range(
                &mut fs,
                reference,
                0,
                &mut output,
                blob_read_limits(0),
                &mut cache,
            )
            .unwrap();
        assert_eq!(&output[..read], bytes);
    }
    let changed = uste_storage::BlobReference::new(
        old.scope(),
        old.id(),
        old.byte_len() + 1,
        old.chunk_count(),
        old.content_digest(),
    )
    .unwrap();
    output.fill(0x55);
    assert!(matches!(
        disk.read_blob_range(
            &mut fs,
            changed,
            0,
            &mut output,
            blob_read_limits(0),
            &mut cache
        ),
        Err(TransactionError::InvalidRequest)
    ));
    assert_eq!(output, [0x55; 64]);
}

#[test]
fn disk_blob_reads_every_io_failure_returns_no_success_then_reopens_exactly() {
    disk_blob_read_fault_matrix(false);
    disk_blob_read_fault_matrix(true);
}

fn disk_blob_fixture() -> (Fs, EntryName, FaultDiskCounter, CounterState, BlobInventory) {
    let (mut fs, name, disk, state, inventory) = fixture(true);
    drop(disk);
    let disk = reopen_certificate_mode(&mut fs, &name, state.clone(), true);
    assert_eq!(disk.certificate_anchor_residency(), (false, 0));
    (fs, name, disk, state, inventory)
}

fn disk_blob_read_fault_matrix(overlay: bool) {
    let expected = if overlay {
        b"new owner is principal two".as_slice()
    } else {
        b"first owner remains principal one".as_slice()
    };
    let (mut baseline, _, disk, _, inventory) = disk_blob_fixture();
    let reference = *inventory
        .references()
        .iter()
        .find(|r| r.byte_len() == expected.len() as u64)
        .unwrap();
    let mut cache = PageCache::new(64 * 1024).unwrap();
    baseline.arm(FaultPlan::default()).unwrap();
    disk.read_blob_range(
        &mut baseline,
        reference,
        0,
        &mut [0; 64],
        blob_read_limits(1),
        &mut cache,
    )
    .unwrap();
    for operation in [
        Operation::OpenExisting,
        Operation::Metadata,
        Operation::ReadAt,
    ] {
        let count = baseline.operation_count(operation);
        assert!(count > 0);
        eprintln!(
            "disk_blob_read overlay={overlay} operation={operation:?} boundaries={count} fault_cases={}",
            count * 3
        );
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut fs, name, disk, state, inventory) = disk_blob_fixture();
                let reference = *inventory
                    .references()
                    .iter()
                    .find(|r| r.byte_len() == expected.len() as u64)
                    .unwrap();
                let mut cache = PageCache::new(64 * 1024).unwrap();
                fs.arm(
                    FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap(),
                )
                .unwrap();
                assert!(
                    disk.read_blob_range(
                        &mut fs,
                        reference,
                        0,
                        &mut [0; 64],
                        blob_read_limits(1),
                        &mut cache
                    )
                    .is_err()
                );
                assert_eq!(fs.pending_faults(), 0);
                drop(disk);
                fs.restart().unwrap();
                let disk = reopen_certificate_mode(&mut fs, &name, state, true);
                let mut output = [0; 64];
                let read = disk
                    .read_blob_range(
                        &mut fs,
                        reference,
                        0,
                        &mut output,
                        blob_read_limits(1),
                        &mut cache,
                    )
                    .unwrap();
                assert_eq!(&output[..read], expected);
            }
        }
    }
}

#[test]
fn disk_blob_reads_legacy_base_requires_bounded_prefix_and_rejects_scope_before_io() {
    let (mut fs, name, mut disk, _, inventory) = fixture(false);
    let reference = inventory.references()[0];
    disk.rebase_metadata(&mut fs, metadata_rebase_limits())
        .unwrap();
    let state = disk.state().unwrap().clone();
    drop(disk);
    fs.restart().unwrap();
    // One legacy owner requires a correspondence pass plus an earliest-owner pass. This
    // admission allowance is separate from the read's independently tested discovery bound.
    let disk = reopen_with_admission_passes(&mut fs, &name, state, true, 2);
    let mut cache = PageCache::new(64 * 1024).unwrap();
    let mut output = [0x55; 64];
    assert!(
        disk.read_blob_range(
            &mut fs,
            reference,
            0,
            &mut output,
            blob_read_limits(1),
            &mut cache
        )
        .is_err()
    );
    assert_eq!(output, [0x55; 64]);
    let read = disk
        .read_blob_range(
            &mut fs,
            reference,
            0,
            &mut output,
            blob_read_limits(2),
            &mut cache,
        )
        .unwrap();
    assert_eq!(&output[..read], b"new owner is principal two");
    let foreign = uste_storage::BlobReference::new(
        NamespaceRef::new(scope().database(), NamespaceId::from_bytes([0xee; 16])),
        reference.id(),
        reference.byte_len(),
        reference.chunk_count(),
        reference.content_digest(),
    )
    .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert!(matches!(
        disk.read_blob_range(
            &mut fs,
            foreign,
            0,
            &mut output,
            blob_read_limits(2),
            &mut cache
        ),
        Err(TransactionError::InvalidRequest)
    ));
    assert!(matches!(
        disk.read_blob_range(
            &mut fs,
            reference,
            reference.byte_len() + 1,
            &mut output,
            blob_read_limits(2),
            &mut cache
        ),
        Err(TransactionError::ResourceLimit)
    ));
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::OpenExisting), 0);
}

#[test]
fn disk_blob_reads_uncertain_commit_refuses_before_io_and_restart_restores_reads() {
    let (mut fs, name, mut disk, state, inventory) = disk_blob_fixture();
    let reference = inventory.references()[0];
    let mut cache = PageCache::new(64 * 1024).unwrap();
    fs.arm(
        FaultPlan::new([FaultPoint {
            operation: Operation::SyncData,
            occurrence: 2,
            action: FaultAction::Error(uste_storage::AdapterErrorKind::Io),
        }])
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        disk.commit(
            &mut fs,
            request(3, &3_u64.to_be_bytes()),
            &mut TestClock(20),
            &NeverCancel,
            IndexGetLimits::new(16, 136).unwrap(),
            &mut cache,
        ),
        Err(TransactionError::OutcomeUnknown)
    );
    assert_eq!(fs.pending_faults(), 0);
    fs.arm(FaultPlan::default()).unwrap();
    let mut output = [0x55; 64];
    assert!(matches!(
        disk.read_blob_range(
            &mut fs,
            reference,
            0,
            &mut output,
            blob_read_limits(2),
            &mut cache
        ),
        Err(TransactionError::OutcomeUnknown)
    ));
    assert_eq!(output, [0x55; 64]);
    assert_eq!(fs.operation_count(Operation::OpenExisting), 0);
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    drop(disk);
    fs.restart().unwrap();
    let disk = reopen_certificate_mode(&mut fs, &name, state, true);
    let read = disk
        .read_blob_range(
            &mut fs,
            reference,
            0,
            &mut output,
            blob_read_limits(2),
            &mut cache,
        )
        .unwrap();
    assert_eq!(read as u64, reference.byte_len());
    let expected = if reference.byte_len() == b"new owner is principal two".len() as u64 {
        b"new owner is principal two".as_slice()
    } else {
        b"first owner remains principal one".as_slice()
    };
    assert_eq!(&output[..read], expected);
}

#[test]
fn first_reference_bootstraps_from_empty_owners_and_survives_owner_free_suffix() {
    let (mut fs, name, mut disk, _, inventory) = fixture(false);
    disk.rebase_metadata_with_first_references(&mut fs, metadata_rebase_limits(), suffix())
        .unwrap();
    let state = disk.state().unwrap().clone();
    drop(disk);
    fs.restart().unwrap();
    let mut disk = reopen(&mut fs, &name, state);
    let lookup = IndexGetLimits::new(16, 136).unwrap();
    let mut cache = PageCache::new(64 * 1024).unwrap();
    // Re-reference under another principal without adding an owner.
    disk.commit(
        &mut fs,
        TransactionRequest {
            blob_inventory: Some(&inventory),
            ..request(3, &3_u64.to_be_bytes())
        },
        &mut TestClock(20),
        &NeverCancel,
        lookup,
        &mut cache,
    )
    .unwrap();
    assert_eq!(disk.overlay_counts(), (1, 0));
    disk.rebase_metadata_with_first_references(
        &mut fs,
        metadata_rebase_limits(),
        CoordinatorFirstReferenceLimits {
            maximum_owners: 0,
            ..suffix()
        },
    )
    .unwrap();
    let state = disk.state().unwrap().clone();
    drop(disk);
    fs.restart().unwrap();
    let disk = reopen(&mut fs, &name, state);
    assert_eq!(
        disk.committed_blob_owner(&mut fs, inventory.references()[0], lookup, &mut cache)
            .unwrap(),
        Some(PrincipalDigest::from_bytes([2; 32]))
    );
    assert_eq!(disk.state().unwrap().value, 6);
}
