#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::{
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

fn fresh_root(label: &str) -> PathBuf {
    let discriminator = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/memory-adapter-process-tests")
        .join(format!("{label}-{}-{discriminator}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    root
}

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_uste-memory-demo")
}

fn create_kill_and_recover(root: &Path) {
    let mut child = Command::new(binary())
        .arg("crash-probe")
        .arg(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(
        line.trim(),
        "M1_CRASH_READY schema=memory-pilot-v1 revision=4"
    );
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());

    let recovered = Command::new(binary())
        .arg("verify-crash")
        .arg(root)
        .output()
        .unwrap();
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    assert_eq!(
        String::from_utf8(recovered.stdout).unwrap().trim(),
        "M1_CRASH_RECOVERED revision=4 citation=exact"
    );
}

#[test]
fn acknowledged_memory_commit_survives_sigkill_and_wrong_key_fails_closed() {
    let root = fresh_root("kill");
    create_kill_and_recover(&root);
    let wrong = Command::new(binary())
        .arg("verify-wrong-key")
        .arg(&root)
        .output()
        .unwrap();
    assert!(!wrong.status.success());
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("KeyOrIntegrityFailure"));
}

#[test]
fn authenticated_committed_byte_corruption_fails_closed() {
    let root = fresh_root("corrupt");
    create_kill_and_recover(&root);
    let certificates = root
        .join("uste-derived-index")
        .join("memory-demo")
        .join("CERTIFICATES");
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(certificates)
        .unwrap();
    let length = file.metadata().unwrap().len();
    assert!(length > 32);
    file.seek(SeekFrom::Start(length - 17)).unwrap();
    let mut byte = [0_u8; 1];
    file.read_exact(&mut byte).unwrap();
    byte[0] ^= 0x80;
    file.seek(SeekFrom::Start(length - 17)).unwrap();
    file.write_all(&byte).unwrap();
    file.sync_all().unwrap();

    let corrupted = Command::new(binary())
        .arg("verify-crash")
        .arg(root)
        .output()
        .unwrap();
    assert!(!corrupted.status.success());
    assert!(String::from_utf8_lossy(&corrupted.stderr).contains("KeyOrIntegrityFailure"));
}
