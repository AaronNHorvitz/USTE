#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
//! Separate-process terminal commands, not SIGKILL/resume qualification.
use std::{
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const EXECUTABLE: &str = env!("CARGO_BIN_EXE_uste-t20-bench");
struct Fixture {
    root: PathBuf,
    password: PathBuf,
    oracle: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
        fs::create_dir_all(&parent).unwrap();
        let root = parent.join(format!(
            "packed-terminal-process-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let password = root.join("password");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&password)
            .unwrap()
            .write_all(b"synthetic packed terminal process password")
            .unwrap();
        let oracle = root.join("oracle");
        let mut command = Command::new(EXECUTABLE);
        command.args(["oracle-summary", "--entities", "20"]);
        let output = complete(command);
        assert!(output.status.success());
        fs::write(&oracle, output.stdout).unwrap();
        Self {
            root,
            password,
            oracle,
        }
    }
    fn run(&self, phase: &str) -> serde_json::Value {
        let mut command = Command::new(EXECUTABLE);
        command
            .arg(format!("linux-packed-{phase}"))
            .arg("--root")
            .arg(&self.root)
            .arg("--password-file")
            .arg(&self.password)
            .args(["--entities", "20"]);
        if phase == "query" {
            command.arg("--oracle-file").arg(&self.oracle);
        }
        let output = complete(command);
        assert!(
            output.status.success(),
            "{phase}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn complete(mut command: Command) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if started.elapsed() > Duration::from_secs(90) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("owned packed command deadline exceeded");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn packed_cli_terminal_phases_preserve_state_and_separate_oracle() {
    let fixture = Fixture::new();
    let create = fixture.run("create");
    let open = fixture.run("open");
    let resumed = fixture.run("resume");
    assert_eq!(resumed["resume_base_revision"], 4);
    assert_eq!(resumed["resume_suffix_groups"], 0);
    assert_eq!(create["frontier"], 4);
    assert_eq!(create["v1_state_digest"], open["v1_state_digest"]);
    let query = fixture.run("query");
    assert_eq!(query["queries"], 384);
    assert_eq!(query["successful_queries"], 384);
    assert_eq!(query["engine_benchmark"], false);
    assert_eq!(query["preemptive_deadline_enforced"], false);
    let rebuild = fixture.run("rebuild");
    assert_eq!(rebuild["origin_suffix_groups"], 3);
    assert_eq!(rebuild["v1_state_digest"], create["v1_state_digest"]);
    let second = fixture.run("query");
    for field in [
        "output_digest",
        "visits",
        "logical_result_bytes",
        "successful_queries",
        "expected_visit_limits",
        "expected_result_limits",
    ] {
        assert_eq!(second[field], query[field], "{field}");
    }
}
#[test]
fn packed_cli_qualifying_size_refuses_before_missing_paths_are_used() {
    for phase in ["create", "open", "rebuild", "resume", "query"] {
        let mut command = Command::new(EXECUTABLE);
        command.arg(format!("linux-packed-{phase}")).args([
            "--root",
            "absent-packed-root",
            "--password-file",
            "absent-packed-password",
            "--entities",
            "100000",
        ]);
        if phase == "query" {
            command.args(["--oracle-file", "absent-packed-oracle"]);
        }
        let output = complete(command);
        assert!(!output.status.success());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("USTE_BM01_DISK_DEVELOPMENT_LIMIT")
        );
        assert!(output.stdout.is_empty());
    }
}
