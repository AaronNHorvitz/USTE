use super::*;
use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf};

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
        install_policy(&mut raw, &mut fs).unwrap();
        if prefix == 1 {
            return;
        }
        if prefix != 5 {
            let snapshot = raw.read_view().unwrap().state().clone();
            uste_graph::publish_graph_state_root(&mut raw, &mut fs, &snapshot).unwrap();
            uste_txn::publish_coordinator_metadata_root(&mut raw, &mut fs).unwrap();
            uste_txn::publish_coordinator_transaction_index(&mut raw, &mut fs).unwrap();
        }
        let result =
            visit_development_batches(Bm01Profile::new(20).unwrap(), |sequence, operations| {
                assert!(sequence == 2 || (prefix == 6 && sequence == 3));
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
                if prefix == 6 && sequence == 2 {
                    Ok(())
                } else {
                    Err("stop fixture at its selected certificate".into())
                }
            });
        assert!(result.is_err());
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
        assert_eq!(json["engine_benchmark"], false);
        fixture.run("open").unwrap();
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
    let oversized_suffix = Fixture::new();
    oversized_suffix.seed(6);
    assert_eq!(
        oversized_suffix.run("resume").unwrap_err().code(),
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
