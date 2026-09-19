use super::*;
use uste_txn::COORDINATOR_FIRST_REFERENCE_PROFILE_V1 as FIRST;

fn prepare(
    fs: &mut Fs,
    name: &EntryName,
    initial_owner: bool,
    origin: bool,
) -> (Recovery, CoordinatorDiskBase) {
    if !origin {
        return admit_mode(fs, name, CommitRevision::FIRST, initial_owner);
    }
    let mut recovery = open(fs, name);
    let genesis = recovery
        .recover_primary_genesis(fs, CounterState::new(scope()), 1_000_000, 1)
        .unwrap();
    let (metadata, transactions) = uste_txn::stage_primary_genesis_metadata(
        &mut recovery,
        fs,
        &genesis,
        1,
        metadata_rebase_limits().merge,
    )
    .unwrap();
    let first = uste_txn::stage_genesis_first_references(
        &mut recovery,
        fs,
        &genesis,
        1,
        metadata_rebase_limits().merge,
    )
    .unwrap();
    assert_eq!(first.is_some(), initial_owner);
    let base = admit_roots_with_first(fs, &recovery, transactions, metadata, first);
    (recovery, base)
}
fn recover_first(
    fs: &mut Fs,
    recovery: Recovery,
    base: CoordinatorDiskBase,
    state: Anchored,
    domain: &mut Domain,
) -> Result<(Disk, PrimaryMetadataRecoveryReport), TransactionError> {
    Disk::recover_with_first_reference_streaming_domain(
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
        &mut cache(),
        domain,
    )
}
fn publish(disk: &mut Disk, fs: &mut Fs) {
    disk.rebase_metadata_with_first_references(
        fs,
        metadata_rebase_limits(),
        uste_txn::CoordinatorFirstReferenceLimits {
            maximum_owners: 0,
            maximum_groups: 0,
            maximum_encoded_bytes: 0,
        },
    )
    .unwrap();
    assert!(!disk.rebase_required());
}
fn assert_first(
    disk: &Disk,
    fs: &mut Fs,
    references: [BlobReference; 2],
    first: [u64; 2],
    last: u8,
) {
    let roots = disk.load_index_root_manifests(fs, FIRST).unwrap();
    let root = roots.iter().find(|r| r.revision().get() == u64::from(last));
    if last == 1 && first[0] == 2 {
        assert!(root.is_none());
        return;
    }
    let root = root.unwrap();
    for (reference, revision) in references.into_iter().zip(first) {
        let value = disk
            .index_get_bounded(
                fs,
                root,
                1,
                &reference.id().as_bytes(),
                lookup(),
                &mut cache(),
            )
            .unwrap()
            .0;
        assert_eq!(
            value,
            (revision <= u64::from(last)).then(|| revision.to_be_bytes().to_vec())
        );
    }
}

#[test]
fn streamed_first_references_preserve_earliest_claims_and_bootstrap_empty_or_origin_bases() {
    for initial in [false, true] {
        for origin in [false, true] {
            for last in [1, 4] {
                let (mut fs, name, state, references) =
                    fixture_projection(initial, !origin, last, true);
                let (recovery, base) = prepare(&mut fs, &name, initial, origin);
                let (mut disk, report) =
                    recover_first(&mut fs, recovery, base, state, &mut Domain::default()).unwrap();
                assert_eq!(disk.overlay_counts(), (0, 0));
                assert_eq!(report.revisions, u64::from(last - 1));
                assert_eq!(report.staged_runs, u64::from(last - 1) * 5);
                if last == 4 {
                    assert_eq!(report.output_entries, 33);
                    assert_eq!(report.output_logical_bytes, 3657);
                    assert_terminal_owners(
                        &disk,
                        &mut fs,
                        references,
                        if initial { [1, 2] } else { [2, 2] },
                    );
                    assert!(matches!(
                        disk.rebase_metadata(&mut fs, metadata_rebase_limits()),
                        Err(TransactionError::InvalidRequest)
                    ));
                }
                publish(&mut disk, &mut fs);
                assert_first(
                    &disk,
                    &mut fs,
                    references,
                    if initial { [1, 2] } else { [2, 2] },
                    last,
                );
                let state = disk.state().unwrap().clone();
                drop(disk);
                fs.restart().unwrap();
                let (recovery, base) = admit_mode(
                    &mut fs,
                    &name,
                    CommitRevision::new(u64::from(last)).unwrap(),
                    initial || last > 1,
                );
                let (disk, report) =
                    recover_first(&mut fs, recovery, base, state, &mut Domain::default()).unwrap();
                assert_eq!(report, PrimaryMetadataRecoveryReport::default());
                assert_first(
                    &disk,
                    &mut fs,
                    references,
                    if initial { [1, 2] } else { [2, 2] },
                    last,
                );
            }
        }
    }
}

