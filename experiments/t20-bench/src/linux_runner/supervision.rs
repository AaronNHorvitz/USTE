//! Parent-side deadline supervision for the BM-01 sampling worker.

use std::{
    io::{self, BufRead, BufReader},
    path::Path,
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

use crate::Bm01Profile;

use super::LinuxRunnerError;
mod lookup;

const QUERY_DEADLINE: Duration = Duration::from_secs(30);
const WORKER_EXIT_DEADLINE: Duration = Duration::from_secs(5);
const MAX_PROTOCOL_LINE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Eq, PartialEq)]
enum WorkerMessage {
    QueryStarted,
    QueryFinished,
    Report(String),
    Error,
    Invalid,
    End,
}

#[derive(Clone, Copy)]
enum SampleMode {
    Legacy,
    Disk,
    Packed,
    PackedLookup,
    PackedRange,
    PackedWide,
    PackedWideLookup,
    PackedWideRange,
}

impl SampleMode {
    fn packed(self) -> bool {
        matches!(
            self,
            Self::Packed
                | Self::PackedLookup
                | Self::PackedRange
                | Self::PackedWide
                | Self::PackedWideLookup
                | Self::PackedWideRange
        )
    }
}

pub fn supervise_packed_wide_sample(
    executable: &Path,
    root: &Path,
    password_file: &Path,
    bundle_file: &Path,
    profile: Bm01Profile,
) -> Result<String, LinuxRunnerError> {
    super::disk::validate_native_profile(profile)?;
    supervise_mode(
        executable,
        root,
        password_file,
        bundle_file,
        profile,
        SampleMode::PackedWide,
    )
}
pub fn supervise_packed_wide_lookup_sample(
    executable: &Path,
    root: &Path,
    password_file: &Path,
    bundle_file: &Path,
    profile: Bm01Profile,
) -> Result<String, LinuxRunnerError> {
    super::disk::validate_native_profile(profile)?;
    supervise_mode(
        executable,
        root,
        password_file,
        bundle_file,
        profile,
        SampleMode::PackedWideLookup,
    )
}
pub fn supervise_packed_wide_range_sample(
    executable: &Path,
    root: &Path,
    password_file: &Path,
    bundle_file: &Path,
    profile: Bm01Profile,
) -> Result<String, LinuxRunnerError> {
    super::disk::validate_native_profile(profile)?;
    supervise_mode(
        executable,
        root,
        password_file,
        bundle_file,
        profile,
        SampleMode::PackedWideRange,
    )
}

pub fn supervise_packed_lookup_sample(
    executable: &Path,
    root: &Path,
    password_file: &Path,
    bundle_file: &Path,
    profile: Bm01Profile,
) -> Result<String, LinuxRunnerError> {
    super::disk::validate_native_profile(profile)?;
    supervise_mode(
        executable,
        root,
        password_file,
        bundle_file,
        profile,
        SampleMode::PackedLookup,
    )
}
pub fn supervise_packed_range_sample(
    executable: &Path,
    root: &Path,
    password_file: &Path,
    bundle_file: &Path,
    profile: Bm01Profile,
) -> Result<String, LinuxRunnerError> {
    super::disk::validate_native_profile(profile)?;
    supervise_mode(
        executable,
        root,
        password_file,
        bundle_file,
        profile,
        SampleMode::PackedRange,
    )
}

pub fn supervise_packed_sample(
    executable: &Path,
    root: &Path,
    password_file: &Path,
    bundle_file: &Path,
    profile: Bm01Profile,
) -> Result<String, LinuxRunnerError> {
    super::disk::validate_native_profile(profile)?;
    supervise_mode(
        executable,
        root,
        password_file,
        bundle_file,
        profile,
        SampleMode::Packed,
    )
}

pub fn supervise_sample(
    executable: &Path,
    root: &Path,
    password_file: &Path,
    bundle_file: &Path,
    profile: Bm01Profile,
) -> Result<String, LinuxRunnerError> {
    supervise_mode(
        executable,
        root,
        password_file,
        bundle_file,
        profile,
        SampleMode::Legacy,
    )
}

pub fn supervise_disk_sample(
    executable: &Path,
    root: &Path,
    password_file: &Path,
    bundle_file: &Path,
    profile: Bm01Profile,
) -> Result<String, LinuxRunnerError> {
    super::disk::validate_native_profile(profile)?;
    supervise_mode(
        executable,
        root,
        password_file,
        bundle_file,
        profile,
        SampleMode::Disk,
    )
}

