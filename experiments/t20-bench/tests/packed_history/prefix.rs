use super::*;

fn prefix(fixture: &Fixture, phase: &str, records: u64, target: u64) -> Output {
    let mut command = fixture.command(phase, records);
    command.arg("--through-revision").arg(target.to_string());
    complete(command)
}
fn report(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn bounded_prefix_policy_only_exact_resume_and_rewind_refusal() {
    let fixture = Fixture::new();
    let first = report(prefix(&fixture, "create-prefix", 2, 1));
    assert_eq!(first["frontier"], 1);
    assert_eq!(first["verified_history_versions"], 0);
    let middle = report(prefix(&fixture, "resume-prefix", 2, 50));
    assert_eq!(middle["frontier"], 50);
    assert_eq!(middle["construction_target_revision"], 50);
    assert_eq!(middle["verified_history_versions"], 98);
    let certificates = fs::read(fixture.certificates()).unwrap();
    let repeated = report(prefix(&fixture, "resume-prefix", 2, 50));
    assert_eq!(middle["v1_state_digest"], repeated["v1_state_digest"]);
    assert!(fs::read(fixture.certificates()).unwrap() == certificates);
    for (phase, target) in [("resume-prefix", 49), ("create-prefix", 50)] {
        assert!(!prefix(&fixture, phase, 2, target).status.success());
        assert!(fs::read(fixture.certificates()).unwrap() == certificates);
    }
    let checkpoint = fixture.run("resume");
    assert_eq!(checkpoint["frontier"], 100);
    assert_eq!(checkpoint["verified_history_versions"], 198);
    assert!(checkpoint["construction_target_revision"].is_null());
}

#[test]
fn bounded_prefix_multibatch_clean_restart_preserves_source() {
    let fixture = Fixture::new();
    let first = report(prefix(&fixture, "create-prefix", 513, 3));
    assert_eq!(first["verified_history_versions"], 513);
    let certificates = fs::read(fixture.certificates()).unwrap();
    let second = report(prefix(&fixture, "resume-prefix", 513, 5));
    assert_eq!(second["frontier"], 5);
    assert_eq!(second["verified_history_versions"], 1026);
    let extended = fs::read(fixture.certificates()).unwrap();
    assert!(extended.starts_with(&certificates));
    assert!(extended.len() > certificates.len());
    let repeated = report(prefix(&fixture, "resume-prefix", 513, 5));
    assert_eq!(second["v1_state_digest"], repeated["v1_state_digest"]);
    assert!(fs::read(fixture.certificates()).unwrap() == extended);
}

#[test]
fn bounded_prefix_invalid_targets_and_flags_fail_before_io() {
    for (phase, records, flags, expected) in [
        (
            "create-prefix",
            "513",
            vec!["--through-revision", "0"],
            "PREFIX_TARGET",
        ),
        (
            "create-prefix",
            "513",
            vec!["--through-revision", "2"],
            "PREFIX_TARGET",
        ),
        (
            "resume-prefix",
            "513",
            vec!["--through-revision", "201"],
            "PREFIX_TARGET",
        ),
        (
            "resume-prefix",
            "513",
            vec!["--through-revision", "18446744073709551615"],
            "PREFIX_TARGET",
        ),
        (
            "create-prefix",
            "514",
            vec!["--through-revision", "1"],
            "DEVELOPMENT_LIMIT",
        ),
        (
            "resume-prefix",
            "100000",
            vec!["--through-revision", "1"],
            "DEVELOPMENT_LIMIT",
        ),
        ("create-prefix", "2", vec![], "require --through-revision"),
        (
            "create",
            "2",
            vec!["--through-revision", "1"],
            "other phases refuse",
        ),
        (
            "resume-prefix",
            "2",
            vec!["--through-revision", "1", "--through-revision", "1"],
            "duplicate",
        ),
        (
            "resume-prefix",
            "2",
            vec!["--through-revision", "1", "--pause-after-revision", "1"],
            "other phases refuse",
        ),
    ] {
        let mut command = Command::new(EXE);
        command
            .arg(format!("bm06-packed-linux-{phase}"))
            .args([
                "--root",
                "absent-packed-prefix-root",
                "--password-file",
                "absent-packed-prefix-password",
                "--records",
                records,
            ])
            .args(flags);
        let output = complete(command);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
