use super::*;
use uste_txn::{
    COORDINATOR_BLOB_USAGE_PROFILE_V1 as USAGE, COORDINATOR_FIRST_REFERENCE_PROFILE_V1 as FIRST,
    CoordinatorBlobUsageLimits,
};

fn usage_limits() -> CoordinatorBlobUsageLimits {
    CoordinatorBlobUsageLimits {
        run: IndexRunReadLimits::new(32, 32, 8192).unwrap(),
        lookup: lookup(),
        maximum_owners: 2,
    }
}
fn prepare(
    fs: &mut Fs,
    name: &EntryName,
    initial: bool,
    origin: bool,
    private_first: bool,
) -> (Recovery, CoordinatorDiskBase) {
    let mut recovery = open(fs, name);
    let genesis = recovery
        .recover_primary_genesis(fs, CounterState::new(scope()), 1_000_000, 1)
        .unwrap();
    let (metadata, transactions) = if origin {
        uste_txn::stage_primary_genesis_metadata(
            &mut recovery,
            fs,
            &genesis,
            1,
            metadata_rebase_limits().merge,
        )
        .unwrap()
    } else {
        let metadata = uste_txn::load_coordinator_metadata_candidates_for_recovery::<
            CounterState,
            _,
            _,
            _,
            _,
        >(&recovery, fs)
        .unwrap()
        .into_iter()
        .find(|r| r.revision() == CommitRevision::FIRST)
        .unwrap();
        let transactions = recovery
            .load_index_root_manifests(fs, uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1)
            .unwrap()
            .into_iter()
            .find(|r| r.revision() == CommitRevision::FIRST)
            .unwrap();
        (metadata, transactions)
    };
    let first = if origin || private_first {
        uste_txn::stage_genesis_first_references(
            &mut recovery,
            fs,
            &genesis,
            1,
            metadata_rebase_limits().merge,
        )
        .unwrap()
    } else if initial {
        Some(
            recovery
                .load_index_root_manifests(fs, FIRST)
                .unwrap()
                .into_iter()
                .find(|r| r.revision() == CommitRevision::FIRST)
                .unwrap(),
        )
    } else {
        None
    };
    let root = uste_txn::stage_genesis_blob_usage(
        &mut recovery,
        fs,
        &genesis,
        usage_limits(),
        metadata_rebase_limits().merge,
    )
    .unwrap();
    let mut base = admit_roots_with_first(fs, &recovery, transactions, metadata, first);
    base.admit_blob_usage_index(&recovery, fs, root, usage_limits(), &mut cache())
        .unwrap();
    (recovery, base)
}
fn recover_usage(
    fs: &mut Fs,
    recovery: Recovery,
    base: CoordinatorDiskBase,
    state: Anchored,
    usage_lookup: IndexGetLimits,
    domain: &mut Domain,
) -> Result<(Disk, PrimaryMetadataRecoveryReport), TransactionError> {
    Disk::recover_with_indexed_usage_streaming_domain(
        recovery,
        fs,
        base,
        state,
        RetentionDays::new(30).unwrap(),
        uste_txn::DiskCoordinatorRecoveryLimits {
            overlay: uste_txn::CoordinatorRecoveryLimits::new(0, 0).unwrap(),
            lookup: lookup(),
            maximum_encoded_bytes: 1_000_000,
        },
        limits(),
        usage_lookup,
        &mut cache(),
        domain,
    )
}
fn publish(disk: &mut Disk, fs: &mut Fs) -> Result<(), TransactionError> {
    disk.rebase_metadata_with_blob_usage(
        fs,
        metadata_rebase_limits(),
        uste_txn::CoordinatorFirstReferenceLimits {
            maximum_owners: 0,
            maximum_groups: 0,
            maximum_encoded_bytes: 0,
        },
        usage_limits(),
    )
}
fn compare(disk: &Disk, fs: &mut Fs, references: [BlobReference; 2], owners: [u8; 2], last: u8) {
    assert_eq!(disk.overlay_counts(), (0, 0));
    for principal in [1, 2, 3, 99] {
        let mut bytes = 0;
        let mut total = 0;
        let mut count = 0;
        for (index, reference) in references.into_iter().enumerate() {
            if owners[index] == 0 || (last == 1 && index == 1) {
                continue;
            }
            total += reference.byte_len();
            count += 1;
            if owners[index] == principal {
                bytes += reference.byte_len();
            }
        }
        let principal = PrincipalDigest::from_bytes([principal; 32]);
        let indexed = disk
            .committed_blob_usage_indexed(fs, principal, 2, lookup(), &mut cache())
            .unwrap();
        assert_eq!(
            indexed,
            uste_txn::CommittedBlobUsage {
                namespace_bytes: total,
                principal_bytes: bytes,
                owners: count
            }
        );
        assert_eq!(
            indexed,
            disk.committed_blob_usage(
                fs,
                principal,
                uste_txn::DiskBlobAccountingLimits {
                    base: usage_limits().run,
                    maximum_total_owners: 2
                }
            )
            .unwrap()
        );
    }
}