fn supervise_mode(
    executable: &Path,
    root: &Path,
    password_file: &Path,
    bundle_file: &Path,
    profile: Bm01Profile,
    mode: SampleMode,
) -> Result<String, LinuxRunnerError> {
    let child = Command::new(executable)
        .arg(match mode {
            SampleMode::Legacy => "linux-sample-worker",
            SampleMode::Disk => "linux-disk-sample-worker",
            SampleMode::Packed => "linux-packed-sample-worker",
            SampleMode::PackedLookup => "linux-packed-lookup-sample-worker",
            SampleMode::PackedRange => "linux-packed-range-sample-worker",
            SampleMode::PackedWide => "linux-packed-wide-sample-worker",
            SampleMode::PackedWideLookup => "linux-packed-wide-lookup-sample-worker",
            SampleMode::PackedWideRange => "linux-packed-wide-range-sample-worker",
        })
        .arg("--root")
        .arg(root)
        .arg("--password-file")
        .arg(password_file)
        .arg("--oracle-file")
        .arg(bundle_file)
        .arg("--entities")
        .arg(profile.entities().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_SAMPLE_SPAWN"))?;
    let mut worker = WorkerGuard::new(child)?;
    let output = match worker.take_stdout() {
        Ok(output) => output,
        Err(_) => return fail_after_termination(&mut worker, "USTE_BM01_SAMPLE_PROTOCOL"),
    };
    let (sender, receiver) = mpsc::sync_channel(16);
    if thread::Builder::new()
        .name("bm01-sample-protocol".into())
        .spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let message = match read_protocol_line(&mut reader) {
                    Ok(Some(line)) => parse_message(&line),
                    Ok(None) => break,
                    Err(_) => WorkerMessage::Invalid,
                };
                let terminal = matches!(
                    message,
                    WorkerMessage::Report(_)
                        | WorkerMessage::Error
                        | WorkerMessage::Invalid
                        | WorkerMessage::End
                );
                if sender.send(message).is_err() || terminal {
                    return;
                }
            }
            let _ = sender.send(WorkerMessage::End);
        })
        .is_err()
    {
        worker.terminate()?;
        return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_SPAWN"));
    }
    supervise_protocol(&mut worker, &receiver, profile, mode)
}

fn supervise_protocol(
    worker: &mut WorkerGuard,
    receiver: &Receiver<WorkerMessage>,
    profile: Bm01Profile,
    mode: SampleMode,
) -> Result<String, LinuxRunnerError> {
    supervise_protocol_with_deadline(worker, receiver, profile, QUERY_DEADLINE, mode)
}

fn supervise_protocol_with_deadline(
    worker: &mut WorkerGuard,
    receiver: &Receiver<WorkerMessage>,
    profile: Bm01Profile,
    query_deadline: Duration,
    mode: SampleMode,
) -> Result<String, LinuxRunnerError> {
    let mut completed_queries = 0_u64;
    loop {
        let message = match receiver.recv() {
            Ok(message) => message,
            Err(_) => {
                return fail_after_termination(worker, "USTE_BM01_SAMPLE_PROTOCOL");
            }
        };
        match message {
            WorkerMessage::QueryStarted => match receiver.recv_timeout(query_deadline) {
                Ok(WorkerMessage::QueryFinished) => {}
                Err(RecvTimeoutError::Timeout) => {
                    return fail_after_termination(worker, "USTE_BM01_QUERY_DEADLINE");
                }
                _ => {
                    return fail_after_termination(worker, "USTE_BM01_SAMPLE_PROTOCOL");
                }
            },
            WorkerMessage::Report(report) => {
                let report = match finalize_report(&report, profile, completed_queries, mode) {
                    Ok(report) => report,
                    Err(_) => {
                        return fail_after_termination(worker, "USTE_BM01_SAMPLE_PROTOCOL");
                    }
                };
                let status = worker.wait_for_exit()?;
                if !status.success() {
                    return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_WORKER"));
                }
                return Ok(report);
            }
            WorkerMessage::Error | WorkerMessage::Invalid | WorkerMessage::End => {
                return fail_after_termination(worker, "USTE_BM01_SAMPLE_WORKER");
            }
            WorkerMessage::QueryFinished => {
                return fail_after_termination(worker, "USTE_BM01_SAMPLE_PROTOCOL");
            }
        }
        completed_queries = completed_queries
            .checked_add(1)
            .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"))?;
    }
}

struct WorkerGuard {
    child: Child,
    lifetime: Option<ChildStdin>,
    reaped: bool,
}

impl WorkerGuard {
    fn new(mut child: Child) -> Result<Self, LinuxRunnerError> {
        let Some(lifetime) = child.stdin.take() else {
            child
                .kill()
                .map_err(|_| LinuxRunnerError::new("USTE_BM01_SAMPLE_CLEANUP"))?;
            child
                .wait()
                .map_err(|_| LinuxRunnerError::new("USTE_BM01_SAMPLE_CLEANUP"))?;
            return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"));
        };
        Ok(Self {
            child,
            lifetime: Some(lifetime),
            reaped: false,
        })
    }