#[test]
fn streamed_first_references_refuse_populated_unwitnessed_base_before_io() {
    let (mut fs, name, state, _) = fixture();
    let (recovery, base) = admit(&mut fs, &name, CommitRevision::FIRST);
    fs.arm(FaultPlan::default()).unwrap();
    let mut domain = Domain::default();
    assert!(matches!(
        recover_first(&mut fs, recovery, base, state, &mut domain),
        Err(TransactionError::InvalidRequest)
    ));
    assert_eq!((domain.advanced, domain.finished), (0, false));
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
}

#[test]
fn streamed_first_references_every_io_fault_preserves_old_witness_and_restart() {
    let (mut fs, name, state, _) = fixture_projection(true, true, 4, true);
    let (recovery, base) = prepare(&mut fs, &name, true, false);
    fs.arm(FaultPlan::default()).unwrap();
    drop(recover_first(&mut fs, recovery, base, state, &mut Domain::default()).unwrap());
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
                let (mut trial, name, state, references) = fixture_projection(true, true, 4, true);
                let (recovery, base) = prepare(&mut trial, &name, true, false);
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
                let result = recover_first(
                    &mut trial,
                    recovery,
                    base,
                    state.clone(),
                    &mut Domain::default(),
                );
                if action == FaultAction::CrashAfter && !trial.is_crashed() {
                    assert!(matches!(
                        operation,
                        Operation::OpenExisting | Operation::RemoveFile
                    ));
                    assert_terminal(&result.unwrap().0, &mut trial, references);
                    optional_no_crash += 1;
                } else {
                    assert!(result.is_err(), "{operation:?}/{occurrence}/{action:?}");
                }
                assert_eq!(trial.pending_faults(), 0);
                trial.restart().unwrap();
                let (recovery, base) = prepare(&mut trial, &name, true, false);
                for profile in [
                    FIRST,
                    COORDINATOR_METADATA_PROFILE_V1,
                    uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
                ] {
                    assert!(
                        recovery
                            .load_index_root_manifests(&mut trial, profile)
                            .unwrap()
                            .iter()
                            .all(|root| root.revision() == CommitRevision::FIRST)
                    );
                }
                let (mut disk, _) =
                    recover_first(&mut trial, recovery, base, state, &mut Domain::default())
                        .unwrap();
                assert_terminal(&disk, &mut trial, references);
                publish(&mut disk, &mut trial);
                assert_first(&disk, &mut trial, references, [1, 2], 4);
                attempts += 1;
            }
        }
    }
    eprintln!("first-reference streamed faults={attempts} optional_no_crash={optional_no_crash}");
    assert!(attempts > 0);
}

