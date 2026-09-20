#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
//! Separate-process packed correctness and owned-child SIGKILL; no benchmark qualification.
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::OpenOptionsExt, process::ExitStatusExt},
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
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
    fn command(&self, phase: &str) -> Command {
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
        command
    }
    fn run(&self, phase: &str) -> serde_json::Value {
        let output = complete(self.command(phase));
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
struct OwnedChild(Option<Child>);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn complete(mut command: Command) -> Output {
    let mut owned = OwnedChild(Some(
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
    let started = Instant::now();
    loop {
        if owned.0.as_mut().unwrap().try_wait().unwrap().is_some() {
            return owned.0.take().unwrap().wait_with_output().unwrap();
        }
        if started.elapsed() > Duration::from_secs(90) {
            panic!("owned packed command deadline exceeded");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn packed_cli_sigkill_bootstrap_and_unpaired_metadata_resume_exactly() {
    let reference = Fixture::new();
    let expected = reference.run("create");
    for pause in 0..=4 {
        let fixture = Fixture::new();
        let mut child = OwnedChild(Some(
            fixture
                .command("create-crash-probe")
                .args(["--pause-after-revision", &pause.to_string()])
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
        let line = receiver
            .recv_timeout(Duration::from_secs(60))
            .unwrap()
            .unwrap();
        let marker: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(marker["schema"], "bm01-packed-durable-prefix-v1");
        assert_eq!(marker["frontier"], pause);
        child.0.as_mut().unwrap().kill().unwrap();
        assert_eq!(child.0.as_mut().unwrap().wait().unwrap().signal(), Some(9));
        child.0.take();
        reader.join().unwrap();
        let certificates = fixture.root.join("bm01-linux-packed-engine/CERTIFICATES");
        let prefix = fs::read(&certificates).unwrap();
        // An empty store has no prior profile to contradict. A certified policy/data prefix does.
        if pause != 0 {
            let mut wrong = fixture.command("resume");
            wrong.args(["--entities", "21"]);
            assert!(!complete(wrong).status.success());
            assert!(
                fs::read(&certificates).unwrap() == prefix,
                "wrong profile changed authority"
            );
        }
        // Deliberately incomplete certificate tail: reporting must retain the initial opener's
        // repair count, not replace it with the later cold-admission report.
        fs::OpenOptions::new()
            .append(true)
            .open(&certificates)
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
        assert_eq!(resumed["v1_state_digest"], expected["v1_state_digest"]);
        let terminal = fs::read(&certificates).unwrap();
        assert!(terminal.starts_with(&prefix), "certified prefix changed");
        assert_eq!(
            fixture.run("resume")["v1_state_digest"],
            expected["v1_state_digest"]
        );
        assert!(
            fs::read(&certificates).unwrap() == terminal,
            "retry changed authority"
        );
        assert_eq!(fixture.run("query")["successful_queries"], 384);
    }
}

#[test]
fn packed_cli_probe_refuses_future_revision_before_io() {
    for pause in [5, u64::MAX] {
        let mut command = Command::new(EXECUTABLE);
        command.args([
            "linux-packed-create-crash-probe",
            "--root",
            "absent-packed-probe-root",
            "--password-file",
            "absent-packed-probe-password",
            "--entities",
            "20",
            "--pause-after-revision",
            &pause.to_string(),
        ]);
        let output = complete(command);
        assert!(!output.status.success());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("USTE_BM01_PACKED_PROBE_REVISION")
        );
        assert!(output.stdout.is_empty());
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
