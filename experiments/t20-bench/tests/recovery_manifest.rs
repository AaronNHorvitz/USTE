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
