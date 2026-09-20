//! Opt-in bounded native development fixture, not a qualifying recovery campaign.
use super::*;

#[test]
#[ignore = "bounded native 513-record development run requires an explicit serial memory-limited invocation"]
fn packed_history_native_513_record_intermediate_tail_sigkill_resumes() {
    native_history_scale(513, 199, 201);
}

#[test]
#[ignore = "bounded native 4096-record development run requires an explicit serial memory-limited invocation"]
fn packed_history_native_4096_record_eight_batch_tail_sigkill_resumes() {
    native_history_scale(4096, 793, 801);
}

fn native_history_scale(records: u64, checkpoint_revision: u64, terminal_revision: u64) {
    // Retain this synthetic database on success or failure for resource/recovery inspection.
    let fixture = std::mem::ManuallyDrop::new(Fixture::new());
    println!("retained_native_history_root={}", fixture.root.display());
    let run = |phase| {
        let output =
            complete_with_deadline(fixture.command(phase, records), Duration::from_secs(1800));
        assert!(
            output.status.success(),
            "{phase}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_phase_work(&report);
        println!("phase={phase} report={report}");
        assert_eq!(report["development_record_limit"], 4096);
        assert_eq!(report["qualifying_recovery_trials"], 0);
        assert_eq!(report["engine_benchmark"], false);
        let nonces = &report["construction_nonce_session"];
        if matches!(phase, "create" | "resume") {
            assert_eq!(nonces["measurement_scope"], "construction-owner-only");
            assert_eq!(nonces["nonce_limit"], 1_048_576);
            assert_eq!(
                nonces["issued_nonces"].as_u64().unwrap()
                    + nonces["remaining_nonces"].as_u64().unwrap(),
                1_048_576
            );
            if phase == "create" {
                assert!(nonces["issued_nonces"].as_u64().unwrap() > 0);
            }
        } else {
            assert!(nonces.is_null());
        }
        report
    };
    let checkpoint = run("create");
    assert_eq!(checkpoint["frontier"], checkpoint_revision);
    assert_eq!(checkpoint["verified_history_versions"], 99 * records);
    let prefix = fs::read(fixture.certificates()).unwrap();
    // The certificate log includes one encrypted header frame before revision one.
    assert_eq!(prefix.len() as u64, (checkpoint_revision + 1) * 4161);
    assert_eq!(
        fixture.kill_at_records(
            "tail-prefix-crash-probe",
            Some(checkpoint_revision + 1),
            records,
            Duration::from_secs(1800)
        ),
        checkpoint_revision + 1
    );
    let partial = fs::read(fixture.certificates()).unwrap();
    assert!(partial.starts_with(&prefix));
    assert_eq!(partial.len() as u64, (checkpoint_revision + 2) * 4161);
    let resumed = run("resume");
    assert_eq!(resumed["recovered_frontier"], checkpoint_revision + 1);
    assert_eq!(resumed["resume_base_revision"], checkpoint_revision);
    assert_eq!(resumed["resume_suffix_groups"], 1);
    assert_eq!(resumed["frontier"], terminal_revision);
    assert_eq!(resumed["verified_history_versions"], 100 * records);
    let terminal = fs::read(fixture.certificates()).unwrap();
    assert!(terminal.starts_with(&partial));
    assert_eq!(terminal.len() as u64, (terminal_revision + 1) * 4161);
    let recovered = run("recover-checkpoint");
    assert_eq!(recovered["selected_base_revision"], checkpoint_revision);
    assert_eq!(
        recovered["suffix_groups"],
        terminal_revision - checkpoint_revision
    );
    assert_eq!(recovered["checkpoint_tail_replay"], true);
    assert_eq!(recovered["verified_history_versions"], 100 * records);
    assert_eq!(recovered["v1_state_digest"], resumed["v1_state_digest"]);
    let repeated = run("resume");
    assert_eq!(repeated["recovered_frontier"], terminal_revision);
    assert_eq!(repeated["resume_base_revision"], terminal_revision);
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
        (4097, 0, "USTE_BM06_PACKED_DEVELOPMENT_LIMIT"),
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
