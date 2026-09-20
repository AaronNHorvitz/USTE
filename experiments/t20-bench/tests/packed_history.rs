#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
//! Real separate-process packed history phases, not a qualifying recovery campaign.
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const EXE: &str = env!("CARGO_BIN_EXE_uste-t20-bench");
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
    fn run(&self, phase: &str) -> serde_json::Value {
        let mut command = Command::new(EXE);
        command
            .arg(format!("bm06-packed-linux-{phase}"))
            .arg("--root")
            .arg(&self.root)
            .arg("--password-file")
            .arg(&self.password)
            .args(["--records", "2"]);
        let output = complete(command);
        assert!(
            output.status.success(),
            "{}",
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
            started.elapsed() < Duration::from_secs(90),
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
    assert_eq!(recovered["suffix_groups"], 1);
    assert_eq!(recovered["verified_history_versions"], 200);
    assert_eq!(recovered["engine_benchmark"], false);
    assert_eq!(recovered["qualifying_recovery_trials"], 0);
    assert_eq!(recovered["complete_authenticated_io"], false);
    assert_eq!(recovered["incomplete_prefix_resume_implemented"], false);
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
    for phase in ["create", "open", "tail", "recover", "rebuild"] {
        for records in ["3", "100000"] {
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
