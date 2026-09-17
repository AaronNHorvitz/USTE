use uste_storage::{
    AdapterErrorKind, Clock, ClockObservation, EntryName, FileSystem, RandomSource,
    fault::{
        FaultAction, FaultFileSystem, FaultPlan, FaultPoint, Operation, ScriptedClock,
        ScriptedRandom,
    },
    memory::MemoryFileSystem,
    read_exact_at, write_all_at,
};
use uste_types::UtcInstant;

const IO_VECTORS: &str = include_str!("../../../acceptance/r1/io-faults.tsv");

fn name(value: &str) -> EntryName {
    EntryName::new(value).unwrap()
}

#[test]
fn entry_names_are_single_bounded_components() {
    for rejected in ["", ".", "..", "a/b", "nul\0byte"] {
        assert!(EntryName::new(rejected).is_err(), "accepted {rejected:?}");
    }
    assert!(EntryName::new("x".repeat(255)).is_ok());
    assert!(EntryName::new("x".repeat(256)).is_err());
    assert_eq!(name("journal-0001").as_str(), "journal-0001");
}

#[test]
fn literal_fault_profile_names_every_exercised_boundary() {
    for expected in [
        "interrupted_write\twrite_at\tInterrupted\tretry_without_offset_advance",
        "short_write\twrite_at\tmaximum_2_then_3\tcomplete_exact_bytes",
        "short_read\tread_at\tInterrupted_then_maximum_2\tcomplete_exact_bytes",
        "over_report\tread_at_write_at\tprogress_above_buffer\tAdapterContract",
        "zero_progress\twrite_at\tOk_0\tZeroProgress",
        "disk_full\twrite_at\tNoSpace\tabort_without_retry",
        "flush_failure\tsync_data\tIo\tunsynchronized_bytes_lost_on_restart",
        "directory_flush_failure\tsync_directory\tIo\tunsynchronized_name_lost_on_restart",
        "rename_crash_before\trename_no_replace\tCrashBefore\told_name_and_bytes_after_restart",
        "rename_crash\trename_no_replace\tCrashAfter\told_name_after_restart_until_directory_sync",
        "flush_crash\tsync_data\tCrashAfter\tdurable_bytes_after_restart",
        "stale_handle\tmetadata\told_generation\tStaleHandle",
        "wall_rollback\tclock\t100_then_90\tpreserve_observations_without_commit_ordering",
        "random_failure\trandom_fill\tIo\toutput_not_authoritative",
        "process_kill\thost_test\tSIGKILL_after_file_and_directory_sync\texact_bytes_reopen",
    ] {
        assert!(IO_VECTORS.lines().any(|line| line == expected));
    }
    assert_eq!(IO_VECTORS.lines().count(), 16);
    assert!(
        FaultPlan::new([FaultPoint {
            operation: Operation::WriteAt,
            occurrence: 1,
            action: FaultAction::Error(AdapterErrorKind::InjectedCrash),
        }])
        .is_err()
    );
    for action in [
        FaultAction::ShortWrite { maximum: 1 },
        FaultAction::ShortRead { maximum: 1 },
        FaultAction::ZeroProgress,
        FaultAction::OverReport,
    ] {
        assert!(
            FaultPlan::new([FaultPoint {
                operation: Operation::SyncData,
                occurrence: 1,
                action,
            }])
            .is_err()
        );
    }
}

