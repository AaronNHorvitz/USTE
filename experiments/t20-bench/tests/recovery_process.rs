#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
//! Every signaled process is a child created and owned by this test.
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::{DirBuilderExt, FileExt, OpenOptionsExt},
        process::ExitStatusExt,
    },
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const EXE: &str = env!("CARGO_BIN_EXE_uste-t20-bench");

struct Fixture {
    root: PathBuf,
    password: PathBuf,
    records: u64,
}
impl Fixture {
    fn new(records: u64) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!(
                "bm06-process-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let password = root.join("password");
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&password)
            .unwrap()
            .write_all(b"synthetic BM-06 process-test credential, not a deployment password")
            .unwrap();
        Self {
            root,
            password,
            records,
        }
    }
    fn command(&self, phase: &str) -> Command {
        let mut command = Command::new(EXE);
        command
            .arg(format!("bm06-linux-{phase}"))
            .arg("--root")
            .arg(&self.root)
            .arg("--password-file")
            .arg(&self.password)
            .arg("--records")
            .arg(self.records.to_string());
        command
    }
    fn run(&self, phase: &str) -> serde_json::Value {
        let output = complete(self.command(phase));
        assert!(
            output.status.success(),
            "{phase}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["engine_benchmark"], false);
        assert_eq!(report["qualifying_recovery_trials"], 0);
        assert_eq!(report["storage_metadata_memory_resident"], false);
        assert_eq!(report["kernel_filesystem_device_cache"], "uncontrolled");
        assert!(!String::from_utf8_lossy(&output.stdout).contains(self.root.to_str().unwrap()));
        report
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
    let mut child = OwnedChild(Some(
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
    let deadline = Instant::now() + Duration::from_secs(120);
    while child.0.as_mut().unwrap().try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "owned BM-06 test child exceeded deadline"
        );
        thread::sleep(Duration::from_millis(10));
    }
    child.0.take().unwrap().wait_with_output().unwrap()
}

#[test]
fn bm06_native_profile_refuses_before_filesystem_or_credential_access() {
    use uste_t20_bench::{
        linux_runner::disk::recovery::run, recovery_materialization::Bm06Profile,
    };
    let missing = std::path::Path::new("/nonexistent-bm06-test-must-not-open");
    for records in [3, 100_000] {
        assert_eq!(
            run(
                missing,
                missing,
                Bm06Profile::new(records).unwrap(),
                "create"
            )
            .unwrap_err()
            .code(),
            "USTE_BM06_DEVELOPMENT_LIMIT"
        );
    }
    assert_eq!(
        run(missing, missing, Bm06Profile::new(2).unwrap(), "unknown")
            .unwrap_err()
            .code(),
        "USTE_BM06_PHASE"
    );
}

#[test]
fn bm06_native_sigkill_tail_recovers_exact_history_and_rejects_substitution() {
    let mut fixture = Fixture::new(2);
    let created = fixture.run("create");
    assert_eq!(created["frontier"], 100);
    assert_eq!(created["verified_history_versions"], 198);
    fixture.records = 1;
    assert!(!complete(fixture.command("tail")).status.success());
    fixture.records = 2;
    let mut command = fixture.command("tail-crash-probe");
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
    let line = receiver
        .recv_timeout(Duration::from_secs(30))
        .unwrap()
        .unwrap();
    let marker: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(marker["schema"], "bm06-durable-tail-v1");
    assert_eq!(marker["frontier"], 101);
    assert_eq!(marker["base_revision"], 100);
    assert!(!complete(fixture.command("open")).status.success()); // live owner
    child.0.as_mut().unwrap().kill().unwrap();
    assert_eq!(child.0.as_mut().unwrap().wait().unwrap().signal(), Some(9));
    child.0.take();
    reader.join().unwrap();
    let recovered = fixture.run("recover");
    assert_eq!(recovered["initial_graph_revision"], 100);
    assert_eq!(recovered["initial_metadata_revision"], 100);
    assert_eq!(recovered["frontier"], 101);
    assert_eq!(recovered["verified_history_versions"], 200);
    assert_eq!(recovered["verified_payload_bytes"], 819200);
    assert_eq!(fixture.run("open")["initial_graph_revision"], 101);
    assert_eq!(fixture.run("recover")["initial_graph_revision"], 101);
    let wrong = fixture.root.join("wrong-password");
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&wrong)
        .unwrap()
        .write_all(b"wrong synthetic BM-06 credential")
        .unwrap();
    let mut command = Command::new(EXE);
    command
        .arg("bm06-linux-open")
        .arg("--root")
        .arg(&fixture.root)
        .arg("--password-file")
        .arg(wrong)
        .args(["--records", "2"]);
    let output = complete(command);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(fixture.run("open")["frontier"], 101);
    // Required committed bytes must never fall back to an older valid graph/cache root.
    let certificate = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(fixture.root.join("bm06-linux-disk-engine/CERTIFICATES"))
        .unwrap();
    let offset = 101 * 4161 + 100;
    let mut original = [0_u8];
    assert_eq!(certificate.read_at(&mut original, offset).unwrap(), 1);
    assert_eq!(certificate.write_at(&[original[0] ^ 1], offset).unwrap(), 1);
    certificate.sync_all().unwrap();
    for phase in ["open", "recover"] {
        let output = complete(fixture.command(phase));
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("USTE_BM06_OPEN"));
    }
    assert_eq!(certificate.write_at(&original, offset).unwrap(), 1);
    certificate.sync_all().unwrap();
    assert_eq!(fixture.run("open")["frontier"], 101);
}

#[test]
fn bm06_native_tail_and_open_preserve_explicit_phase_frontiers() {
    let fixture = Fixture::new(1);
    assert_eq!(fixture.run("create")["verified_history_versions"], 99);
    for phase in ["create", "recover", "open"] {
        assert!(!complete(fixture.command(phase)).status.success());
    }
    let tail = fixture.run("tail");
    assert_eq!(tail["frontier"], 101);
    assert_eq!(tail["verified_history_revision"], 100);
    assert_eq!(tail["verified_history_versions"], 99);
    assert!(!complete(fixture.command("open")).status.success()); // pending derived publication
    assert_eq!(fixture.run("recover")["verified_history_versions"], 100);
    assert!(!complete(fixture.command("tail")).status.success());
    assert_eq!(fixture.run("open")["verified_payload_bytes"], 409600);
}
