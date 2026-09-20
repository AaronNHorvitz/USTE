#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
use std::process::Command;

#[test]
fn bm06_cli_manifest_is_fixture_only_and_refuses_mixed_or_unbounded_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_uste-t20-bench"))
        .args(["bm06-manifest", "--records", "2"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["events"], 200);
    assert_eq!(report["records"], 2);
    assert_eq!(report["frontier_revision"], 101);
    assert_eq!(report["checkpoint_revision"], 100);
    assert_eq!(report["history_payload_bytes"], 819200);
    assert_eq!(report["engine_benchmark"], false);
    assert_eq!(report["database_materialized"], false);
    assert_eq!(report["recovery_trials_run"], 0);
    assert_eq!(report["qualification"], "nonqualifying-small-scale");
    assert!(report["canonical_request_stream_digest"].is_null());
    assert_eq!(
        report["synthetic_stream_digest"],
        "b0cc0a82916763e1a764bd1e5611261de407487a5936677ce87cd9d0046ce0fe"
    );
    for arguments in [
        vec!["--records"],
        vec!["--records", "0"],
        vec!["--records", "100001"],
        vec!["--records", "18446744073709551616"],
        vec!["--records", "-1"],
        vec!["--entities", "2"],
        vec!["--records", "2", "--records", "3"],
        vec!["--root", "/must-not-access"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_uste-t20-bench"))
            .arg("bm06-manifest")
            .args(arguments)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn bm06_disk_cli_preserves_model_boundary_and_rejects_exact_scale() {
    for records in ["3", "100000"] {
        let output = Command::new(env!("CARGO_BIN_EXE_uste-t20-bench"))
            .args(["bm06-disk-check", "--records", records])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    let output = Command::new(env!("CARGO_BIN_EXE_uste-t20-bench"))
        .args(["bm06-disk-check", "--records", "2"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["engine_benchmark"], false);
    assert_eq!(report["filesystem_profile"], "durable-memory-model");
    assert_eq!(report["verified_versions"], 200);
    assert_eq!(report["verified_payload_bytes"], 819200);
    assert_eq!(report["base_revision"], 100);
    assert_eq!(report["recovered_revision"], 101);
    assert_eq!(report["qualifying_recovery_trials"], 0);
    assert_eq!(report["storage_metadata_memory_resident"], false);
}

#[test]
fn bm06_packed_cli_preserves_history_and_model_boundary() {
    for arguments in [
        vec![],
        vec!["--records", "3"],
        vec!["--records", "100000"],
        vec!["--records", "2", "--entities", "20"],
        vec!["--root", "absent-packed-bm06-root"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_uste-t20-bench"))
            .arg("bm06-packed-check")
            .args(arguments)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    let output = Command::new(env!("CARGO_BIN_EXE_uste-t20-bench"))
        .args(["bm06-packed-check", "--records", "2"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["engine_benchmark"], false);
    assert_eq!(report["filesystem_profile"], "durable-memory-model");
    assert_eq!(report["verified_versions"], 200);
    assert_eq!(report["verified_payload_bytes"], 819200);
    assert_eq!(report["base_revision"], 100);
    assert_eq!(report["recovered_revision"], 101);
    assert_eq!(report["origin_suffix_groups"], 100);
    assert_eq!(report["qualifying_recovery_trials"], 0);
    assert_eq!(report["complete_authenticated_io"], false);
}