#[test]
fn checked_write_retries_interruption_and_short_progress_exactly() {
    let plan = FaultPlan::new([
        FaultPoint {
            operation: Operation::WriteAt,
            occurrence: 1,
            action: FaultAction::Error(AdapterErrorKind::Interrupted),
        },
        FaultPoint {
            operation: Operation::WriteAt,
            occurrence: 2,
            action: FaultAction::ShortWrite { maximum: 2 },
        },
        FaultPoint {
            operation: Operation::WriteAt,
            occurrence: 3,
            action: FaultAction::ShortWrite { maximum: 3 },
        },
        FaultPoint {
            operation: Operation::ReadAt,
            occurrence: 1,
            action: FaultAction::Error(AdapterErrorKind::Interrupted),
        },
        FaultPoint {
            operation: Operation::ReadAt,
            occurrence: 2,
            action: FaultAction::ShortRead { maximum: 2 },
        },
    ])
    .unwrap();
    let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
    let root = filesystem.root();
    let file = filesystem.create_new(&root, &name("group")).unwrap();
    filesystem.sync_directory(&root).unwrap();
    write_all_at(&mut filesystem, &file, 0, b"short writes are progress").unwrap();
    filesystem.sync_data(&file).unwrap();

    filesystem.restart().unwrap();
    let root = filesystem.root();
    let file = filesystem.open_existing(&root, &name("group")).unwrap();
    let mut recovered = vec![0; b"short writes are progress".len()];
    read_exact_at(&mut filesystem, &file, 0, &mut recovered).unwrap();
    assert_eq!(recovered, b"short writes are progress");
    assert_eq!(filesystem.pending_faults(), 0);
    let mut too_long = vec![0; recovered.len() + 1];
    assert_eq!(
        read_exact_at(&mut filesystem, &file, 0, &mut too_long)
            .unwrap_err()
            .kind(),
        AdapterErrorKind::UnexpectedEof
    );
}

#[test]
fn adapter_progress_overreport_is_rejected_for_reads_and_writes() {
    let mut inner = MemoryFileSystem::default();
    let root = inner.root();
    let file = inner.create_new(&root, &name("contract")).unwrap();
    write_all_at(&mut inner, &file, 0, b"bytes").unwrap();
    let plan = FaultPlan::new([
        FaultPoint {
            operation: Operation::WriteAt,
            occurrence: 1,
            action: FaultAction::OverReport,
        },
        FaultPoint {
            operation: Operation::ReadAt,
            occurrence: 1,
            action: FaultAction::OverReport,
        },
    ])
    .unwrap();
    let mut filesystem = FaultFileSystem::new(inner, plan);
    assert_eq!(
        write_all_at(&mut filesystem, &file, 0, b"x")
            .unwrap_err()
            .kind(),
        AdapterErrorKind::AdapterContract
    );
    let mut output = [0_u8; 1];
    assert_eq!(
        read_exact_at(&mut filesystem, &file, 0, &mut output)
            .unwrap_err()
            .kind(),
        AdapterErrorKind::AdapterContract
    );
}

#[test]
fn zero_progress_disk_full_and_flush_failure_fail_without_false_durability() {
    let plan = FaultPlan::new([
        FaultPoint {
            operation: Operation::WriteAt,
            occurrence: 1,
            action: FaultAction::ZeroProgress,
        },
        FaultPoint {
            operation: Operation::WriteAt,
            occurrence: 2,
            action: FaultAction::Error(AdapterErrorKind::NoSpace),
        },
        FaultPoint {
            operation: Operation::SyncData,
            occurrence: 1,
            action: FaultAction::Error(AdapterErrorKind::Io),
        },
    ])
    .unwrap();
    let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
    let root = filesystem.root();
    let file = filesystem.create_new(&root, &name("certificate")).unwrap();
    filesystem.sync_directory(&root).unwrap();

    assert_eq!(
        write_all_at(&mut filesystem, &file, 0, b"zero")
            .unwrap_err()
            .kind(),
        AdapterErrorKind::ZeroProgress
    );
    assert_eq!(
        write_all_at(&mut filesystem, &file, 0, b"full")
            .unwrap_err()
            .kind(),
        AdapterErrorKind::NoSpace
    );
    write_all_at(&mut filesystem, &file, 0, b"not durable").unwrap();
    assert_eq!(
        filesystem.sync_data(&file).unwrap_err().kind(),
        AdapterErrorKind::Io
    );

    filesystem.restart().unwrap();
    let root = filesystem.root();
    let file = filesystem
        .open_existing(&root, &name("certificate"))
        .unwrap();
    assert_eq!(filesystem.metadata(&file).unwrap().len, 0);
}