    fn take_stdout(&mut self) -> Result<std::process::ChildStdout, LinuxRunnerError> {
        self.child
            .stdout
            .take()
            .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"))
    }

    fn terminate(&mut self) -> Result<(), LinuxRunnerError> {
        self.lifetime.take();
        if let Ok(Some(_)) = self.child.try_wait() {
            self.reaped = true;
            return Ok(());
        }
        if self.child.kill().is_err() {
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    self.reaped = true;
                    return Ok(());
                }
                Ok(None) | Err(_) => {
                    return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_CLEANUP"));
                }
            }
        }
        self.child
            .wait()
            .map_err(|_| LinuxRunnerError::new("USTE_BM01_SAMPLE_CLEANUP"))?;
        self.reaped = true;
        Ok(())
    }

    fn wait_for_exit(&mut self) -> Result<ExitStatus, LinuxRunnerError> {
        let started = Instant::now();
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.reaped = true;
                    self.lifetime.take();
                    return Ok(status);
                }
                Ok(None) => {}
                Err(_) => {
                    self.terminate()?;
                    return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_WORKER"));
                }
            }
            if started.elapsed() >= WORKER_EXIT_DEADLINE {
                self.terminate()?;
                return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_WORKER"));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.terminate();
        }
    }
}

fn fail_after_termination<T>(
    worker: &mut WorkerGuard,
    code: &'static str,
) -> Result<T, LinuxRunnerError> {
    worker.terminate()?;
    Err(LinuxRunnerError::new(code))
}

