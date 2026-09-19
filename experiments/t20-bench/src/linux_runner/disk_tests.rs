use super::*;
use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf};

#[test]
fn native_profile_admission_is_separate_and_precedes_filesystem_or_process_access() {
    assert!(
        validate_native_profile(Bm01Profile::new(MAX_NATIVE_DEVELOPMENT_ENTITIES).unwrap()).is_ok()
    );
    assert_eq!(
        materialization_revision_count(Bm01Profile::new(MAX_NATIVE_DEVELOPMENT_ENTITIES).unwrap()),
        23
    );
    let absent = Path::new("deliberately-absent-native-profile-boundary");
    for count in [MAX_NATIVE_DEVELOPMENT_ENTITIES + 1, 100_000] {
        let profile = Bm01Profile::new(count).unwrap();
        for phase in ["create", "resume", "open"] {
            assert_eq!(
                run(absent, absent, profile, phase).unwrap_err().code(),
                "USTE_BM01_DISK_DEVELOPMENT_LIMIT"
            );
        }
        assert_eq!(
            query_correctness(absent, absent, absent, profile)
                .unwrap_err()
                .code(),
            "USTE_BM01_DISK_DEVELOPMENT_LIMIT"
        );
        assert_eq!(
            create_crash_probe(absent, absent, profile, 1)
                .unwrap_err()
                .code(),
            "USTE_BM01_DISK_DEVELOPMENT_LIMIT"
        );
        assert_eq!(
            super::super::supervision::supervise_disk_sample(
                absent, absent, absent, absent, profile
            )
            .unwrap_err()
            .code(),
            "USTE_BM01_DISK_DEVELOPMENT_LIMIT"
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
            "disk-native-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let password = root.join("password");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&password)
            .unwrap();
        file.write_all(b"synthetic disk runner password, not a deployment credential")
            .unwrap();
        Self { root, password }
    }
    fn run(&self, phase: &str) -> Result<String, LinuxRunnerError> {
        run(
            &self.root,
            &self.password,
            Bm01Profile::new(20).unwrap(),
            phase,
        )
    }
    fn seed(&self, prefix: u8) {
        let mut fs = open_filesystem(&self.root).unwrap();
        let mut adapter =
            PortableRecoveryAdapter::new(credential::read_password(&self.password).unwrap());
        let vault = KeyVault::create(scope().database(), &mut adapter, OsEntropy).unwrap();
        let mut raw = CommitCoordinator::create(
            &mut fs,
            scope(),
            retention().unwrap(),
            EntryName::new("bm01-linux-disk-engine").unwrap(),
            vault,
            OsEntropy,
            GraphState::new(scope()),
        )
        .unwrap();
        if prefix == 0 {
            return;
        }
        if (7..=11).contains(&prefix) {
            let bytes = encode_transaction(&GraphTransaction::new(
                scope(),
                vec![Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Evidence(NewEvidence {
                        id: evidence_ref(scope()),
                        digest: [0; 32],
                        locator: text("unrelated-prefix").unwrap(),
                    }),
                }],
            ))
            .unwrap();
            raw.commit(
                &mut fs,
                TransactionRequest {
                    principal: if prefix == 8 {
                        uste_txn::PrincipalDigest::from_bytes([9; 32])
                    } else {
                        PRINCIPAL
                    },
                    idempotency_key: identity(
                        if prefix == 7 || prefix == 10 { 9 } else { 1 },
                        IdempotencyKey::from_bytes,
                    ),
                    transaction_id: identity(
                        if prefix == 9 || prefix == 10 { 9 } else { 1 },
                        TransactionId::from_bytes,
                    ),
                    canonical_request: &bytes,
                    blob_inventory: None,
                },
                &mut SystemClock::new(),
                &NeverCancel,
            )
            .unwrap();
            return;
        }
        install_policy(&mut raw, &mut fs).unwrap();
        if prefix == 12 {
            // Valid profile Evidence and final revision, but only one relationship. A binding
            // digest alone is not proof that every fixture record was materialized.
            visit_disk_batches(Bm01Profile::new(20).unwrap(), |sequence, mut operations| {
                if sequence > 2 {
                    operations.truncate(1);
                }
                let encoded =
                    encode_transaction(&GraphTransaction::new(scope(), operations)).unwrap();
                raw.commit(
                    &mut fs,
                    TransactionRequest {
                        principal: PRINCIPAL,
                        idempotency_key: identity(sequence, IdempotencyKey::from_bytes),
                        transaction_id: identity(sequence, TransactionId::from_bytes),
                        canonical_request: &encoded,
                        blob_inventory: None,
                    },
                    &mut SystemClock::new(),
                    &NeverCancel,
                )
                .unwrap();
                Ok(())
            })
            .unwrap();
            let snapshot = raw.read_view().unwrap().state().clone();
            uste_graph::publish_graph_state_root(&mut raw, &mut fs, &snapshot).unwrap();
            uste_txn::publish_coordinator_metadata_root(&mut raw, &mut fs).unwrap();
            uste_txn::publish_coordinator_transaction_index(&mut raw, &mut fs).unwrap();
            return;
        }
        if prefix == 1 {
            return;
        }
        if prefix != 5 {
            let snapshot = raw.read_view().unwrap().state().clone();
            uste_graph::publish_graph_state_root(&mut raw, &mut fs, &snapshot).unwrap();
            uste_txn::publish_coordinator_metadata_root(&mut raw, &mut fs).unwrap();
            uste_txn::publish_coordinator_transaction_index(&mut raw, &mut fs).unwrap();
        }
        let result = visit_disk_batches(Bm01Profile::new(20).unwrap(), |sequence, operations| {
            assert!(sequence == 2 || (prefix == 6 && sequence == 3) || prefix == 13);
            let encoded = encode_transaction(&GraphTransaction::new(scope(), operations)).unwrap();
            raw.commit(
                &mut fs,
                TransactionRequest {
                    principal: PRINCIPAL,
                    idempotency_key: identity(sequence, IdempotencyKey::from_bytes),
                    transaction_id: identity(sequence, TransactionId::from_bytes),
                    canonical_request: &encoded,
                    blob_inventory: None,
                },
                &mut SystemClock::new(),
                &NeverCancel,
            )
            .unwrap();
            if (prefix == 6 && sequence == 2) || prefix == 13 {
                Ok(())
            } else {
                Err("stop fixture at its selected certificate".into())
            }
        });
        assert_eq!(result.is_ok(), prefix == 13);
        if prefix == 3 || prefix == 4 {
            let snapshot = raw.read_view().unwrap().state().clone();
            uste_graph::publish_graph_state_root(&mut raw, &mut fs, &snapshot).unwrap();
        }
        if prefix == 4 {
            uste_txn::publish_coordinator_metadata_root(&mut raw, &mut fs).unwrap();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn native_disk_bootstrap_pending_and_partial_metadata_resume() {
    for prefix in 0..=4 {
        let fixture = Fixture::new();
        fixture.seed(prefix);
        assert!(fixture.run("open").is_err());
        let report = fixture
            .run("resume")
            .unwrap_or_else(|error| panic!("prefix {prefix}: {error}"));
        let json: serde_json::Value = serde_json::from_str(&report).unwrap();
        assert_eq!(json["frontier"], 4);
        assert_eq!(json["full_memory_graph_state"], false);
        assert_eq!(json["full_memory_coordinator_metadata"], false);
        assert_eq!(json["storage_metadata_memory_resident"], true);
        assert_eq!(
            json["suffix_recovery"]["certificate_anchor_residency"]["full_history_resident"],
            false
        );
        assert_eq!(
            json["suffix_recovery"]["certificate_anchor_residency"]["resident_entries"],
            0
        );
        assert_eq!(json["engine_benchmark"], false);
        assert_eq!(
            json["final_state_counts"],
            serde_json::json!([221, 421, 200, 200, 200, 600, 1, 1])
        );
        assert_eq!(
            json["cold_admission"]["graph_revision"],
            if prefix >= 3 { 2 } else { 1 }
        );
        assert_eq!(json["cold_admission"]["metadata_revision"], 1);
        let opened: serde_json::Value =
            serde_json::from_str(&fixture.run("open").unwrap()).unwrap();
        let admission = &opened["cold_admission"];
        assert_eq!(admission["graph_revision"], 4);
        assert_eq!(admission["metadata_revision"], 4);
        assert_eq!(admission["state_counts"], json["final_state_counts"]);
        assert_eq!(admission["scan_entries"], 1845);
        assert_eq!(admission["scan_runs"], 8);
        assert_eq!(admission["complete_authenticated_io"], false);
        assert_eq!(
            admission["measurement_scope"],
            "cold-graph-semantic-admission-only"
        );
        for field in [
            "scan_logical_bytes",
            "scan_pages_read",
            "exact_lookups",
            "predecessor_lookups",
            "semantic_reference_visits",
            "lookup_page_visits_including_cache_hits",
            "lookup_result_bytes",
            "peak_history_group_logical_bytes",
        ] {
            assert!(admission[field].as_u64().unwrap() > 0, "{field}");
        }
        fixture.run("resume").unwrap();
        assert_eq!(
            run(
                &fixture.root,
                &fixture.password,
                Bm01Profile::new(30).unwrap(),
                "resume"
            )
            .unwrap_err()
            .code(),
            "USTE_BM01_PROFILE_BINDING"
        );
        fixture.run("open").unwrap();
    }
}

#[test]
fn native_disk_binding_and_frontier_do_not_substitute_for_fixture_cardinality() {
    let fixture = Fixture::new();
    fixture.seed(12);
    for _ in 0..2 {
        assert_eq!(
            fixture.run("open").unwrap_err().code(),
            "USTE_BM01_FIXTURE_CARDINALITY"
        );
    }
}

#[test]
fn native_disk_create_and_missing_large_prefix_roots_fail_closed() {
    let fixture = Fixture::new();
    fixture.run("create").unwrap();
    fixture.run("open").unwrap();
    assert!(fixture.run("create").is_err());
    let missing = Fixture::new();
    missing.seed(5);
    assert_eq!(
        missing.run("resume").unwrap_err().code(),
        "USTE_BM01_DISK_ADMISSION"
    );
    assert_eq!(
        run(
            Path::new("unused"),
            Path::new("unused"),
            Bm01Profile::qualifying(),
            "create"
        )
        .unwrap_err()
        .code(),
        "USTE_BM01_DISK_DEVELOPMENT_LIMIT"
    );
}

#[test]
fn native_disk_multi_revision_suffix_repairs_only_on_resume_and_matches_oracle() {
    for (prefix, revisions) in [(6, 2), (13, 3)] {
        let fixture = Fixture::new();
        fixture.seed(prefix);
        // Open must not publish a terminal graph root. The subsequent resume report must
        // still observe base one and perform every missing graph revision itself.
        assert!(fixture.run("open").is_err());
        let resumed: serde_json::Value =
            serde_json::from_str(&fixture.run("resume").unwrap()).unwrap();
        assert_eq!(resumed["cold_admission"]["graph_revision"], 1);
        assert_eq!(resumed["cold_admission"]["metadata_revision"], 1);
        assert_eq!(resumed["suffix_recovery"]["revisions"], revisions);
        assert_eq!(resumed["suffix_recovery"]["maximum_revisions"], 3);
        assert_eq!(
            resumed["suffix_recovery"]["maximum_encoded_journal_bytes"],
            3 * 16_785_538_u64 + 6 * 4161
        );
        assert_eq!(resumed["frontier"], 4);
        assert_eq!(resumed["full_memory_graph_state"], false);
        assert_eq!(
            resumed["suffix_recovery"]["certificate_anchor_residency"]["full_history_resident"],
            false
        );
        assert_eq!(
            resumed["suffix_recovery"]["certificate_anchor_residency"]["resident_entries"],
            0
        );
        let opened: serde_json::Value =
            serde_json::from_str(&fixture.run("open").unwrap()).unwrap();
        assert_eq!(opened["suffix_recovery"]["revisions"], 0);
        assert_eq!(opened["cold_admission"]["metadata_revision"], 4);
        let profile = Bm01Profile::new(20).unwrap();
        let oracle = fixture.root.join("oracle-summary");
        fs::write(&oracle, OracleSummary::build(profile).unwrap().to_tsv()).unwrap();
        let queried: serde_json::Value = serde_json::from_str(
            &query_correctness(&fixture.root, &fixture.password, &oracle, profile).unwrap(),
        )
        .unwrap();
        assert_eq!(queried["successful_queries"], 384);
        let retry: serde_json::Value =
            serde_json::from_str(&fixture.run("resume").unwrap()).unwrap();
        assert_eq!(retry["suffix_recovery"]["revisions"], 0);
        assert_eq!(retry["frontier"], 4);
    }
}

#[test]
fn native_disk_queries_match_separate_summary_and_reject_substitution() {
    let fixture = Fixture::new();
    fixture.run("create").unwrap();
    let profile = Bm01Profile::new(20).unwrap();
    let summary = OracleSummary::build(profile).unwrap();
    let path = fixture.root.join("oracle-summary");
    fs::write(&path, summary.to_tsv()).unwrap();
    let report = query_correctness(&fixture.root, &fixture.password, &path, profile).unwrap();
    let json: serde_json::Value = serde_json::from_str(&report).unwrap();
    assert_eq!(json["queries"], 384);
    assert_eq!(json["successful_queries"], 384);
    assert_eq!(json["expected_visit_limits"], 0);
    assert_eq!(json["expected_result_limits"], 0);
    assert_eq!(json["frontier"], 4);
    assert_eq!(json["oracle_adjacency_memory_resident"], false);
    assert_eq!(json["full_memory_graph_state"], false);
    assert_eq!(json["full_memory_coordinator_metadata"], false);
    assert_eq!(json["storage_metadata_memory_resident"], true);
    assert_eq!(json["engine_benchmark"], false);
    assert_eq!(json["preemptive_deadline_enforced"], false);
    assert_eq!(json["cache_budget_bytes"], 64 * 1024 * 1024);
    let io = &json["query_adapter_io"];
    let index = &json["cached_index_work"];
    assert_eq!(index["failed_operations"], 0);
    assert_eq!(index["complete_authenticated_index_io"], false);
    assert!(index["completed_operations"].as_u64().unwrap() > 0);
    assert!(index["authenticated_pages_loaded"].as_u64().unwrap() > 0);
    assert_eq!(io["measurement_scope"], "filesystem-adapter-calls");
    assert_eq!(io["complete_authenticated_index_io"], false);
    assert!(io["read_returned_bytes"].as_u64().unwrap() > 0);
    assert!(io["operations"]["read_at"]["calls"].as_u64().unwrap() > 0);
    assert_eq!(io["operations"]["read_at"]["failures"], 0);
    assert_eq!(io["operations"]["write_at"]["calls"], 0);
    assert!(
        json["setup_adapter_io"]["read_returned_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(json["cache_hits"].as_u64().unwrap() > 0);
    assert!(json["cache_misses"].as_u64().unwrap() > 0);
    assert_eq!(json["oracle_summary_digest"], hex(&summary.digest()));
    let mut aggregate = blake3::Hasher::new_derive_key("USTE BM-01 linux-query-v1");
    let mut total_visits = 0;
    let mut total_bytes = 0;
    for expected in summary.expectations() {
        aggregate.update(&[
            expected.query.class.code(),
            expected.query.direction.code(),
            expected.query.depth,
        ]);
        aggregate.update(&expected.query.ordinal.to_be_bytes());
        let OracleExpectedOutcome::Output {
            visits,
            logical_result_bytes,
            output_digest,
            ..
        } = expected.outcome
        else {
            panic!("20/200 corpus must succeed");
        };
        total_visits += visits;
        total_bytes += logical_result_bytes;
        aggregate.update(&[1]);
        aggregate.update(&output_digest);
    }
    assert_eq!(json["output_digest"], hex(aggregate.finalize().as_bytes()));
    assert_eq!(json["visits"], total_visits);
    assert_eq!(json["logical_result_bytes"], total_bytes);

    // Component cache pressure, not a qualifying workload or an altered public cache budget.
    // Reopen the same native fixture and compare every query under a one-page cache.
    let pressured: serde_json::Value = serde_json::from_str(
        &query::query_correctness_with_cache_budget(
            &fixture.root,
            &fixture.password,
            &path,
            profile,
            uste_storage::MIN_INDEX_CACHE_BYTES,
        )
        .unwrap(),
    )
    .unwrap();
    for field in [
        "queries",
        "successful_queries",
        "expected_visit_limits",
        "expected_result_limits",
        "visits",
        "logical_result_bytes",
        "oracle_summary_digest",
        "output_digest",
    ] {
        assert_eq!(pressured[field], json[field], "{field}");
    }
    assert_eq!(
        pressured["cache_budget_bytes"],
        uste_storage::MIN_INDEX_CACHE_BYTES
    );
    assert!(pressured["cache_evictions"].as_u64().unwrap() > 0);
    assert!(
        pressured["cache_accounted_bytes"].as_u64().unwrap()
            <= u64::try_from(uste_storage::MIN_INDEX_CACHE_BYTES).unwrap()
    );
    assert_eq!(
        pressured["query_adapter_io"]["operations"]["write_at"]["calls"],
        0
    );

    fs::write(
        &path,
        OracleSummary::build_warmup(profile).unwrap().to_tsv(),
    )
    .unwrap();
    assert_eq!(
        query_correctness(Path::new("unused"), Path::new("unused"), &path, profile)
            .unwrap_err()
            .code(),
        "USTE_BM01_ORACLE_QUERY_SET"
    );
    fs::write(
        &path,
        OracleSummary::build(Bm01Profile::new(30).unwrap())
            .unwrap()
            .to_tsv(),
    )
    .unwrap();
    assert_eq!(
        query_correctness(Path::new("unused"), Path::new("unused"), &path, profile)
            .unwrap_err()
            .code(),
        "USTE_BM01_ORACLE_PROFILE"
    );
    fs::write(&path, b"truncated summary").unwrap();
    assert!(query_correctness(Path::new("unused"), Path::new("unused"), &path, profile).is_err());
}

#[test]
fn native_bootstrap_refuses_foreign_retry_identity_without_appending_policy() {
    for prefix in 7..=11 {
        let fixture = Fixture::new();
        fixture.seed(prefix);
        assert_eq!(
            fixture.run("resume").unwrap_err().code(),
            if prefix == 11 {
                "USTE_BM01_POLICY_COMMIT"
            } else {
                "USTE_BM01_BOOTSTRAP_PROFILE"
            }
        );
        let mut fs = open_filesystem(&fixture.root).unwrap();
        let mut adapter =
            PortableRecoveryAdapter::new(credential::read_password(&fixture.password).unwrap());
        let (_, report, transaction) = AuthenticatedIndexRecovery::open_with_frontier_transaction(
            &mut fs,
            &EntryName::new("bm01-linux-disk-engine").unwrap(),
            scope(),
            OsEntropy,
            OsEntropy,
            &mut adapter,
        )
        .unwrap();
        assert_eq!(report.frontier.unwrap().get(), 1);
        assert_eq!(transaction.unwrap().revision().get(), 1);
    }
}
