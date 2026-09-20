#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
//! Real separate-process packed history phases, not a qualifying recovery campaign.
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::{DirBuilderExt, OpenOptionsExt},
        process::ExitStatusExt,
    },
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const EXE: &str = env!("CARGO_BIN_EXE_uste-t20-bench");
#[path = "packed_history/prefix.rs"]
mod prefix;
#[path = "packed_history/scale.rs"]
mod scale;
struct Fixture {
    root: PathBuf,
    password: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!(
                "packed-history-cli-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let password = root.join("password");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&password)
            .unwrap()
            .write_all(b"synthetic packed history process password")
            .unwrap();
        Self { root, password }
    }
    fn command(&self, phase: &str, records: u64) -> Command {
        let mut command = Command::new(EXE);
        command
            .arg(format!("bm06-packed-linux-{phase}"))
            .arg("--root")
            .arg(&self.root)
            .arg("--password-file")
            .arg(&self.password)
            .arg("--records")
            .arg(records.to_string());
        command
    }
    fn run(&self, phase: &str) -> serde_json::Value {
        let output = complete(self.command(phase, 2));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = serde_json::from_slice(&output.stdout).unwrap();
        assert_phase_work(&report);
        report
    }
    fn kill_at(&self, phase: &str, pause: Option<u64>) -> u64 {
        self.kill_at_records(phase, pause, 2, Duration::from_secs(60))
    }
    fn kill_at_records(
        &self,
        phase: &str,
        pause: Option<u64>,
        records: u64,
        deadline: Duration,
    ) -> u64 {
        let mut command = self.command(phase, records);
        if let Some(pause) = pause {
            command.arg("--pause-after-revision").arg(pause.to_string());
        }
        let mut child = OwnedChild(Some(
            command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ));
        let stdout = child.0.as_mut().unwrap().stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
            let _ = sender.send(result);
        });
        let line = receiver.recv_timeout(deadline).unwrap().unwrap();
        let marker: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(marker["schema"], "bm06-packed-durable-prefix-v1");
        child.0.as_mut().unwrap().kill().unwrap();
        assert_eq!(child.0.as_mut().unwrap().wait().unwrap().signal(), Some(9));
        child.0.take();
        reader.join().unwrap();
        marker["frontier"].as_u64().unwrap()
    }
    fn certificates(&self) -> PathBuf {
        self.root.join("bm06-linux-packed-engine/CERTIFICATES")
    }
    fn remove_roots(&self) {
        let mut removed = 0;
        for entry in fs::read_dir(self.root.join("bm06-linux-packed-engine")).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name().to_str().unwrap().starts_with("p-") {
                fs::remove_file(entry.path()).unwrap();
                removed += 1;
            }
        }
        assert!(removed > 0);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