fn finalize_report(
    report: &str,
    profile: Bm01Profile,
    completed_queries: u64,
    mode: SampleMode,
) -> Result<String, LinuxRunnerError> {
    let value: serde_json::Value = serde_json::from_str(report)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"))?;
    let object = value
        .as_object()
        .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"))?;
    expect_string(
        object,
        "schema",
        match mode {
            SampleMode::Legacy => "bm01-linux-sampling-v1",
            SampleMode::Disk => "bm01-linux-disk-sampling-v1",
            SampleMode::Packed => "bm01-linux-packed-sampling-v1",
            SampleMode::PackedLookup => "bm01-linux-packed-lookup-sampling-v1",
            SampleMode::PackedRange => "bm01-linux-packed-range-sampling-v1",
            SampleMode::PackedWide => "bm01-linux-packed-wide-sampling-v1",
            SampleMode::PackedWideLookup => "bm01-linux-packed-wide-lookup-sampling-v1",
            SampleMode::PackedWideRange => "bm01-linux-packed-wide-range-sampling-v1",
        },
    )?;
    if mode.packed() {
        super::disk::validate_native_profile(profile)?;
        expect_bool(object, "engine_benchmark", true)?;
        expect_bool(object, "full_memory_graph_state", false)?;
        expect_bool(object, "full_memory_coordinator_metadata", false)?;
        expect_bool(object, "complete_authenticated_io", false)?;
        expect_string(
            object,
            "storage_metadata_mode",
            "disk-certificate-and-blob-recovery",
        )?;
        expect_string(
            object,
            "authenticated_io_accounting",
            "partial-single-owner-vault-decrypt",
        )?;
        expect_string(
            object,
            "setup_vault_work_scope",
            "last-cold-open-owner-only",
        )?;
        expect_vault_work(object.get("setup_vault_work"))?;
        expect_vault_work(object.get("warmup_vault_work"))?;
        expect_string(
            object,
            "qualification",
            "nonqualifying-development-sampling",
        )?;
        expect_string(object, "budget_evaluation", "not-performed")?;
        expect_u64(
            object,
            "frontier",
            crate::engine::materialization_revision_count(profile),
        )?;
        let setup = object
            .get("setup")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"))?;
        expect_bool(setup, "complete_fixture", true)?;
        expect_u64(
            setup,
            "graph_admission_cache_bytes",
            crate::engine::packed::GRAPH_ADMISSION_CACHE_BYTES as u64,
        )?;
        expect_string(
            setup,
            "graph_admission_cache_scope",
            crate::engine::packed::GRAPH_ADMISSION_CACHE_SCOPE,
        )?;
        expect_bool(setup, "coordinator_admission_buffered", true)?;
        expect_u64(
            setup,
            "coordinator_admission_cache_bytes",
            crate::engine::packed::COORDINATOR_ADMISSION_CACHE_BYTES as u64,
        )?;
        expect_string(
            setup,
            "coordinator_admission_cache_scope",
            crate::engine::packed::COORDINATOR_ADMISSION_CACHE_SCOPE,
        )?;
        expect_u64(
            setup,
            "frontier",
            crate::engine::materialization_revision_count(profile),
        )?;
    }
    if matches!(
        mode,
        SampleMode::PackedLookup | SampleMode::PackedWideLookup
    ) {
        lookup::validate(object, matches!(mode, SampleMode::PackedWideLookup))?;
    } else if matches!(mode, SampleMode::PackedRange | SampleMode::PackedWideRange) {
        lookup::validate_range(object, matches!(mode, SampleMode::PackedWideRange))?;
    } else if matches!(mode, SampleMode::PackedWide) {
        lookup::validate_pages(object, true)?;
    } else if matches!(mode, SampleMode::Packed) && object.contains_key("query_cache_configuration")
    {
        lookup::validate_pages(object, false)?;
    }
    if matches!(mode, SampleMode::Disk) {
        expect_bool(object, "full_memory_graph_state", false)?;
        expect_bool(object, "full_memory_coordinator_metadata", false)?;
        expect_bool(object, "storage_metadata_memory_resident", false)?;
        expect_disk_storage_report(object, profile)?;
        expect_string(
            object,
            "authenticated_io_accounting",
            "partial-cached-primitives",
        )?;
        expect_string(
            object,
            "qualification",
            "nonqualifying-development-sampling",
        )?;
        expect_string(object, "budget_evaluation", "not-performed")?;
    }
    expect_bool(object, "query_deadline_enforced", false)?;
    expect_bool(object, "query_deadline_postchecked", true)?;
    expect_u64(object, "query_deadline_seconds", QUERY_DEADLINE.as_secs())?;
    expect_u64(object, "entities", profile.entities())?;
    expect_u64(object, "relationships", profile.relationships())?;
    let warmup = object
        .get("warmup")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"))?;
    let warmup_queries = value_u64(warmup, "queries")?;
    if warmup_queries != 96 {
        return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"));
    }
    let samples = object
        .get("samples")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"))?;
    let expected_samples = if profile == Bm01Profile::qualifying() {
        5
    } else {
        1
    };
    if samples.len() != expected_samples {
        return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"));
    }
    let mut claimed_queries = warmup_queries;
    for sample in samples {
        let sample = sample
            .as_object()
            .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"))?;
        let executions = value_u64(sample, "timed_executions")?;
        if mode.packed() {
            let work = sample
                .get("vault_work")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"))?;
            if work.len() != 2 {
                return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"));
            }
            for (value, name) in work
                .iter()
                .zip(["uste-empty", "uste-retained-after-identical-query"])
            {
                let object = value
                    .as_object()
                    .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"))?;
                expect_string(object, "cache", name)?;
                expect_vault_work(object.get("work"))?;
            }
        }
        let rounds = value_u64(sample, "rounds")?;
        let minimum = if profile == Bm01Profile::qualifying() {
            60_000
        } else {
            0
        };
        expect_u64(sample, "minimum_duration_milliseconds", minimum)?;
        if executions == 0
            || executions > 2_000_000
            || rounds.checked_mul(768) != Some(executions)
            || value_u64(sample, "elapsed_milliseconds")? < minimum
        {
            return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"));
        }
        claimed_queries = claimed_queries
            .checked_add(executions)
            .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"))?;
    }
    if claimed_queries != completed_queries {
        return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"));
    }
    let old_deadline = "\"query_deadline_enforced\":false";
    if report.matches(old_deadline).count() != 1 {
        return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"));
    }
    let mut finalized = report.replacen(old_deadline, "\"query_deadline_enforced\":true", 1);
    if profile == Bm01Profile::qualifying() {
        let old = "qualification-candidate-deadline-and-environment-unverified";
        if finalized.matches(old).count() != 1 {
            return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"));
        }
        finalized = finalized.replacen(old, "qualification-candidate-environment-unverified", 1);
    }
    Ok(finalized)
}

fn expect_vault_work(value: Option<&serde_json::Value>) -> Result<(), LinuxRunnerError> {
    let object = value
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"))?;
    expect_string(
        object,
        "measurement_scope",
        "single-owner-vault-completed-decrypt-calls",
    )?;
    for field in [
        "physical_device_io",
        "complete_authenticated_io",
        "includes_key_unwrap",
        "includes_other_vaults",
    ] {
        expect_bool(object, field, false)?;
    }
    for field in [
        "successful_calls",
        "failed_calls",
        "authenticated_encoded_bytes",
        "returned_plaintext_bytes",
    ] {
        value_u64(object, field)?;
    }
    Ok(())
}