#[test]
fn streamed_quota_preserves_first_charges_updates_existing_principals_and_cold_admits() {
    for initial in [false, true] {
        for origin in [false, true] {
            for second in [1, 2] {
                for zero in [false, true] {
                    let (mut fs, name, state, references) =
                        fixture_principals(initial, !origin, 4, true, second, zero);
                    let (recovery, base) = prepare(&mut fs, &name, initial, origin, false);
                    let (mut disk, report) = recover_usage(
                        &mut fs,
                        recovery,
                        base,
                        state,
                        lookup(),
                        &mut Domain::default(),
                    )
                    .unwrap();
                    assert_eq!(report.revisions, 3);
                    assert_eq!(report.staged_runs, 24);
                    assert_eq!(
                        report.output_entries,
                        if initial && second == 2 { 48 } else { 45 }
                    );
                    assert_eq!(
                        report.output_logical_bytes,
                        if initial && second == 2 { 4824 } else { 4680 }
                    );
                    let owners = [if initial { 1 } else { second }, second];
                    compare(&disk, &mut fs, references, owners, 4);
                    assert!(publish(&mut disk, &mut fs).is_ok());
                    assert!(!disk.rebase_required());
                    let state = disk.state().unwrap().clone();
                    drop(disk);
                    fs.restart().unwrap();
                    let (recovery, mut base) =
                        admit_mode(&mut fs, &name, CommitRevision::new(4).unwrap(), true);
                    let root = recovery
                        .load_index_root_manifests(&mut fs, USAGE)
                        .unwrap()
                        .into_iter()
                        .find(|r| r.revision().get() == 4)
                        .unwrap();
                    base.admit_blob_usage_index(
                        &recovery,
                        &mut fs,
                        root,
                        usage_limits(),
                        &mut cache(),
                    )
                    .unwrap();
                    let (disk, report) = recover_usage(
                        &mut fs,
                        recovery,
                        base,
                        state,
                        lookup(),
                        &mut Domain::default(),
                    )
                    .unwrap();
                    assert_eq!(report, PrimaryMetadataRecoveryReport::default());
                    compare(&disk, &mut fs, references, owners, 4);
                }
            }
        }
    }
}

#[test]
fn streamed_quota_private_optional_roots_require_rebase_even_without_a_suffix() {
    for initial in [false, true] {
        for private_first in [false, true] {
            let (mut fs, name, state, references) = fixture_projection(initial, true, 1, true);
            let (recovery, base) = prepare(&mut fs, &name, initial, false, private_first);
            let (mut disk, report) = recover_usage(
                &mut fs,
                recovery,
                base,
                state,
                lookup(),
                &mut Domain::default(),
            )
            .unwrap();
            assert_eq!(report, PrimaryMetadataRecoveryReport::default());
            assert!(disk.rebase_required());
            compare(
                &disk,
                &mut fs,
                references,
                if initial { [1, 0] } else { [0, 0] },
                1,
            );
            publish(&mut disk, &mut fs).unwrap();
            assert!(!disk.rebase_required());
            assert!(
                disk.load_index_root_manifests(&mut fs, USAGE)
                    .unwrap()
                    .iter()
                    .any(|r| r.revision() == CommitRevision::FIRST)
            );
        }
    }
}

