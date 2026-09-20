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
        if matches!(phase, "query" | "sample") {
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
        if pause <= 1 {
            let mut wrong = fixture.command("rebuild");
            wrong.args(["--entities", "21"]);
            assert!(!complete(wrong).status.success());
            if pause == 0 {
                assert!(!complete(fixture.command("rebuild")).status.success());
            } else {
                let rebuilt = fixture.run("rebuild");
                assert_eq!(rebuilt["frontier"], 1);
                assert_eq!(rebuilt["origin_suffix_groups"], 0);
                assert_eq!(rebuilt["complete_fixture"], false);
            }
            assert!(
                fs::read(&certificates).unwrap() == prefix,
                "bootstrap rebuild changed authority"
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
        let owners = &resumed["owner_vault_work"];
        assert_eq!(owners["owner_count"], if pause <= 1 { 3 } else { 2 });
        assert_eq!(owners["owners"]["bootstrap_resume"].is_object(), pause <= 1);
        assert!(owners["owners"]["bootstrap"].is_null());
        assert_eq!(owners["owners"]["terminal"], resumed["terminal_vault_work"]);
        assert!(
            owners["total"]["successful_calls"].as_u64().unwrap()
                > resumed["terminal_vault_work"]["successful_calls"]
                    .as_u64()
                    .unwrap()
        );
        assert_eq!(owners["complete_authenticated_io"], false);
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
    for report in [&create, &open] {
        assert_eq!(report["proof_cache_bytes"], 64 * 1024 * 1024);
        assert_eq!(report["proof_cache_scope"], "fresh-per-preparation");
        assert_eq!(report["graph_admission_cache_bytes"], 64 * 1024 * 1024);
        assert_eq!(report["staging_cache_bytes"], 64 * 1024 * 1024);
        assert_eq!(
            report["staging_cache_scope"],
            "fresh-per-private-tree-batch"
        );
        assert_eq!(
            report["graph_admission_cache_scope"],
            "fresh-per-canonical-family-then-fresh-semantic"
        );
        assert_eq!(report["coordinator_admission_buffered"], true);
        assert_eq!(
            report["coordinator_admission_cache_bytes"],
            64 * 1024 * 1024
        );
        assert_eq!(
            report["coordinator_admission_cache_scope"],
            "fresh-per-canonical-family-then-fresh-correspondence"
        );
        assert_eq!(
            report["terminal_vault_work_scope"],
            "last-cold-open-owner-only"
        );
        assert!(
            report["terminal_vault_work"]["successful_calls"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert_eq!(
            report["terminal_vault_work"]["complete_authenticated_io"],
            false
        );
    }
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
    let misses = query["cache_misses"].as_u64().unwrap();
    assert_eq!(query["query_vault_work"]["successful_calls"], misses);
    assert_eq!(
        query["query_vault_work"]["authenticated_encoded_bytes"],
        misses * 20545
    );
    assert_eq!(
        query["query_vault_work"]["returned_plaintext_bytes"],
        misses * 16384
    );
    assert_eq!(query["query_vault_work"]["failed_calls"], 0);
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
    for phase in ["create", "open", "rebuild", "resume", "query", "sample"] {
        let mut command = Command::new(EXECUTABLE);
        command.arg(format!("linux-packed-{phase}")).args([
            "--root",
            "absent-packed-root",
            "--password-file",
            "absent-packed-password",
            "--entities",
            "100000",
        ]);
        if matches!(phase, "query" | "sample") {
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

#[test]
fn packed_cli_supervised_sampling_preserves_frozen_pairs_and_oracle_digest() {
    use uste_t20_bench::{Bm01Profile, OracleBundle, OracleExpectedOutcome};
    let fixture = Fixture::new();
    fixture.run("create");
    let certificates = fixture.root.join("bm01-linux-packed-engine/CERTIFICATES");
    let before = fs::read(&certificates).unwrap();
    let mut command = Command::new(EXECUTABLE);
    command.args(["oracle-bundle", "--entities", "20"]);
    let output = complete(command);
    assert!(output.status.success());
    fs::write(&fixture.oracle, output.stdout).unwrap();
    let report = fixture.run("sample");
    assert_eq!(report["schema"], "bm01-linux-packed-sampling-v1");
    assert_eq!(report["query_deadline_enforced"], true);
    assert_eq!(report["query_deadline_postchecked"], true);
    assert_eq!(report["query_deadline_seconds"], 30);
    assert_eq!(report["engine_benchmark"], true);
    assert_eq!(
        report["qualification"],
        "nonqualifying-development-sampling"
    );
    assert_eq!(report["budget_evaluation"], "not-performed");
    assert_eq!(report["complete_authenticated_io"], false);
    assert_eq!(
        report["authenticated_io_accounting"],
        "partial-single-owner-vault-decrypt"
    );
    assert_eq!(
        report["setup_vault_work_scope"],
        "last-cold-open-owner-only"
    );
    for field in ["setup_vault_work", "warmup_vault_work"] {
        let work = &report[field];
        assert!(work["successful_calls"].as_u64().unwrap() > 0);
        assert_eq!(work["failed_calls"], 0);
        assert_eq!(work["physical_device_io"], false);
        assert_eq!(work["complete_authenticated_io"], false);
    }
    assert_eq!(report["warmup"]["queries"], 96);
    assert_eq!(report["samples"].as_array().unwrap().len(), 1);
    let sample = &report["samples"][0];
    assert_eq!(sample["rounds"], 1);
    assert_eq!(sample["timed_executions"], 768);
    assert_eq!(sample["minimum_duration_milliseconds"], 0);
    assert_eq!(sample["latency_groups"].as_array().unwrap().len(), 32);
    assert!(sample.get("cached_index_work").is_none());
    let empty = &sample["cache_work"][0];
    let retained = &sample["cache_work"][1];
    assert_eq!(empty["index_cache_budget_bytes"], 64 * 1024 * 1024);
    assert!(empty["index_cache_misses"].as_u64().unwrap() > 0);
    assert_eq!(retained["index_cache_misses"], 0);
    assert!(retained["index_cache_hits"].as_u64().unwrap() > 0);
    assert!(
        sample["adapter_io"][0]["work"]["read_returned_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(sample["adapter_io"][1]["work"]["read_returned_bytes"], 0);
    assert_eq!(sample["adapter_io"][0]["work"]["write_returned_bytes"], 0);
    assert_eq!(sample["adapter_io"][1]["work"]["write_returned_bytes"], 0);
    let cold_crypto = &sample["vault_work"][0]["work"];
    let warm_crypto = &sample["vault_work"][1]["work"];
    let misses = empty["index_cache_misses"].as_u64().unwrap();
    assert_eq!(cold_crypto["successful_calls"], misses);
    assert_eq!(cold_crypto["failed_calls"], 0);
    assert_eq!(cold_crypto["authenticated_encoded_bytes"], misses * 20545);
    assert_eq!(cold_crypto["returned_plaintext_bytes"], misses * 16384);
    for field in [
        "successful_calls",
        "failed_calls",
        "authenticated_encoded_bytes",
        "returned_plaintext_bytes",
    ] {
        assert_eq!(warm_crypto[field], 0);
    }
    let bundle = OracleBundle::build(Bm01Profile::new(20).unwrap()).unwrap();
    let mut digest = blake3::Hasher::new_derive_key("USTE BM-01 linux-sampling-v1");
    digest.update(&1_u64.to_be_bytes());
    for expected in bundle.measured().expectations() {
        let (kind, output) = match expected.outcome {
            OracleExpectedOutcome::Output { output_digest, .. } => (1, output_digest),
            OracleExpectedOutcome::VisitLimit => (2, [0; 32]),
            OracleExpectedOutcome::ResultLimit => (3, [0; 32]),
        };
        for cache in [1, 2] {
            digest.update(&[
                cache,
                kind,
                expected.query.class.code(),
                expected.query.direction.code(),
                expected.query.depth,
            ]);
            digest.update(&expected.query.ordinal.to_be_bytes());
            digest.update(&output);
        }
    }
    assert_eq!(
        sample["output_digest"],
        digest.finalize().to_hex().to_string()
    );
    assert!(
        fs::read(&certificates).unwrap() == before,
        "sampling changed authority"
    );
}
