#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
//! Explicit Btrfs process-loss checks. Every killed process is a child owned by this test.
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
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
            "disk-process-loss-{}-{}",
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
            .write_all(b"synthetic process-loss fixture password, not a deployment credential")
            .unwrap();
        let oracle = root.join("oracle-summary");
        let mut generator = Command::new(EXECUTABLE);
        generator.args(["oracle-summary", "--entities", "20"]);
        let output = complete(generator);
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
            .arg(phase)
            .arg("--root")
            .arg(&self.root)
            .arg("--password-file")
            .arg(&self.password)
            .args(["--entities", "20"]);
        command
    }
    fn run(&self, phase: &str) -> serde_json::Value {
        let mut command = self.command(phase);
        if phase == "linux-disk-query" || phase == "linux-disk-sample" {
            command.arg("--oracle-file").arg(&self.oracle);
        }
        let output = complete(command);
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

struct ChildGuard(Option<Child>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn complete(mut command: Command) -> Output {
    let mut child = ChildGuard(Some(
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
    // Drain both pipes concurrently: the bounded oracle summary can exceed pipe capacity.
    let stdout = child.0.as_mut().unwrap().stdout.take().unwrap();
    let stderr = child.0.as_mut().unwrap().stderr.take().unwrap();
    let drain = |reader: Box<dyn Read + Send>| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            reader
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .unwrap();
            assert!(bytes.len() <= 1024 * 1024, "bounded CLI output");
            bytes
        })
    };
    let output = drain(Box::new(stdout));
    let errors = drain(Box::new(stderr));
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if child.0.as_mut().unwrap().try_wait().unwrap().is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "owned CLI process exceeded the test deadline"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let status = child.0.take().unwrap().wait().unwrap();
    Output {
        status,
        stdout: output.join().unwrap(),
        stderr: errors.join().unwrap(),
    }
}

#[test]
fn disk_cli_sigkill_prefixes_resume_and_match_separate_oracle() {
    for revision in [1_u64, 2, 3] {
        let fixture = Fixture::new();
        let mut command = fixture.command("linux-disk-create-crash-probe");
        command
            .arg("--pause-after-revision")
            .arg(revision.to_string());
        let mut child = ChildGuard(Some(
            command
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        ));
        let stdout = child.0.as_mut().unwrap().stdout.take().unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        let reader = thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout.take(1024)).read_line(&mut line);
            let _ = sender.send((result, line));
        });
        let (result, marker) = receiver
            .recv_timeout(Duration::from_secs(60))
            .expect("bounded readiness marker");
        assert!(result.unwrap() > 0 && marker.ends_with('\n'));
        let marker: serde_json::Value = serde_json::from_str(&marker).unwrap();
        assert_eq!(marker["schema"], "bm01-linux-crash-probe-v1");
        assert_eq!(marker["phase"], "durable-prefix-paused");
        assert_eq!(marker["frontier"], revision);
        assert_eq!(marker["planned_frontier"], 4);
        assert_eq!(marker["engine_benchmark"], false);
        reader.join().unwrap();
        child.0.as_mut().unwrap().kill().unwrap();
        let status = child.0.as_mut().unwrap().wait().unwrap();
        assert_eq!(status.signal(), Some(9));
        child.0.take();

        let resumed = fixture.run("linux-disk-resume");
        assert_eq!(resumed["recovered_revision"], revision);
        assert_eq!(resumed["frontier"], 4);
        assert_eq!(resumed["full_memory_graph_state"], false);
        assert_eq!(resumed["full_memory_coordinator_metadata"], false);
        let opened = fixture.run("linux-disk-open");
        assert_eq!(opened["recovered_revision"], 4);
        let queried = fixture.run("linux-disk-query");
        assert_eq!(queried["successful_queries"], 384);
        assert_eq!(queried["queries"], 384);
        assert_eq!(queried["engine_benchmark"], false);
        assert_eq!(queried["oracle_adjacency_memory_resident"], false);
        let retried = fixture.run("linux-disk-resume");
        assert_eq!(retried["recovered_revision"], 4);
        assert_eq!(retried["frontier"], 4);
    }
}

