//! Opt-in bounded native development fixture, not a qualifying recovery campaign.
use super::*;

#[test]
#[ignore = "bounded native 513-record development run requires an explicit serial memory-limited invocation"]
fn packed_history_native_513_record_intermediate_tail_sigkill_resumes() {
    // Retain this synthetic database on success or failure for resource/recovery inspection.
    let fixture = std::mem::ManuallyDrop::new(Fixture::new());
    println!("retained_native_history_root={}", fixture.root.display());
    let run = |phase| {
        let output = complete_with_deadline(fixture.command(phase, 513), Duration::from_secs(1800));
        assert!(
            output.status.success(),
            "{phase}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        println!("phase={phase} report={report}");
        assert_eq!(report["development_record_limit"], 513);
        assert_eq!(report["qualifying_recovery_trials"], 0);
        assert_eq!(report["engine_benchmark"], false);
        report
    };
    let checkpoint = run("create");
    assert_eq!(checkpoint["frontier"], 199);
    assert_eq!(checkpoint["verified_history_versions"], 50_787);
    let prefix = fs::read(fixture.certificates()).unwrap();
    // The certificate log includes one encrypted header frame before revision one.
    assert_eq!(prefix.len(), 200 * 4161);
    assert_eq!(
        fixture.kill_at_records(
            "tail-prefix-crash-probe",
            Some(200),
            513,
            Duration::from_secs(1800)
        ),
        200
    );
    let partial = fs::read(fixture.certificates()).unwrap();
    assert!(partial.starts_with(&prefix));
    assert_eq!(partial.len(), 201 * 4161);
    let resumed = run("resume");
    assert_eq!(resumed["recovered_frontier"], 200);
    assert_eq!(resumed["resume_base_revision"], 199);
    assert_eq!(resumed["resume_suffix_groups"], 1);
    assert_eq!(resumed["frontier"], 201);
    assert_eq!(resumed["verified_history_versions"], 51_300);
    let terminal = fs::read(fixture.certificates()).unwrap();
    assert!(terminal.starts_with(&partial));
    assert_eq!(terminal.len(), 202 * 4161);
    let recovered = run("recover-checkpoint");
    assert_eq!(recovered["selected_base_revision"], 199);
    assert_eq!(recovered["suffix_groups"], 2);
    assert_eq!(recovered["checkpoint_tail_replay"], true);
    assert_eq!(recovered["verified_history_versions"], 51_300);
    assert_eq!(recovered["v1_state_digest"], resumed["v1_state_digest"]);
    let repeated = run("resume");
    assert_eq!(repeated["recovered_frontier"], 201);
    assert_eq!(repeated["resume_base_revision"], 201);
    assert_eq!(repeated["resume_suffix_groups"], 0);
    assert_eq!(repeated["v1_state_digest"], resumed["v1_state_digest"]);
    assert_eq!(run("open")["v1_state_digest"], resumed["v1_state_digest"]);
    assert!(fs::read(fixture.certificates()).unwrap() == terminal);
}

#[test]
fn packed_history_tail_prefix_probe_admits_only_a_bounded_tail_before_io() {
    for (records, pause, error) in [
        (513, 199, "USTE_BM06_PACKED_PROBE_REVISION"),
        (513, 202, "USTE_BM06_PACKED_PROBE_REVISION"),
        (2, 100, "USTE_BM06_PACKED_PROBE_REVISION"),
        (2, u64::MAX, "USTE_BM06_PACKED_PROBE_REVISION"),
        (514, 0, "USTE_BM06_PACKED_DEVELOPMENT_LIMIT"),
        (100_000, 19_406, "USTE_BM06_PACKED_DEVELOPMENT_LIMIT"),
    ] {
        let mut command = Command::new(EXE);
        command.args([
            "bm06-packed-linux-tail-prefix-crash-probe",
            "--root",
            "absent-packed-history-root",
            "--password-file",
            "absent-packed-history-password",
            "--records",
            &records.to_string(),
            "--pause-after-revision",
            &pause.to_string(),
        ]);
        let output = complete(command);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr).unwrap().contains(error));
    }
}
