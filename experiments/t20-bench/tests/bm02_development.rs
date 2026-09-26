#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
//! Runs a tiny BM-02 development plan end to end on a fresh Btrfs root owned by this test.
use std::{
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const EXECUTABLE: &str = env!("CARGO_BIN_EXE_uste-t20-bench");

#[test]
fn tiny_development_plan_commits_reads_and_labels_itself_nonqualifying() {
    let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
    fs::create_dir_all(&parent).unwrap();
    let root = parent.join(format!(
        "bm02-development-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let store = root.join("store");
    fs::create_dir(&store).unwrap();
    let password = root.join("password");
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&password)
        .unwrap()
        .write_all(b"synthetic BM-02 development fixture password, not a deployment credential")
        .unwrap();
    let run = |single: &str| {
        Command::new(EXECUTABLE)
            .arg("linux-bm02-development")
            .arg("--root")
            .arg(&store)
            .arg("--password-file")
            .arg(&password)
            .args(["--single", single, "--batches", "2", "--batch-events", "16"])
            .output()
            .unwrap()
    };
    let output = run("9");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["qualification"], "nonqualifying-development");
    assert_eq!(report["budget_evaluation"], "not-performed");
    assert_eq!(report["single_commit_latency"]["count"], 9);
    assert_eq!(report["create_commit_latency"]["count"], 7);
    assert_eq!(report["correction_commit_latency"]["count"], 2);
    assert_eq!(report["interleaved_read_latency"]["count"], 9);
    assert_eq!(report["batch_events"], 32);
    // Revision 1 installs policy; nine single commits and two batches follow.
    assert_eq!(report["final_revision"], 12);
    assert_eq!(report["entities_created"], 7 + 32);
    assert!(
        report["batch_events_per_second_phase_wall"]
            .as_f64()
            .unwrap()
            > 0.0
    );
    assert!(
        report["batch_events_per_second_commit_time"]
            .as_f64()
            .unwrap()
            >= report["batch_events_per_second_phase_wall"]
                .as_f64()
                .unwrap()
    );

    // The runner only creates stores; a second run against the same root must refuse.
    assert!(!run("1").status.success());
    fs::remove_dir_all(&root).unwrap();
}