fn expect_disk_storage_report(
    object: &serde_json::Map<String, serde_json::Value>,
    profile: Bm01Profile,
) -> Result<(), LinuxRunnerError> {
    let storage = object
        .get("storage_recovery")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"))?;
    expect_string(storage, "mode", "disk-blob-metadata-v1")?;
    expect_string(
        storage,
        "measurement_scope",
        "last-storage-owner-cold-open-only",
    )?;
    expect_bool(storage, "complete_filesystem_io_accounting", false)?;
    for key in [
        "resident_certificate_entries",
        "resident_blob_references",
        "resident_inventory_ids",
        "resident_namespace_totals",
    ] {
        expect_u64(storage, key, 0)?;
    }
    for phase in ["validation", "replay"] {
        let pass = storage
            .get(phase)
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"))?;
        expect_u64(
            pass,
            "groups",
            crate::materialization_revision_count(profile),
        )?;
        expect_u64(pass, "reference_bindings", 0)?;
        expect_u64(pass, "verified_logical_blob_bytes", 0)?;
    }
    Ok(())
}

fn expect_string(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    expected: &str,
) -> Result<(), LinuxRunnerError> {
    if object.get(key).and_then(serde_json::Value::as_str) != Some(expected) {
        return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"));
    }
    Ok(())
}

fn expect_bool(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    expected: bool,
) -> Result<(), LinuxRunnerError> {
    if object.get(key).and_then(serde_json::Value::as_bool) != Some(expected) {
        return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"));
    }
    Ok(())
}

fn expect_u64(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    expected: u64,
) -> Result<(), LinuxRunnerError> {
    if value_u64(object, key)? != expected {
        return Err(LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"));
    }
    Ok(())
}

fn value_u64(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<u64, LinuxRunnerError> {
    object
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL"))
}

fn parse_message(line: &str) -> WorkerMessage {
    match line {
        "bm01-query-start-v1" => WorkerMessage::QueryStarted,
        "bm01-query-finish-v1" => WorkerMessage::QueryFinished,
        _ => {
            if let Some(report) = line.strip_prefix("bm01-report-v1\t") {
                WorkerMessage::Report(report.to_owned())
            } else if valid_error(line.strip_prefix("bm01-error-v1\t")) {
                WorkerMessage::Error
            } else {
                WorkerMessage::Invalid
            }
        }
    }
}

fn read_protocol_line(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "unterminated sampling protocol line",
            ));
        }
        let (take, complete) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(position) => (position, true),
            None => (buffer.len(), false),
        };
        let new_length = line
            .len()
            .checked_add(take)
            .ok_or_else(|| io::Error::other("sampling protocol line overflow"))?;
        if new_length > MAX_PROTOCOL_LINE_BYTES {
            return Err(io::Error::other("sampling protocol line too large"));
        }
        line.try_reserve(take)
            .map_err(|_| io::Error::other("sampling protocol allocation failed"))?;
        line.extend_from_slice(&buffer[..take]);
        reader.consume(take + usize::from(complete));
        if complete {
            return String::from_utf8(line)
                .map(Some)
                .map_err(|_| io::Error::other("sampling protocol is not UTF-8"));
        }
    }
}

fn valid_error(code: Option<&str>) -> bool {
    code.is_some_and(|code| {
        !code.is_empty()
            && code.len() <= 64
            && code
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    })
}

#[cfg(test)]
mod tests {
    use std::{
        io::Cursor,
        process::{Command, Stdio},
        sync::mpsc,
        time::Duration,
    };

    use super::{
        MAX_PROTOCOL_LINE_BYTES, WorkerGuard, WorkerMessage, finalize_report, parse_message,
        read_protocol_line, supervise_protocol_with_deadline,
    };
    use crate::Bm01Profile;

    #[test]
    fn protocol_parser_is_closed_and_content_free() {
        assert_eq!(
            parse_message("bm01-query-start-v1"),
            WorkerMessage::QueryStarted
        );
        assert_eq!(
            parse_message("bm01-query-finish-v1"),
            WorkerMessage::QueryFinished
        );
        assert_eq!(
            parse_message("bm01-report-v1\t{\"schema\":\"x\"}"),
            WorkerMessage::Report("{\"schema\":\"x\"}".into())
        );
        assert_eq!(
            parse_message("bm01-error-v1\tUSTE_BM01_QUERY_MISMATCH"),
            WorkerMessage::Error
        );
        assert_eq!(
            parse_message("bm01-error-v1\tbad/path"),
            WorkerMessage::Invalid
        );
        assert_eq!(parse_message("other"), WorkerMessage::Invalid);
    }