#[test]
fn streamed_quota_absence_and_projection_omission_refuse_before_io() {
    for mode in [0, 1, 2] {
        let (mut fs, name, state, _) = fixture_projection(true, true, 4, true);
        let (recovery, base) = if mode == 0 {
            admit_mode(&mut fs, &name, CommitRevision::FIRST, true)
        } else {
            prepare(&mut fs, &name, true, false, false)
        };
        fs.arm(FaultPlan::default()).unwrap();
        let mut domain = Domain::default();
        let result = if mode == 0 {
            recover_usage(&mut fs, recovery, base, state, lookup(), &mut domain)
        } else if mode == 1 {
            recover(&mut fs, recovery, base, state, limits(), &mut domain)
        } else {
            first_references::recover_first(&mut fs, recovery, base, state, &mut domain)
        };
        assert!(matches!(result, Err(TransactionError::InvalidRequest)));
        assert_eq!((domain.advanced, domain.finished), (0, false));
        assert_eq!(fs.operation_count(Operation::ReadAt), 0);
        assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    }
}

#[test]
fn streamed_quota_lookup_limit_cannot_publish_partial_charges() {
    let (mut fs, name, state, references) = fixture_principals(true, true, 4, true, 1, false);
    let (recovery, base) = prepare(&mut fs, &name, true, false, false);
    let mut domain = Domain::default();
    assert!(
        recover_usage(
            &mut fs,
            recovery,
            base,
            state.clone(),
            IndexGetLimits::new(32, 15).unwrap(),
            &mut domain
        )
        .is_err()
    );
    assert_eq!((domain.advanced, domain.finished), (1, false));
    fs.restart().unwrap();
    let (recovery, base) = prepare(&mut fs, &name, true, false, false);
    assert!(
        recovery
            .load_index_root_manifests(&mut fs, USAGE)
            .unwrap()
            .is_empty()
    );
    let (disk, _) = recover_usage(
        &mut fs,
        recovery,
        base,
        state,
        IndexGetLimits::new(32, 16).unwrap(),
        &mut Domain::default(),
    )
    .unwrap();
    compare(&disk, &mut fs, references, [1, 1], 4);
}

fn published_fixture() -> (Fs, EntryName, Anchored, [BlobReference; 2]) {
    let (mut fs, name, state, references) = fixture_projection(true, true, 1, true);
    let (recovery, base) = prepare(&mut fs, &name, true, false, false);
    let (mut disk, _) = recover_usage(
        &mut fs,
        recovery,
        base,
        state.clone(),
        lookup(),
        &mut Domain::default(),
    )
    .unwrap();
    publish(&mut disk, &mut fs).unwrap();
    drop(disk);
    fs.restart().unwrap();
    let (mut writer, _) = CommitCoordinator::open(
        &mut fs,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy::new(100_000_000),
        CounterEntropy::new(101_000_000),
        &mut TestKeyAdapter,
        CounterState::new(scope()),
    )
    .unwrap();
    let inventory = BlobInventory::new(scope(), references).unwrap();
    for revision in 2..=4 {
        writer
            .commit(
                &mut fs,
                TransactionRequest {
                    blob_inventory: (revision != 4).then_some(&inventory),
                    ..request(revision, &(u64::from(revision)).to_be_bytes())
                },
                &mut TestClock(20),
                &NeverCancel,
            )
            .unwrap();
    }
    drop(writer);
    fs.restart().unwrap();
    (fs, name, state, references)
}
fn admit_usage(fs: &mut Fs, name: &EntryName) -> (Recovery, CoordinatorDiskBase) {
    let (recovery, mut base) = admit_mode(fs, name, CommitRevision::FIRST, true);
    let root = recovery
        .load_index_root_manifests(fs, USAGE)
        .unwrap()
        .into_iter()
        .find(|r| r.revision() == CommitRevision::FIRST)
        .unwrap();
    base.admit_blob_usage_index(&recovery, fs, root, usage_limits(), &mut cache())
        .unwrap();
    (recovery, base)
}