#[test]
fn rename_crash_requires_directory_flush_before_new_name_survives_restart() {
    let plan = FaultPlan::new([FaultPoint {
        operation: Operation::RenameNoReplace,
        occurrence: 1,
        action: FaultAction::CrashAfter,
    }])
    .unwrap();
    let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
    let root = filesystem.root();
    let staging = filesystem.create_new(&root, &name("staging")).unwrap();
    write_all_at(&mut filesystem, &staging, 0, b"published bytes").unwrap();
    filesystem.sync_data(&staging).unwrap();
    filesystem.sync_directory(&root).unwrap();

    assert_eq!(
        filesystem
            .rename_no_replace(&root, &name("staging"), &root, &name("final"))
            .unwrap_err()
            .kind(),
        AdapterErrorKind::InjectedCrash
    );
    assert!(filesystem.is_crashed());
    assert_eq!(
        filesystem
            .open_existing(&root, &name("staging"))
            .unwrap_err()
            .kind(),
        AdapterErrorKind::InjectedCrash
    );
    filesystem.restart().unwrap();

    let root = filesystem.root();
    assert!(filesystem.open_existing(&root, &name("staging")).is_ok());
    assert_eq!(
        filesystem
            .open_existing(&root, &name("final"))
            .unwrap_err()
            .kind(),
        AdapterErrorKind::NotFound
    );

    filesystem
        .rename_no_replace(&root, &name("staging"), &root, &name("final"))
        .unwrap();
    filesystem.sync_directory(&root).unwrap();
    filesystem.restart().unwrap();
    let root = filesystem.root();
    let final_file = filesystem.open_existing(&root, &name("final")).unwrap();
    let mut bytes = vec![0; b"published bytes".len()];
    read_exact_at(&mut filesystem, &final_file, 0, &mut bytes).unwrap();
    assert_eq!(bytes, b"published bytes");
    assert_eq!(filesystem.pending_faults(), 0);
}

#[test]
fn failed_directory_sync_and_crash_before_rename_publish_no_name_change() {
    let directory_plan = FaultPlan::new([FaultPoint {
        operation: Operation::SyncDirectory,
        occurrence: 1,
        action: FaultAction::Error(AdapterErrorKind::Io),
    }])
    .unwrap();
    let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), directory_plan);
    let root = filesystem.root();
    let file = filesystem.create_new(&root, &name("unpublished")).unwrap();
    write_all_at(&mut filesystem, &file, 0, b"orphan").unwrap();
    filesystem.sync_data(&file).unwrap();
    assert_eq!(
        filesystem.sync_directory(&root).unwrap_err().kind(),
        AdapterErrorKind::Io
    );
    filesystem.restart().unwrap();
    let root = filesystem.root();
    assert_eq!(
        filesystem
            .open_existing(&root, &name("unpublished"))
            .unwrap_err()
            .kind(),
        AdapterErrorKind::NotFound
    );

    let rename_plan = FaultPlan::new([FaultPoint {
        operation: Operation::RenameNoReplace,
        occurrence: 1,
        action: FaultAction::CrashBefore,
    }])
    .unwrap();
    let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), rename_plan);
    let root = filesystem.root();
    let file = filesystem.create_new(&root, &name("old")).unwrap();
    write_all_at(&mut filesystem, &file, 0, b"old bytes").unwrap();
    filesystem.sync_data(&file).unwrap();
    filesystem.sync_directory(&root).unwrap();
    assert_eq!(
        filesystem
            .rename_no_replace(&root, &name("old"), &root, &name("new"))
            .unwrap_err()
            .kind(),
        AdapterErrorKind::InjectedCrash
    );
    assert_eq!(
        filesystem.metadata(&file).unwrap_err().kind(),
        AdapterErrorKind::InjectedCrash
    );
    filesystem.restart().unwrap();
    let root = filesystem.root();
    assert!(filesystem.open_existing(&root, &name("old")).is_ok());
    assert_eq!(
        filesystem
            .open_existing(&root, &name("new"))
            .unwrap_err()
            .kind(),
        AdapterErrorKind::NotFound
    );
}