    #[test]
    fn deadline_terminates_the_exact_worker_process() {
        for mode in [
            super::SampleMode::Legacy,
            super::SampleMode::Disk,
            super::SampleMode::Packed,
            super::SampleMode::PackedLookup,
            super::SampleMode::PackedRange,
            super::SampleMode::PackedWide,
            super::SampleMode::PackedWideLookup,
            super::SampleMode::PackedWideRange,
        ] {
            let child = Command::new("sleep")
                .arg("60")
                .stdin(Stdio::piped())
                .spawn()
                .unwrap();
            let mut worker = WorkerGuard::new(child).unwrap();
            let (sender, receiver) = mpsc::channel();
            sender.send(WorkerMessage::QueryStarted).unwrap();
            let error = supervise_protocol_with_deadline(
                &mut worker,
                &receiver,
                Bm01Profile::new(20).unwrap(),
                Duration::from_millis(10),
                mode,
            )
            .unwrap_err();
            assert_eq!(error.code(), "USTE_BM01_QUERY_DEADLINE");
            assert!(worker.reaped);
        }
    }

    #[test]
    fn protocol_lines_are_utf8_newline_terminated_and_bounded() {
        let mut valid = Cursor::new(b"bm01-query-start-v1\n".as_slice());
        assert_eq!(
            read_protocol_line(&mut valid).unwrap().as_deref(),
            Some("bm01-query-start-v1")
        );
        assert!(read_protocol_line(&mut valid).unwrap().is_none());
        assert!(read_protocol_line(&mut Cursor::new(b"unterminated")).is_err());
        assert!(read_protocol_line(&mut Cursor::new([0xff, b'\n'])).is_err());
        let oversized = vec![b'x'; MAX_PROTOCOL_LINE_BYTES + 1];
        assert!(read_protocol_line(&mut Cursor::new(oversized)).is_err());
    }

    #[test]
    fn supervisor_validates_counts_and_owns_enforcement_claim() {
        let report = concat!(
            "{\"schema\":\"bm01-linux-sampling-v1\",",
            "\"qualification\":\"nonqualifying-development-sampling\",",
            "\"query_deadline_enforced\":false,",
            "\"query_deadline_postchecked\":true,",
            "\"query_deadline_seconds\":30,",
            "\"entities\":20,\"relationships\":200,",
            "\"warmup\":{\"queries\":96},",
            "\"samples\":[{\"timed_executions\":768,\"rounds\":1,\"minimum_duration_milliseconds\":0,\"elapsed_milliseconds\":12}]}"
        );
        let finalized = finalize_report(
            report,
            Bm01Profile::new(20).unwrap(),
            864,
            super::SampleMode::Legacy,
        )
        .unwrap();
        assert!(finalized.contains("\"query_deadline_enforced\":true"));
        assert!(!finalized.contains("\"query_deadline_enforced\":false"));
        assert!(
            finalize_report(
                report,
                Bm01Profile::new(20).unwrap(),
                863,
                super::SampleMode::Legacy
            )
            .is_err()
        );
        assert!(
            finalize_report(
                "{}",
                Bm01Profile::new(20).unwrap(),
                864,
                super::SampleMode::Legacy
            )
            .is_err()
        );
    }