#[test]
fn streamed_quota_published_base_and_late_certificate_corruption_preserve_charges() {
    let (mut fs, name, state, references) = published_fixture();
    let (recovery, base) = admit_usage(&mut fs, &name);
    let directory = fs.open_directory(&fs.root(), &name).unwrap();
    let file = fs
        .open_existing(&directory, &EntryName::new("CERTIFICATES").unwrap())
        .unwrap();
    let offset = 4 * 4161 + 100;
    let mut byte = [0];
    assert_eq!(fs.read_at(&file, offset, &mut byte).unwrap(), 1);
    byte[0] ^= 1;
    assert_eq!(fs.write_at(&file, offset, &byte).unwrap(), 1);
    let mut domain = Domain::default();
    assert!(
        recover_usage(
            &mut fs,
            recovery,
            base,
            state.clone(),
            lookup(),
            &mut domain
        )
        .is_err()
    );
    assert!(!domain.finished);
    byte[0] ^= 1;
    assert_eq!(fs.write_at(&file, offset, &byte).unwrap(), 1);
    fs.restart().unwrap();
    let (recovery, base) = admit_usage(&mut fs, &name);
    assert!(
        recovery
            .load_index_root_manifests(&mut fs, USAGE)
            .unwrap()
            .iter()
            .all(|r| r.revision() == CommitRevision::FIRST)
    );
    let (disk, _) = recover_usage(
        &mut fs,
        recovery,
        base,
        state,
        lookup(),
        &mut Domain::default(),
    )
    .unwrap();
    compare(&disk, &mut fs, references, [1, 2], 4);
}

#[test]
fn streamed_quota_all_recovery_and_publication_faults_preserve_restart() {
    for publication in [false, true] {
        let (mut fs, name, state, _) = published_fixture();
        let (recovery, base) = admit_usage(&mut fs, &name);
        if publication {
            let (mut disk, _) = recover_usage(
                &mut fs,
                recovery,
                base,
                state,
                lookup(),
                &mut Domain::default(),
            )
            .unwrap();
            fs.arm(FaultPlan::default()).unwrap();
            publish(&mut disk, &mut fs).unwrap();
        } else {
            fs.arm(FaultPlan::default()).unwrap();
            drop(
                recover_usage(
                    &mut fs,
                    recovery,
                    base,
                    state,
                    lookup(),
                    &mut Domain::default(),
                )
                .unwrap(),
            );
        }
        let mut attempts = 0;
        let mut optional_no_crash = 0;
        for operation in [
            Operation::OpenExisting,
            Operation::Metadata,
            Operation::ReadAt,
            Operation::CreateNew,
            Operation::WriteAt,
            Operation::SetLen,
            Operation::SyncAll,
            Operation::SyncDirectory,
            Operation::RenameNoReplace,
            Operation::RemoveFile,
            Operation::SyncData,
        ] {
            for occurrence in 1..=fs.operation_count(operation) {
                for action in [
                    FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                    FaultAction::CrashBefore,
                    FaultAction::CrashAfter,
                ] {
                    let (mut trial, name, state, references) = published_fixture();
                    let (recovery, base) = admit_usage(&mut trial, &name);
                    let plan = FaultPlan::new([FaultPoint {
                        operation,
                        occurrence,
                        action,
                    }])
                    .unwrap();
                    let result = if publication {
                        let (mut disk, _) = recover_usage(
                            &mut trial,
                            recovery,
                            base,
                            state.clone(),
                            lookup(),
                            &mut Domain::default(),
                        )
                        .unwrap();
                        trial.arm(plan).unwrap();
                        publish(&mut disk, &mut trial)
                    } else {
                        trial.arm(plan).unwrap();
                        recover_usage(
                            &mut trial,
                            recovery,
                            base,
                            state.clone(),
                            lookup(),
                            &mut Domain::default(),
                        )
                        .map(|_| ())
                    };
                    if action == FaultAction::CrashAfter && !trial.is_crashed() {
                        assert!(matches!(
                            operation,
                            Operation::OpenExisting | Operation::RemoveFile
                        ));
                        result.unwrap();
                        optional_no_crash += 1;
                    } else {
                        assert!(
                            result.is_err(),
                            "publication={publication}/{operation:?}/{occurrence}/{action:?}"
                        );
                    }
                    assert_eq!(trial.pending_faults(), 0);
                    trial.restart().unwrap();
                    // Reconstruct private origin even if a terminal root subset became visible.
                    let (recovery, base) = prepare(&mut trial, &name, true, true, true);
                    for profile in [
                        USAGE,
                        FIRST,
                        COORDINATOR_METADATA_PROFILE_V1,
                        uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
                    ] {
                        assert!(
                            recovery
                                .load_index_root_manifests(&mut trial, profile)
                                .unwrap()
                                .iter()
                                .all(|r| r.revision().get() == 1
                                    || (publication && r.revision().get() == 4))
                        );
                    }
                    let (mut disk, _) = recover_usage(
                        &mut trial,
                        recovery,
                        base,
                        state,
                        lookup(),
                        &mut Domain::default(),
                    )
                    .unwrap();
                    publish(&mut disk, &mut trial).unwrap();
                    compare(&disk, &mut trial, references, [1, 2], 4);
                    attempts += 1;
                }
            }
        }
        eprintln!(
            "quota faults publication={publication} attempts={attempts} optional_no_crash={optional_no_crash}"
        );
        assert!(attempts > 0);
    }
}

