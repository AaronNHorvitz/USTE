//! Fixed phase partitions of adapter observations, not complete authenticated/device I/O.
use super::*;
use crate::linux_runner::disk::io::IoSnapshot;
use std::time::Duration;

pub(super) fn report(
    elapsed: [Duration; 4],
    snapshots: [IoSnapshot; 4],
) -> Result<serde_json::Value, LinuxRunnerError> {
    let labels = [
        "setup_and_admission",
        "history_verification",
        "post_verification_tail_or_retry",
        "terminal_digest_admission",
    ];
    let mut stages = serde_json::Map::new();
    let mut previous_time = Duration::ZERO;
    let mut previous_io = IoSnapshot::default();
    let mut total_io = IoSnapshot::default();
    for ((label, time), snapshot) in labels.into_iter().zip(elapsed).zip(snapshots) {
        let duration = time
            .checked_sub(previous_time)
            .ok_or_else(|| error("USTE_BM06_PHASE_CLOCK"))?;
        let io = snapshot.delta(previous_io)?;
        total_io.accumulate(io)?;
        stages.insert(
            label.into(),
            serde_json::json!({
                "elapsed_microseconds": duration.as_micros(), "adapter_io": io.json()?,
            }),
        );
        previous_time = time;
        previous_io = snapshot;
    }
    if total_io != snapshots[3] {
        return Err(error("USTE_BM06_PHASE_IO"));
    }
    Ok(serde_json::json!({
        "measurement_scope": "sequential-command-phases-through-terminal-digest",
        "complete_authenticated_io": false, "physical_device_io": false,
        "qualifying_recovery_latency": false,
        "measured_elapsed_microseconds": elapsed[3].as_micros(),
        "stages": stages,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uste_storage::{FileSystem, memory::MemoryFileSystem};

    #[test]
    fn phase_work_partitions_real_adapter_calls_and_refuses_clock_or_counter_rewind() {
        let mut fs = ObservedFileSystem::new(MemoryFileSystem::default());
        let file = fs
            .create_new(&fs.root(), &EntryName::new("synthetic").unwrap())
            .unwrap();
        let first = fs.snapshot().unwrap();
        fs.write_at(&file, 0, b"ab").unwrap();
        let second = fs.snapshot().unwrap();
        fs.read_at(&file, 0, &mut [0; 2]).unwrap();
        let third = fs.snapshot().unwrap();
        fs.sync_all(&file).unwrap();
        let fourth = fs.snapshot().unwrap();
        let times = [1, 3, 6, 10].map(Duration::from_micros);
        let snapshots = [first, second, third, fourth];
        let measured = report(times, snapshots).unwrap();
        let stages = &measured["stages"];
        for (name, duration) in [
            ("setup_and_admission", 1),
            ("history_verification", 2),
            ("post_verification_tail_or_retry", 3),
            ("terminal_digest_admission", 4),
        ] {
            assert_eq!(stages[name]["elapsed_microseconds"], duration);
        }
        assert_eq!(
            stages["history_verification"]["adapter_io"]["write_returned_bytes"],
            2
        );
        assert_eq!(
            stages["post_verification_tail_or_retry"]["adapter_io"]["read_returned_bytes"],
            2
        );
        assert_eq!(
            stages["terminal_digest_admission"]["adapter_io"]["operations"]["sync_all"]["calls"],
            1
        );
        assert_eq!(measured["qualifying_recovery_latency"], false);
        assert!(!measured.to_string().contains("synthetic"));
        assert!(report([1, 0, 6, 10].map(Duration::from_micros), snapshots).is_err());
        assert!(report(times, [second, first, third, fourth]).is_err());
        assert!(report([Duration::ZERO; 4], [IoSnapshot::default(); 4]).is_ok());
    }
}