    #[test]
    fn supervisor_binds_disk_schema_and_preserves_required_windows() {
        let mut report = serde_json::json!({
            "schema": "bm01-linux-disk-sampling-v1", "qualification": "nonqualifying-development-sampling",
            "query_deadline_enforced": false, "query_deadline_postchecked": true, "query_deadline_seconds": 30,
            "entities": 20, "relationships": 200, "warmup": { "queries": 96 },
            "full_memory_graph_state": false, "full_memory_coordinator_metadata": false,
            "storage_metadata_memory_resident": false, "authenticated_io_accounting": "partial-cached-primitives",
            "storage_recovery": { "mode": "disk-blob-metadata-v1",
                "measurement_scope": "last-storage-owner-cold-open-only", "complete_filesystem_io_accounting": false,
                "resident_certificate_entries": 0, "resident_blob_references": 0,
                "resident_inventory_ids": 0, "resident_namespace_totals": 0,
                "validation": { "groups": 4, "reference_bindings": 0, "verified_logical_blob_bytes": 0 },
                "replay": { "groups": 4, "reference_bindings": 0, "verified_logical_blob_bytes": 0 } },
            "budget_evaluation": "not-performed", "samples": [{ "timed_executions": 768, "rounds": 1,
                "minimum_duration_milliseconds": 0, "elapsed_milliseconds": 12 }],
        });
        let profile = Bm01Profile::new(20).unwrap();
        for pointer in [
            "/storage_metadata_memory_resident",
            "/storage_recovery/mode",
            "/storage_recovery/resident_blob_references",
            "/storage_recovery/validation/groups",
            "/storage_recovery/replay/reference_bindings",
        ] {
            let mut wrong = report.clone();
            *wrong.pointer_mut(pointer).unwrap() = serde_json::Value::Null;
            assert!(
                finalize_report(&wrong.to_string(), profile, 864, super::SampleMode::Disk).is_err()
            );
        }
        for wrong_accounting in ["not-measured", "complete", "physical-device"] {
            let mut wrong = report.clone();
            wrong["authenticated_io_accounting"] = wrong_accounting.into();
            assert!(
                finalize_report(&wrong.to_string(), profile, 864, super::SampleMode::Disk).is_err()
            );
        }
        assert!(
            finalize_report(&report.to_string(), profile, 864, super::SampleMode::Disk).is_ok()
        );
        assert!(
            finalize_report(&report.to_string(), profile, 864, super::SampleMode::Legacy).is_err()
        );
        report["query_deadline_enforced"] = true.into();
        assert!(
            finalize_report(&report.to_string(), profile, 864, super::SampleMode::Disk).is_err()
        );
        report["query_deadline_enforced"] = false.into();
        report["query_deadline_seconds"] = 1.into();
        assert!(
            finalize_report(&report.to_string(), profile, 864, super::SampleMode::Disk).is_err()
        );
        report["query_deadline_seconds"] = 30.into();
        report["samples"][0]["rounds"] = 2.into();
        assert!(
            finalize_report(&report.to_string(), profile, 864, super::SampleMode::Disk).is_err()
        );
        report["samples"][0]["rounds"] = 1.into();
        report["full_memory_graph_state"] = true.into();
        assert!(
            finalize_report(&report.to_string(), profile, 864, super::SampleMode::Disk).is_err()
        );
        report["schema"] = "bm01-linux-sampling-v1".into();
        report["qualification"] =
            "qualification-candidate-deadline-and-environment-unverified".into();
        report["entities"] = 100_000.into();
        report["relationships"] = 1_000_000.into();
        let sample = serde_json::json!({ "timed_executions": 768, "rounds": 1,
            "minimum_duration_milliseconds": 60_000, "elapsed_milliseconds": 60_000 });
        report["samples"] = serde_json::Value::Array(vec![sample; 5]);
        assert!(
            finalize_report(
                &report.to_string(),
                Bm01Profile::qualifying(),
                3936,
                super::SampleMode::Legacy
            )
            .is_ok()
        );
        report["samples"][0]["elapsed_milliseconds"] = 59_999.into();
        assert!(
            finalize_report(
                &report.to_string(),
                Bm01Profile::qualifying(),
                3936,
                super::SampleMode::Legacy
            )
            .is_err()
        );
        report["samples"][0]["elapsed_milliseconds"] = 60_000.into();
        report["samples"][0]["minimum_duration_milliseconds"] = 0.into();
        assert!(
            finalize_report(
                &report.to_string(),
                Bm01Profile::qualifying(),
                3936,
                super::SampleMode::Legacy
            )
            .is_err()
        );
    }

    pub(super) fn packed_report() -> serde_json::Value {
        let work = serde_json::json!({
            "measurement_scope": "single-owner-vault-completed-decrypt-calls",
            "physical_device_io": false, "complete_authenticated_io": false,
            "includes_key_unwrap": false, "includes_other_vaults": false,
            "successful_calls": 0, "failed_calls": 0,
            "authenticated_encoded_bytes": 0, "returned_plaintext_bytes": 0,
        });
        serde_json::json!({
            "schema": "bm01-linux-packed-sampling-v1", "engine_benchmark": true,
            "qualification": "nonqualifying-development-sampling", "budget_evaluation": "not-performed",
            "full_memory_graph_state": false, "full_memory_coordinator_metadata": false,
            "complete_authenticated_io": false, "authenticated_io_accounting": "partial-single-owner-vault-decrypt",
            "setup_vault_work_scope": "last-cold-open-owner-only",
            "setup_vault_work": work.clone(), "warmup_vault_work": work.clone(),
            "storage_metadata_mode": "disk-certificate-and-blob-recovery",
            "query_deadline_enforced": false, "query_deadline_postchecked": true, "query_deadline_seconds": 30,
            "entities": 20, "relationships": 200, "frontier": 4, "warmup": {"queries": 96},
            "setup": {"complete_fixture": true, "frontier": 4,
                "graph_admission_cache_bytes": 64 * 1024 * 1024,
                "graph_admission_cache_scope": "fresh-per-canonical-family-then-fresh-semantic",
                "coordinator_admission_buffered": true,
                "coordinator_admission_cache_bytes": 64 * 1024 * 1024,
                "coordinator_admission_cache_scope": "fresh-per-canonical-family-then-fresh-correspondence"},
            "samples": [{"timed_executions": 768, "rounds": 1, "minimum_duration_milliseconds": 0, "elapsed_milliseconds": 12,
                "vault_work": [{"cache": "uste-empty", "work": work.clone()},
                    {"cache": "uste-retained-after-identical-query", "work": work.clone()}]}],
        })
    }