#[test]
fn quota_genesis_limits_and_all_staging_faults_leave_no_discoverable_projection() {
    let (mut fs, name, _, _) = fixture_options(true, false, 4);
    let mut recovery = open(&mut fs, &name);
    let genesis = recovery
        .recover_primary_genesis(&mut fs, CounterState::new(scope()), 1_000_000, 1)
        .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert!(matches!(
        uste_txn::stage_genesis_blob_usage(
            &mut recovery,
            &mut fs,
            &genesis,
            CoordinatorBlobUsageLimits {
                maximum_owners: 0,
                ..usage_limits()
            },
            metadata_rebase_limits().merge
        ),
        Err(TransactionError::ResourceLimit)
    ));
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    uste_txn::stage_genesis_blob_usage(
        &mut recovery,
        &mut fs,
        &genesis,
        usage_limits(),
        metadata_rebase_limits().merge,
    )
    .unwrap();
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
        Operation::RenameNoReplace,
        Operation::RemoveFile,
        Operation::SyncData,
    ] {
        for occurrence in 1..=fs.operation_count(operation) {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut trial, name, _, _) = fixture_options(true, false, 4);
                let mut recovery = open(&mut trial, &name);
                let genesis = recovery
                    .recover_primary_genesis(&mut trial, CounterState::new(scope()), 1_000_000, 1)
                    .unwrap();
                trial
                    .arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                assert!(
                    uste_txn::stage_genesis_blob_usage(
                        &mut recovery,
                        &mut trial,
                        &genesis,
                        usage_limits(),
                        metadata_rebase_limits().merge
                    )
                    .is_err()
                );
                assert_eq!(trial.pending_faults(), 0);
                drop(recovery);
                trial.restart().unwrap();
                let mut recovery = open(&mut trial, &name);
                assert!(
                    recovery
                        .load_index_root_manifests(&mut trial, USAGE)
                        .unwrap()
                        .is_empty()
                );
                let genesis = recovery
                    .recover_primary_genesis(&mut trial, CounterState::new(scope()), 1_000_000, 1)
                    .unwrap();
                uste_txn::stage_genesis_blob_usage(
                    &mut recovery,
                    &mut trial,
                    &genesis,
                    usage_limits(),
                    metadata_rebase_limits().merge,
                )
                .unwrap();
                attempts += 1;
            }
        }
    }
    eprintln!("quota genesis staging faults={attempts}");
    assert!(attempts > 0);
}
