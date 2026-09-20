use super::*;
use std::{collections::BTreeMap, fs, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf};
struct Fixture {
    root: PathBuf,
    password: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
        fs::create_dir_all(&parent).unwrap();
        let root = parent.join(format!(
            "packed-native-test-{}-{}",
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
            .write_all(b"synthetic packed native fixture, not deployment credentials")
            .unwrap();
        Self { root, password }
    }
    fn run(&self, phase: &str) -> Result<serde_json::Value, LinuxRunnerError> {
        run(
            &self.root,
            &self.password,
            Bm01Profile::new(20).unwrap(),
            phase,
        )
        .map(|json| serde_json::from_str(&json).unwrap())
    }
    fn files(&self, derived: bool) -> BTreeMap<String, Vec<u8>> {
        fs::read_dir(self.root.join(DATABASE))
            .unwrap()
            .map(|e| e.unwrap())
            .filter(|e| e.file_type().unwrap().is_file())
            .filter_map(|e| {
                let name = e.file_name().into_string().unwrap();
                let is_derived = name.starts_with("p-");
                if (derived && !is_derived)
                    || (!derived
                        && name != "CERTIFICATES"
                        && !name.starts_with("j-")
                        && name != "MANIFEST"
                        && name != "KEY")
                {
                    None
                } else {
                    Some((name, fs::read(e.path()).unwrap()))
                }
            })
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn packed_native_limits_and_phase_refuse_before_io() {
    let absent = Path::new("absent-packed-admission-test");
    for phase in ["create", "open", "rebuild"] {
        assert_eq!(
            run(absent, absent, Bm01Profile::qualifying(), phase)
                .unwrap_err()
                .code(),
            "USTE_BM01_DISK_DEVELOPMENT_LIMIT"
        );
    }
    assert_eq!(
        run(absent, absent, Bm01Profile::new(20).unwrap(), "resume")
            .unwrap_err()
            .code(),
        "USTE_BM01_PACKED_PHASE"
    );
    assert_eq!(
        query_correctness(absent, absent, absent, Bm01Profile::qualifying())
            .unwrap_err()
            .code(),
        "USTE_BM01_DISK_DEVELOPMENT_LIMIT"
    );
}
#[test]
fn packed_native_terminal_close_open_rebuild_and_separate_oracle() {
    let fixture = Fixture::new();
    let created = fixture.run("create").unwrap();
    assert_eq!(created["frontier"], 4);
    assert_eq!(created["engine_benchmark"], false);
    assert_eq!(created["incomplete_prefix_resume_implemented"], false);
    let opened = fixture.run("open").unwrap();
    assert_eq!(opened["v1_state_digest"], created["v1_state_digest"]);
    let source = fixture.files(false);
    assert!(source.contains_key("CERTIFICATES"));
    assert!(source.contains_key("MANIFEST"));
    assert!(source.contains_key("KEY"));
    assert!(source.keys().any(|name| name.starts_with("j-")));
    assert!(fixture.run("create").is_err());
    let rebuilt = fixture.run("rebuild").unwrap();
    assert_eq!(rebuilt["origin_suffix_groups"], 3);
    assert_eq!(rebuilt["v1_state_digest"], created["v1_state_digest"]);
    assert!(
        fixture.files(false) == source,
        "authoritative bytes changed"
    );
    let oracle = fixture.root.join("oracle");
    let profile = Bm01Profile::new(20).unwrap();
    let roots = fixture.files(true);
    let mut session = prepare(&fixture.root, &fixture.password, profile, "open").unwrap();
    let mut clock = SystemClock::new();
    visit_disk_batches(profile, |sequence, operations| {
        engine::commit_batch(
            &mut session.coordinator,
            &mut session.filesystem,
            &mut session.policy,
            &session.principal,
            DiskBatch {
                sequence,
                idempotency_key: identity(sequence, IdempotencyKey::from_bytes),
                transaction_id: identity(sequence, TransactionId::from_bytes),
                operations,
            },
            &mut clock,
            session.limits,
        )?;
        assert_eq!(session.coordinator.overlay_counts(), (0, 0));
        Ok(())
    })
    .unwrap();
    drop(session);
    assert!(
        fixture.files(true) == roots,
        "exact retries republished roots"
    );
    assert!(
        fixture.files(false) == source,
        "exact retries changed authoritative bytes"
    );
    fs::write(&oracle, OracleSummary::build(profile).unwrap().to_tsv()).unwrap();
    let output: serde_json::Value = serde_json::from_str(
        &query_correctness(&fixture.root, &fixture.password, &oracle, profile).unwrap(),
    )
    .unwrap();
    assert_eq!(output["queries"], 384);
    assert_eq!(output["successful_queries"], 384);
    assert_eq!(output["oracle_adjacency_memory_resident"], false);
    assert_eq!(output["complete_authenticated_io"], false);
    assert_eq!(output["query_adapter_io"]["write_returned_bytes"], 0);
    assert!(output["cache_hits"].as_u64().unwrap() > 0);
    assert!(
        fixture.files(false) == source,
        "authoritative bytes changed"
    );
    fs::write(
        &oracle,
        OracleSummary::build(Bm01Profile::new(21).unwrap())
            .unwrap()
            .to_tsv(),
    )
    .unwrap();
    assert_eq!(
        query_correctness(&fixture.root, &fixture.password, &oracle, profile)
            .unwrap_err()
            .code(),
        "USTE_BM01_ORACLE_PROFILE"
    );
}
#[test]
fn packed_native_cache_loss_requires_explicit_rebuild_and_preserves_source() {
    for corrupt in [false, true] {
        let fixture = Fixture::new();
        let created = fixture.run("create").unwrap();
        let roots = fixture.files(true);
        assert!(!roots.is_empty());
        let source = fixture.files(false);
        for (name, mut bytes) in roots {
            let path = fixture.root.join(DATABASE).join(name);
            if corrupt {
                bytes[137] ^= 1;
                fs::write(path, bytes).unwrap();
            } else {
                fs::remove_file(path).unwrap();
            }
        }
        assert!(fixture.run("open").is_err());
        assert!(
            fixture.files(false) == source,
            "authoritative bytes changed"
        );
        let rebuilt = fixture.run("rebuild").unwrap();
        assert_eq!(rebuilt["v1_state_digest"], created["v1_state_digest"]);
        assert_eq!(fixture.run("open").unwrap()["frontier"], 4);
        assert!(
            fixture.files(false) == source,
            "authoritative bytes changed"
        );
    }
}
#[test]
fn packed_native_wrong_key_profile_and_committed_corruption_fail_closed() {
    let fixture = Fixture::new();
    fixture.run("create").unwrap();
    let source = fixture.files(false);
    let roots = fixture.files(true);
    let wrong = fixture.root.join("wrong-password");
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&wrong)
        .unwrap()
        .write_all(b"different synthetic password")
        .unwrap();
    for phase in ["open", "rebuild"] {
        assert!(run(&fixture.root, &wrong, Bm01Profile::new(20).unwrap(), phase).is_err());
        assert!(
            run(
                &fixture.root,
                &fixture.password,
                Bm01Profile::new(21).unwrap(),
                phase
            )
            .is_err()
        );
        assert!(fixture.files(true) == roots, "packed roots changed");
        assert!(
            fixture.files(false) == source,
            "authoritative bytes changed"
        );
    }
    let path = fixture.root.join(DATABASE).join("CERTIFICATES");
    let mut bytes = fs::read(&path).unwrap();
    bytes[4 * 4161 + 137] ^= 1;
    fs::write(&path, &bytes).unwrap();
    for phase in ["open", "rebuild"] {
        assert!(fixture.run(phase).is_err());
        assert!(fs::read(&path).unwrap() == bytes, "committed bytes changed");
        assert!(fixture.files(true) == roots, "packed roots changed");
    }
}