    #[test]
    fn supervisor_binds_packed_schema_and_never_upgrades_missing_io_or_qualification() {
        let report = packed_report();
        let profile = Bm01Profile::new(20).unwrap();
        let finalized =
            finalize_report(&report.to_string(), profile, 864, super::SampleMode::Packed).unwrap();
        let finalized: serde_json::Value = serde_json::from_str(&finalized).unwrap();
        assert_eq!(finalized["query_deadline_enforced"], true);
        assert_eq!(finalized["complete_authenticated_io"], false);
        assert_eq!(
            finalized["qualification"],
            "nonqualifying-development-sampling"
        );
        for pointer in [
            "/schema",
            "/engine_benchmark",
            "/full_memory_graph_state",
            "/full_memory_coordinator_metadata",
            "/complete_authenticated_io",
            "/authenticated_io_accounting",
            "/setup_vault_work_scope",
            "/setup_vault_work",
            "/warmup_vault_work",
            "/setup_vault_work/successful_calls",
            "/warmup_vault_work/failed_calls",
            "/samples/0/vault_work",
            "/samples/0/vault_work/0/cache",
            "/samples/0/vault_work/1/work",
            "/samples/0/vault_work/1/work/authenticated_encoded_bytes",
            "/samples/0/vault_work/1/work/returned_plaintext_bytes",
            "/storage_metadata_mode",
            "/qualification",
            "/budget_evaluation",
            "/frontier",
            "/setup/complete_fixture",
            "/setup/graph_admission_cache_bytes",
            "/setup/graph_admission_cache_scope",
            "/setup/coordinator_admission_buffered",
            "/setup/coordinator_admission_cache_bytes",
            "/setup/coordinator_admission_cache_scope",
            "/setup/frontier",
            "/query_deadline_enforced",
            "/query_deadline_postchecked",
            "/query_deadline_seconds",
            "/warmup/queries",
            "/samples/0/timed_executions",
            "/samples/0/rounds",
            "/samples/0/minimum_duration_milliseconds",
        ] {
            let mut wrong = report.clone();
            *wrong.pointer_mut(pointer).unwrap() = serde_json::Value::Null;
            assert!(
                finalize_report(&wrong.to_string(), profile, 864, super::SampleMode::Packed)
                    .is_err(),
                "{pointer}"
            );
        }
        for mode in [super::SampleMode::Legacy, super::SampleMode::Disk] {
            assert!(finalize_report(&report.to_string(), profile, 864, mode).is_err());
        }
        let mut unbuffered = report.clone();
        unbuffered["setup"]["coordinator_admission_buffered"] = false.into();
        assert!(
            finalize_report(
                &unbuffered.to_string(),
                profile,
                864,
                super::SampleMode::Packed
            )
            .is_err()
        );
        for (field, value) in [
            (
                "coordinator_admission_cache_bytes",
                serde_json::json!(32 * 1024 * 1024),
            ),
            (
                "coordinator_admission_cache_scope",
                serde_json::json!("caller-warmed"),
            ),
        ] {
            let mut wrong = report.clone();
            wrong["setup"][field] = value;
            assert!(
                finalize_report(&wrong.to_string(), profile, 864, super::SampleMode::Packed)
                    .is_err()
            );
        }
        for pointer in [
            "/setup_vault_work/physical_device_io",
            "/warmup_vault_work/complete_authenticated_io",
            "/samples/0/vault_work/0/work/includes_key_unwrap",
            "/samples/0/vault_work/1/work/includes_other_vaults",
        ] {
            let mut wrong = report.clone();
            *wrong.pointer_mut(pointer).unwrap() = true.into();
            assert!(
                finalize_report(&wrong.to_string(), profile, 864, super::SampleMode::Packed)
                    .is_err()
            );
        }
        for label in ["not-measured", "complete", "physical-device"] {
            let mut wrong = report.clone();
            wrong["authenticated_io_accounting"] = label.into();
            assert!(
                finalize_report(&wrong.to_string(), profile, 864, super::SampleMode::Packed)
                    .is_err()
            );
        }
        assert!(
            finalize_report(&report.to_string(), profile, 863, super::SampleMode::Packed).is_err()
        );
        let mut wrong = report.clone();
        wrong["query_deadline_enforced"] = true.into();
        assert!(
            finalize_report(&wrong.to_string(), profile, 864, super::SampleMode::Packed).is_err()
        );
        let absent = std::path::Path::new("absent-packed-supervisor");
        assert_eq!(
            super::supervise_packed_sample(
                absent,
                absent,
                absent,
                absent,
                Bm01Profile::qualifying()
            )
            .unwrap_err()
            .code(),
            "USTE_BM01_DISK_DEVELOPMENT_LIMIT"
        );
    }
}
