use super::*;
use std::{collections::BTreeMap, fs, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf};

#[test]
fn native_staging_buffer_selection_preserves_model_defaults_and_work_limits() {
    use crate::recovery_materialization::Bm06Profile;
    for original in [
        Limits::new(Bm01Profile::new(20).unwrap()).unwrap(),
        Limits::new(Bm01Profile::new(100_000).unwrap()).unwrap(),
        Limits::recovery(Bm06Profile::new(1).unwrap()).unwrap(),
        Limits::recovery(Bm06Profile::new(4096).unwrap()).unwrap(),
        Limits::recovery(Bm06Profile::new(100_000).unwrap()).unwrap(),
    ] {
        let selected = buffered_staging(original);
        for (before, after) in [
            (original.origin.genesis.stage, selected.origin.genesis.stage),
            (original.origin.suffix.graph, selected.origin.suffix.graph),
            (original.publication.stage, selected.publication.stage),
        ] {
            assert_eq!(before.staging_cache_bytes, None);
            assert_eq!(after.staging_cache_bytes, Some(STAGING_CACHE_BYTES));
            assert_eq!(
                (
                    before.maximum_batches,
                    before.maximum_read_pages,
                    before.maximum_written_pages,
                    before.deltas_per_batch
                ),
                (
                    after.maximum_batches,
                    after.maximum_read_pages,
                    after.maximum_written_pages,
                    after.deltas_per_batch
                )
            );
            assert_eq!(
                (
                    before.batch.maximum_read_pages,
                    before.batch.maximum_read_bytes,
                    before.batch.maximum_deltas,
                    before.batch.pack.maximum_pages
                ),
                (
                    after.batch.maximum_read_pages,
                    after.batch.maximum_read_bytes,
                    after.batch.maximum_deltas,
                    after.batch.pack.maximum_pages
                )
            );
            assert_eq!(
                before.certificates.maximum_encoded_bytes(),
                after.certificates.maximum_encoded_bytes()
            );
        }
        let before = original.origin.suffix.metadata;
        let after = selected.origin.suffix.metadata;
        assert_eq!(before.staging.staging_cache_bytes, None);
        assert_eq!(after.staging.staging_cache_bytes, Some(STAGING_CACHE_BYTES));
        assert_eq!(
            (
                before.staging.maximum_references,
                before.staging.maximum_owners,
                before.maximum_groups,
                before.maximum_encoded_bytes,
                before.certificate_window
            ),
            (
                after.staging.maximum_references,
                after.staging.maximum_owners,
                after.maximum_groups,
                after.maximum_encoded_bytes,
                after.certificate_window
            )
        );
        assert_eq!(
            before.staging.batch.maximum_read_pages,
            after.staging.batch.maximum_read_pages
        );
        assert_eq!(
            before.staging.batch.maximum_read_bytes,
            after.staging.batch.maximum_read_bytes
        );
    }
}

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
    fn prefix(&self, stop: u64) -> BTreeMap<String, Vec<u8>> {
        let profile = Bm01Profile::new(20).unwrap();
        let limits = Limits::new(profile).unwrap();
        let mut fs = ObservedFileSystem::new(open_filesystem(&self.root).unwrap());
        let mut adapter =
            PortableRecoveryAdapter::new(credential::read_password(&self.password).unwrap());
        let vault = KeyVault::create(scope().database(), &mut adapter, OsEntropy).unwrap();
        let mut raw = CommitCoordinator::create(
            &mut fs,
            scope(),
            retention().unwrap(),
            EntryName::new(DATABASE).unwrap(),
            vault,
            OsEntropy,
            GraphState::new(scope()),
        )
        .unwrap();
        disk::install_policy(&mut raw, &mut fs).unwrap();
        drop(raw);
        let recovery = open_recovery(&mut fs, &mut adapter, limits).unwrap().0;
        let (mut live, _) = recover_packed_graph_origin(
            recovery,
            &mut fs,
            retention().unwrap(),
            CoordinatorRecoveryLimits::new(1, 0).unwrap(),
            limits.origin,
        )
        .unwrap();
        let policy_roots = self.files(true);
        assert_eq!(policy_roots.len(), 3);
        let mut policy = kernel(benchmark_policy(scope()).unwrap()).unwrap();
        let principal = authenticate(&policy).unwrap();
        visit_disk_batches(profile, |sequence, operations| {
            if sequence <= stop {
                engine::commit_batch(
                    &mut live,
                    &mut fs,
                    &mut policy,
                    &principal,
                    DiskBatch {
                        sequence,
                        operations,
                        idempotency_key: identity(sequence, IdempotencyKey::from_bytes),
                        transaction_id: identity(sequence, TransactionId::from_bytes),
                    },
                    &mut SystemClock::new(),
                    limits,
                )?;
            }
            Ok(())
        })
        .unwrap();
        policy_roots
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
    for phase in ["create", "open", "rebuild", "resume"] {
        assert_eq!(
            run(absent, absent, Bm01Profile::qualifying(), phase)
                .unwrap_err()
                .code(),
            "USTE_BM01_DISK_DEVELOPMENT_LIMIT"
        );
    }
    assert_eq!(
        run(absent, absent, Bm01Profile::new(20).unwrap(), "unexpected")
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
    assert_eq!(created["data_bearing_prefix_resume_implemented"], true);
    assert_eq!(created["policy_only_prefix_resume_implemented"], true);
    assert_eq!(created["legacy_unbound_policy_resume_supported"], false);
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
    for phase in ["open", "rebuild", "resume"] {
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
    for phase in ["open", "rebuild", "resume"] {
        assert!(fixture.run(phase).is_err());
        assert!(fs::read(&path).unwrap() == bytes, "committed bytes changed");
        assert!(fixture.files(true) == roots, "packed roots changed");
    }
}

#[test]
fn packed_native_explicit_prefixes_stream_to_same_terminal_state() {
    let fixture = Fixture::new();
    let created = fixture.run("create").unwrap();
    let source = fixture.files(false);
    let profile = Bm01Profile::new(20).unwrap();
    let limits = Limits::new(profile).unwrap();
    for base in 1..=4 {
        let mut fs = ObservedFileSystem::new(open_filesystem(&fixture.root).unwrap());
        let mut adapter =
            PortableRecoveryAdapter::new(credential::read_password(&fixture.password).unwrap());
        let recovery = open_recovery(&mut fs, &mut adapter, limits).unwrap().0;
        let (live, digest, groups) = engine::prefix::recover(
            &mut fs,
            recovery,
            profile,
            uste_types::CommitRevision::new(base).unwrap(),
        )
        .unwrap();
        assert_eq!(groups, 4 - base);
        assert_eq!(digest.is_some(), base == 4);
        assert_eq!(live.state().unwrap().revision().get(), 4);
        assert_eq!(live.overlay_counts(), (0, 0));
        drop(live);
        drop(fs);
        assert_eq!(
            fixture.run("open").unwrap()["v1_state_digest"],
            created["v1_state_digest"]
        );
        assert!(
            fixture.files(false) == source,
            "prefix recovery changed authority"
        );
    }
}

#[test]
fn packed_native_data_prefix_resume_and_missing_roots_fail_closed() {
    let reference = Fixture::new();
    let expected = reference.run("create").unwrap()["v1_state_digest"].clone();
    for frontier in 1..=3 {
        let fixture = Fixture::new();
        let policy_roots = fixture.prefix(frontier);
        let source = fixture.files(false);
        let roots = fixture.files(true);
        assert!(fixture.run("open").is_err());
        assert!(
            run(
                &fixture.root,
                &fixture.password,
                Bm01Profile::new(21).unwrap(),
                "resume"
            )
            .is_err()
        );
        assert!(
            fixture.files(false) == source,
            "wrong profile changed authority"
        );
        assert!(fixture.files(true) == roots, "wrong profile changed roots");
        if frontier == 1 {
            assert_eq!(
                fixture.run("resume").unwrap_err().code(),
                "USTE_BM01_PACKED_BOOTSTRAP_PROFILE"
            );
            assert!(fixture.run("rebuild").is_err());
            assert!(
                fixture.files(false) == source,
                "unbound policy rebuild changed source"
            );
            assert!(
                fixture.files(true) == roots,
                "unbound policy rebuild changed roots"
            );
            continue;
        }
        // Complete cache loss must not silently turn resume into origin reconstruction.
        for name in roots.keys() {
            fs::remove_file(fixture.root.join(DATABASE).join(name)).unwrap();
        }
        assert!(fixture.run("resume").is_err());
        assert!(
            fixture.files(false) == source,
            "cache-loss refusal changed authority"
        );
        // Retain the policy triple and one newer unpaired manifest. Selection must use the
        // complete older triple and stream the certified suffix before fresh materialization.
        for (name, bytes) in &policy_roots {
            fs::write(fixture.root.join(DATABASE).join(name), bytes).unwrap();
        }
        let (name, bytes) = roots
            .iter()
            .find(|(name, _)| !policy_roots.contains_key(*name))
            .unwrap();
        fs::write(fixture.root.join(DATABASE).join(name), bytes).unwrap();
        let resumed = fixture.run("resume").unwrap();
        assert_eq!(resumed["resume_base_revision"], 1);
        assert_eq!(resumed["resume_suffix_groups"], frontier - 1);
        assert_eq!(resumed["v1_state_digest"], expected);
        let final_source = fixture.files(false);
        let final_roots = fixture.files(true);
        assert_eq!(fixture.run("resume").unwrap()["v1_state_digest"], expected);
        assert!(
            fixture.files(false) == final_source,
            "completed resume changed authority"
        );
        assert!(
            fixture.files(true) == final_roots,
            "completed resume republished roots"
        );
    }
}

#[test]
fn packed_native_partial_origin_rebuild_preserves_actual_frontier_before_resume() {
    let reference = Fixture::new();
    let terminal = reference.run("create").unwrap()["v1_state_digest"].clone();
    for frontier in [2, 3] {
        let fixture = Fixture::new();
        fixture.prefix(frontier);
        let source = fixture.files(false);
        for name in fixture.files(true).keys() {
            fs::remove_file(fixture.root.join(DATABASE).join(name)).unwrap();
        }
        assert!(fixture.run("resume").is_err());
        assert!(
            run(
                &fixture.root,
                &fixture.password,
                Bm01Profile::new(21).unwrap(),
                "rebuild"
            )
            .is_err()
        );
        assert!(
            fixture.files(true).is_empty(),
            "wrong profile published partial roots"
        );
        assert!(
            fixture.files(false) == source,
            "wrong profile changed partial authority"
        );
        let rebuilt = fixture.run("rebuild").unwrap();
        assert_eq!(rebuilt["frontier"], frontier);
        assert_eq!(rebuilt["origin_suffix_groups"], frontier - 1);
        assert_eq!(rebuilt["complete_fixture"], false);
        assert!(fixture.run("open").is_err());
        let repeated = fixture.run("rebuild").unwrap();
        assert_eq!(repeated["v1_state_digest"], rebuilt["v1_state_digest"]);
        assert!(
            fixture.files(false) == source,
            "partial rebuild appended events"
        );
        let resumed = fixture.run("resume").unwrap();
        assert_eq!(resumed["frontier"], 4);
        assert_eq!(resumed["complete_fixture"], true);
        assert_eq!(resumed["resume_base_revision"], frontier);
        assert_eq!(resumed["resume_suffix_groups"], 0);
        assert_eq!(resumed["v1_state_digest"], terminal);
    }
}