struct OwnedChild(Option<Child>);
fn assert_phase_work(report: &serde_json::Value) {
    let work = &report["phase_work"];
    assert_eq!(
        work["measurement_scope"],
        "sequential-command-phases-through-terminal-digest"
    );
    for flag in [
        "complete_authenticated_io",
        "physical_device_io",
        "qualifying_recovery_latency",
    ] {
        assert_eq!(work[flag], false);
    }
    let stages = work["stages"].as_object().unwrap();
    assert_eq!(stages.len(), 4);
    let total = &report["adapter_io"];
    for field in [
        "read_requested_bytes",
        "read_returned_bytes",
        "write_requested_bytes",
        "write_returned_bytes",
    ] {
        let sum: u64 = stages
            .values()
            .map(|stage| stage["adapter_io"][field].as_u64().unwrap())
            .sum();
        assert_eq!(sum, total[field].as_u64().unwrap());
    }
    for (operation, counts) in total["operations"].as_object().unwrap() {
        for field in ["calls", "failures"] {
            let sum: u64 = stages
                .values()
                .map(|stage| {
                    stage["adapter_io"]["operations"][operation][field]
                        .as_u64()
                        .unwrap()
                })
                .sum();
            assert_eq!(sum, counts[field].as_u64().unwrap());
        }
    }
    let sum: u64 = stages
        .values()
        .map(|stage| stage["elapsed_microseconds"].as_u64().unwrap())
        .sum();
    let elapsed = work["measured_elapsed_microseconds"].as_u64().unwrap();
    assert!(sum <= elapsed && elapsed - sum < 4);
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn complete(command: Command) -> Output {
    complete_with_deadline(command, Duration::from_secs(90))
}
fn complete_with_deadline(mut command: Command, deadline: Duration) -> Output {
    let mut child = OwnedChild(Some(
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
    let started = Instant::now();
    while child.0.as_mut().unwrap().try_wait().unwrap().is_none() {
        assert!(
            started.elapsed() < deadline,
            "owned packed history child deadline"
        );
        thread::sleep(Duration::from_millis(10));
    }
    child.0.take().unwrap().wait_with_output().unwrap()
}
#[test]
fn packed_history_cli_checkpoint_tail_recovery_and_origin_are_distinct() {
    let fixture = Fixture::new();
    let checkpoint = fixture.run("create");
    assert_eq!(checkpoint["graph_admission_cache_bytes"], 64 * 1024 * 1024);
    assert_eq!(
        checkpoint["graph_admission_cache_scope"],
        "fresh-per-canonical-family-then-fresh-semantic"
    );
    assert_eq!(checkpoint["coordinator_admission_buffered"], true);
    assert_eq!(
        checkpoint["coordinator_admission_cache_bytes"],
        64 * 1024 * 1024
    );
    assert_eq!(
        checkpoint["coordinator_admission_cache_scope"],
        "fresh-per-canonical-family-then-fresh-correspondence"
    );
    assert_eq!(checkpoint["frontier"], 100);
    assert_eq!(checkpoint["verified_history_versions"], 198);
    assert_eq!(
        fixture.run("open")["v1_state_digest"],
        checkpoint["v1_state_digest"]
    );
    let tail = fixture.run("tail");
    assert_eq!(tail["frontier"], 101);
    assert_eq!(tail["history_verified_through_revision"], 100);
    assert!(tail["v1_state_digest"].is_null());
    let recovered = fixture.run("recover");
    assert!(recovered["construction_nonce_session"].is_null());
    assert_eq!(recovered["suffix_groups"], 1);
    assert_eq!(recovered["verified_history_versions"], 200);
    assert_eq!(recovered["engine_benchmark"], false);
    assert_eq!(recovered["qualifying_recovery_trials"], 0);
    assert_eq!(recovered["complete_authenticated_io"], false);
    assert_eq!(recovered["incomplete_prefix_resume_implemented"], true);
    let certificates = fs::read(fixture.certificates()).unwrap();
    let explicit = fixture.run("recover-checkpoint");
    assert_eq!(explicit["selected_base_revision"], 100);
    assert_eq!(explicit["suffix_groups"], 1);
    assert_eq!(explicit["checkpoint_tail_replay"], true);
    assert_eq!(explicit["v1_state_digest"], recovered["v1_state_digest"]);
    assert!(fs::read(fixture.certificates()).unwrap() == certificates);
    let rebuilt = fixture.run("rebuild");
    assert_eq!(rebuilt["origin_suffix_groups"], 100);
    assert_eq!(rebuilt["v1_state_digest"], recovered["v1_state_digest"]);
    assert_eq!(
        fixture.run("open")["v1_state_digest"],
        recovered["v1_state_digest"]
    );
}
#[test]
fn packed_history_cli_rejects_larger_profiles_before_missing_paths() {
    for phase in [
        "create",
        "open",
        "tail",
        "recover",
        "recover-checkpoint",
        "rebuild",
        "resume",
        "tail-crash-probe",
    ] {
        for records in ["514", "100000"] {
            let mut command = Command::new(EXE);
            command.arg(format!("bm06-packed-linux-{phase}")).args([
                "--root",
                "absent-packed-history-root",
                "--password-file",
                "absent-packed-history-password",
                "--records",
                records,
            ]);
            let output = complete(command);
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            assert!(
                String::from_utf8(output.stderr)
                    .unwrap()
                    .contains("USTE_BM06_PACKED_DEVELOPMENT_LIMIT")
            );
        }
    }
}

#[test]
fn packed_history_cli_sigkill_prefixes_resume_exactly() {
    let reference = Fixture::new();
    let expected = reference.run("create")["v1_state_digest"].clone();
    for pause in [0, 1, 2, 50, 99, 100] {
        let fixture = Fixture::new();
        assert_eq!(fixture.kill_at("create-crash-probe", Some(pause)), pause);
        let prefix = fs::read(fixture.certificates()).unwrap();
        if pause != 0 {
            assert!(!complete(fixture.command("resume", 1)).status.success());
            assert!(
                fs::read(fixture.certificates()).unwrap() == prefix,
                "wrong profile changed prefix"
            );
        }
        if pause == 1 {
            let rebuilt = fixture.run("rebuild");
            assert_eq!(rebuilt["frontier"], 1);
            assert_eq!(rebuilt["origin_suffix_groups"], 0);
            assert_eq!(rebuilt["verified_history_versions"], 0);
            assert!(
                fs::read(fixture.certificates()).unwrap() == prefix,
                "policy rebuild appended events"
            );
        }
        fs::OpenOptions::new()
            .append(true)
            .open(fixture.certificates())
            .unwrap()
            .write_all(&[0x71; 3])
            .unwrap();
        let resumed = fixture.run("resume");
        assert_eq!(resumed["recovered_frontier"], pause);
        assert_eq!(resumed["repaired_certificate_tail_bytes"], 3);
        assert_eq!(resumed["bounded_bootstrap_resume"], pause <= 1);
        assert_eq!(
            resumed["resume_base_revision"],
            if pause <= 1 { 1 } else { pause - 1 }
        );
        assert_eq!(resumed["resume_suffix_groups"], u64::from(pause >= 2));
        assert_eq!(resumed["frontier"], 100);
        assert_eq!(resumed["verified_history_versions"], 198);
        assert_eq!(resumed["v1_state_digest"], expected);
        let terminal = fs::read(fixture.certificates()).unwrap();
        assert!(terminal.starts_with(&prefix), "certified prefix changed");
        assert_eq!(fixture.run("resume")["v1_state_digest"], expected);
        assert!(
            fs::read(fixture.certificates()).unwrap() == terminal,
            "resume duplicated events"
        );
    }
}

#[test]
fn packed_history_cli_sigkill_tail_and_partial_origin_preserve_frontiers() {
    let fixture = Fixture::new();
    assert_eq!(fixture.kill_at("create-crash-probe", Some(50)), 50);
    let prefix = fs::read(fixture.certificates()).unwrap();
    fixture.remove_roots();
    assert!(!complete(fixture.command("resume", 2)).status.success());
    assert!(!complete(fixture.command("rebuild", 1)).status.success());
    assert!(
        fs::read(fixture.certificates()).unwrap() == prefix,
        "cache-loss refusal changed source"
    );
    let rebuilt = fixture.run("rebuild");
    assert_eq!(rebuilt["frontier"], 50);
    assert_eq!(rebuilt["origin_suffix_groups"], 49);
    assert_eq!(rebuilt["verified_history_versions"], 98);
    assert!(
        fs::read(fixture.certificates()).unwrap() == prefix,
        "partial rebuild appended events"
    );
    assert_eq!(fixture.run("resume")["frontier"], 100);
    assert_eq!(fixture.kill_at("tail-crash-probe", None), 101);
    let terminal = fs::read(fixture.certificates()).unwrap();
    assert!(!complete(fixture.command("open", 2)).status.success());
    let resumed = fixture.run("resume");
    assert_eq!(resumed["recovered_frontier"], 101);
    assert_eq!(resumed["resume_base_revision"], 100);
    assert_eq!(resumed["resume_suffix_groups"], 1);
    assert_eq!(resumed["frontier"], 101);
    assert_eq!(resumed["verified_history_versions"], 200);
    assert!(
        fs::read(fixture.certificates()).unwrap() == terminal,
        "tail retry duplicated events"
    );
    assert_eq!(
        fixture.run("open")["v1_state_digest"],
        resumed["v1_state_digest"]
    );
    assert_eq!(
        fixture.run("resume")["v1_state_digest"],
        resumed["v1_state_digest"]
    );
    assert!(
        fs::read(fixture.certificates()).unwrap() == terminal,
        "terminal resume changed source"
    );
}

#[test]
fn packed_history_cli_probe_refuses_future_frontiers_before_io() {
    for pause in [101, u64::MAX] {
        let mut command = Command::new(EXE);
        command.args([
            "bm06-packed-linux-create-crash-probe",
            "--root",
            "absent-packed-history-root",
            "--password-file",
            "absent-packed-history-password",
            "--records",
            "2",
            "--pause-after-revision",
            &pause.to_string(),
        ]);
        let output = complete(command);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("USTE_BM06_PACKED_PROBE_REVISION")
        );
    }
}