#[test]
fn crash_after_data_flush_preserves_bytes_but_invalidates_old_handles() {
    let plan = FaultPlan::new([FaultPoint {
        operation: Operation::SyncData,
        occurrence: 1,
        action: FaultAction::CrashAfter,
    }])
    .unwrap();
    let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
    let root = filesystem.root();
    let stale_file = filesystem.create_new(&root, &name("segment")).unwrap();
    filesystem.sync_directory(&root).unwrap();
    write_all_at(&mut filesystem, &stale_file, 0, b"durable group").unwrap();
    assert_eq!(
        filesystem.sync_data(&stale_file).unwrap_err().kind(),
        AdapterErrorKind::InjectedCrash
    );
    filesystem.restart().unwrap();
    assert_eq!(
        filesystem.metadata(&stale_file).unwrap_err().kind(),
        AdapterErrorKind::StaleHandle
    );
    let root = filesystem.root();
    let file = filesystem.open_existing(&root, &name("segment")).unwrap();
    let mut output = vec![0; b"durable group".len()];
    read_exact_at(&mut filesystem, &file, 0, &mut output).unwrap();
    assert_eq!(output, b"durable group");
}

#[test]
fn clock_rollback_and_random_failures_remain_explicit() {
    let newer = ClockObservation {
        wall_utc: UtcInstant::new(100, 0).unwrap(),
        monotonic_ticks: 8,
    };
    let rolled_back = ClockObservation {
        wall_utc: UtcInstant::new(90, 0).unwrap(),
        monotonic_ticks: 9,
    };
    let mut clock = ScriptedClock::new([Ok(newer), Ok(rolled_back), Err(AdapterErrorKind::Io)]);
    assert_eq!(clock.observe().unwrap(), newer);
    assert_eq!(clock.observe().unwrap(), rolled_back);
    assert_eq!(clock.observe().unwrap_err().kind(), AdapterErrorKind::Io);
    assert_eq!(
        clock.observe().unwrap_err().kind(),
        AdapterErrorKind::ScriptExhausted
    );

    let mut random =
        ScriptedRandom::new([Ok(vec![1, 2, 3, 4]), Err(AdapterErrorKind::Io), Ok(vec![9])]);
    let mut output = [0; 4];
    random.fill(&mut output).unwrap();
    assert_eq!(output, [1, 2, 3, 4]);
    assert_eq!(
        random.fill(&mut output).unwrap_err().kind(),
        AdapterErrorKind::Io
    );
    assert_eq!(
        random.fill(&mut output).unwrap_err().kind(),
        AdapterErrorKind::AdapterContract
    );
}

#[test]
fn the_same_fault_plan_produces_the_same_transcript() {
    fn run() -> Vec<AdapterErrorKind> {
        let plan = FaultPlan::new([
            FaultPoint {
                operation: Operation::WriteAt,
                occurrence: 1,
                action: FaultAction::Error(AdapterErrorKind::Interrupted),
            },
            FaultPoint {
                operation: Operation::WriteAt,
                occurrence: 2,
                action: FaultAction::ZeroProgress,
            },
            FaultPoint {
                operation: Operation::SyncAll,
                occurrence: 1,
                action: FaultAction::Error(AdapterErrorKind::QuotaExceeded),
            },
        ])
        .unwrap();
        let mut filesystem = FaultFileSystem::new(MemoryFileSystem::default(), plan);
        let root = filesystem.root();
        let file = filesystem.create_new(&root, &name("trace")).unwrap();
        let first = filesystem.write_at(&file, 0, b"x").unwrap_err().kind();
        let second = write_all_at(&mut filesystem, &file, 0, b"x")
            .unwrap_err()
            .kind();
        let third = filesystem.sync_all(&file).unwrap_err().kind();
        vec![first, second, third]
    }

    assert_eq!(run(), run());
    assert_eq!(
        run(),
        vec![
            AdapterErrorKind::Interrupted,
            AdapterErrorKind::ZeroProgress,
            AdapterErrorKind::QuotaExceeded,
        ]
    );
}