#[test]
fn disk_cli_crash_probe_refuses_nonprefixes_before_filesystem_access() {
    for revision in [0_u64, 4, u64::MAX] {
        let mut command = Command::new(EXECUTABLE);
        command
            .args([
                "linux-disk-create-crash-probe",
                "--root",
                "unused",
                "--password-file",
                "unused",
                "--entities",
                "20",
                "--pause-after-revision",
            ])
            .arg(revision.to_string());
        let output = complete(command);
        assert!(!output.status.success());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("USTE_BM01_CRASH_PROBE_REVISION")
        );
    }
}

#[test]
fn disk_cli_supervised_sampling_preserves_oracle_cache_pairs_and_deadline_claim() {
    let fixture = Fixture::new();
    fixture.run("linux-disk-create");
    let mut generator = Command::new(EXECUTABLE);
    generator.args(["oracle-bundle", "--entities", "20"]);
    let output = complete(generator);
    assert!(output.status.success());
    let bundle =
        uste_t20_bench::OracleBundle::parse(std::str::from_utf8(&output.stdout).unwrap()).unwrap();
    fs::write(&fixture.oracle, output.stdout).unwrap();
    let report = fixture.run("linux-disk-sample");
    assert_eq!(report["schema"], "bm01-linux-disk-sampling-v1");
    assert_eq!(
        report["qualification"],
        "nonqualifying-development-sampling"
    );
    assert_eq!(report["query_deadline_enforced"], true);
    assert_eq!(report["query_deadline_postchecked"], true);
    assert_eq!(report["query_deadline_seconds"], 30);
    assert_eq!(report["budget_evaluation"], "not-performed");
    assert_eq!(report["authenticated_io_accounting"], "not-measured");
    assert_eq!(report["full_memory_graph_state"], false);
    assert_eq!(report["full_memory_coordinator_metadata"], false);
    assert_eq!(report["storage_metadata_memory_resident"], true);
    assert_eq!(report["warmup"]["queries"], 96);
    assert_eq!(report["warmup"]["successes"], 96);
    let samples = report["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 1);
    let sample = &samples[0];
    assert_eq!(sample["timed_executions"], 768);
    assert_eq!(sample["rounds"], 1);
    assert_eq!(sample["minimum_duration_milliseconds"], 0);
    let cache = sample["cache_work"].as_array().unwrap();
    assert_eq!(cache.len(), 2);
    for work in cache {
        assert_eq!(work["index_cache_budget_bytes"], 64 * 1024 * 1024);
        assert!(work.get("index_pages_read").is_none());
        assert!(work.get("authorized_reads").is_none());
        assert!(work["index_cache_hits"].as_u64().unwrap() > 0);
    }
    assert!(cache[0]["index_cache_misses"].as_u64().unwrap() > 0);
    assert_eq!(cache[1]["index_cache_misses"], 0);
    assert_eq!(cache[0]["successful_visits"], cache[1]["successful_visits"]);
    let latencies = sample["latency_groups"].as_array().unwrap();
    assert_eq!(latencies.len(), 32);
    let all_count: u64 = latencies
        .iter()
        .filter(|group| group["class"] == "all")
        .map(|group| group["count"].as_u64().unwrap())
        .sum();
    assert_eq!(all_count, 768);
    let mut digest = blake3::Hasher::new_derive_key("USTE BM-01 linux-sampling-v1");
    digest.update(&1_u64.to_be_bytes());
    for expected in bundle.measured().expectations() {
        let uste_t20_bench::OracleExpectedOutcome::Output { output_digest, .. } = expected.outcome
        else {
            panic!("20/200 corpus must succeed");
        };
        for cache in [1, 2] {
            digest.update(&[
                cache,
                1,
                expected.query.class.code(),
                expected.query.direction.code(),
                expected.query.depth,
            ]);
            digest.update(&expected.query.ordinal.to_be_bytes());
            digest.update(&output_digest);
        }
    }
    assert_eq!(
        sample["output_digest"],
        digest.finalize().to_hex().to_string()
    );
    let mut refused = fixture.command("linux-disk-sample");
    refused
        .arg("--oracle-file")
        .arg(&fixture.oracle)
        .args(["--entities", "100000"]);
    let refused = complete(refused);
    assert!(!refused.status.success());
    assert!(
        String::from_utf8(refused.stderr)
            .unwrap()
            .contains("USTE_BM01_DISK_DEVELOPMENT_LIMIT")
    );
}