#[test]
fn first_reference_genesis_limits_and_staging_faults_leave_no_discoverable_witness() {
    let (mut fs, name, _, _) = fixture_options(true, false, 4);
    let mut recovery = open(&mut fs, &name);
    let genesis = recovery
        .recover_primary_genesis(&mut fs, CounterState::new(scope()), 1_000_000, 1)
        .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert!(matches!(
        uste_txn::stage_genesis_first_references(
            &mut recovery,
            &mut fs,
            &genesis,
            0,
            metadata_rebase_limits().merge
        ),
        Err(TransactionError::ResourceLimit)
    ));
    assert_eq!(fs.operation_count(Operation::ReadAt), 0);
    assert_eq!(fs.operation_count(Operation::CreateNew), 0);
    uste_txn::stage_genesis_first_references(
        &mut recovery,
        &mut fs,
        &genesis,
        1,
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
                    uste_txn::stage_genesis_first_references(
                        &mut recovery,
                        &mut trial,
                        &genesis,
                        1,
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
                        .load_index_root_manifests(&mut trial, FIRST)
                        .unwrap()
                        .is_empty()
                );
                let genesis = recovery
                    .recover_primary_genesis(&mut trial, CounterState::new(scope()), 1_000_000, 1)
                    .unwrap();
                assert!(
                    uste_txn::stage_genesis_first_references(
                        &mut recovery,
                        &mut trial,
                        &genesis,
                        1,
                        metadata_rebase_limits().merge
                    )
                    .unwrap()
                    .is_some()
                );
                attempts += 1;
            }
        }
    }
    eprintln!("first-reference genesis faults={attempts}");
    assert!(attempts > 0);
}

#[test]
fn streamed_first_reference_terminal_publication_faults_preserve_rebuildable_authority() {
    fn ready() -> (Fs, EntryName, Disk, Anchored, [BlobReference; 2]) {
        let (mut fs, name, state, references) = fixture_projection(true, true, 4, true);
        let (recovery, base) = prepare(&mut fs, &name, true, false);
        let (disk, _) = recover_first(
            &mut fs,
            recovery,
            base,
            state.clone(),
            &mut Domain::default(),
        )
        .unwrap();
        (fs, name, disk, state, references)
    }
    fn rebase(disk: &mut Disk, fs: &mut Fs) -> Result<(), TransactionError> {
        disk.rebase_metadata_with_first_references(
            fs,
            metadata_rebase_limits(),
            uste_txn::CoordinatorFirstReferenceLimits {
                maximum_owners: 0,
                maximum_groups: 0,
                maximum_encoded_bytes: 0,
            },
        )
    }
    let (mut fs, _, mut disk, _, _) = ready();
    fs.arm(FaultPlan::default()).unwrap();
    rebase(&mut disk, &mut fs).unwrap();
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
                let (mut trial, name, mut disk, state, references) = ready();
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
                let result = rebase(&mut disk, &mut trial);
                if action == FaultAction::CrashAfter && !trial.is_crashed() {
                    assert!(matches!(
                        operation,
                        Operation::OpenExisting | Operation::RemoveFile
                    ));
                    result.unwrap();
                    optional_no_crash += 1;
                } else {
                    assert!(result.is_err(), "{operation:?}/{occurrence}/{action:?}");
                }
                assert_eq!(trial.pending_faults(), 0);
                drop(disk);
                trial.restart().unwrap();
                let (recovery, base) = prepare(&mut trial, &name, true, true);
                for profile in [
                    FIRST,
                    COORDINATOR_METADATA_PROFILE_V1,
                    uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
                ] {
                    assert!(
                        recovery
                            .load_index_root_manifests(&mut trial, profile)
                            .unwrap()
                            .iter()
                            .all(|root| [1, 4].contains(&root.revision().get()))
                    );
                }
                let (mut disk, _) =
                    recover_first(&mut trial, recovery, base, state, &mut Domain::default())
                        .unwrap();
                publish(&mut disk, &mut trial);
                assert_terminal(&disk, &mut trial, references);
                assert_first(&disk, &mut trial, references, [1, 2], 4);
                attempts += 1;
            }
        }
    }
    eprintln!(
        "first-reference terminal publication faults={attempts} optional_no_crash={optional_no_crash}"
    );
    assert!(attempts > 0);
}
