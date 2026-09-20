use super::*;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::PathBuf,
};
struct Fixture {
    root: PathBuf,
    password: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!(
                "packed-history-native-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let password = root.join("password");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&password)
            .unwrap()
            .write_all(b"synthetic packed history fixture password")
            .unwrap();
        Self { root, password }
    }
    fn run(&self, phase: &str) -> Result<serde_json::Value, LinuxRunnerError> {
        run(
            &self.root,
            &self.password,
            Bm06Profile::new(2).unwrap(),
            phase,
        )
        .map(|json| serde_json::from_str(&json).unwrap())
    }
    fn files(&self, roots: bool) -> BTreeMap<String, Vec<u8>> {
        fs::read_dir(self.root.join(HISTORY_DATABASE))
            .unwrap()
            .map(|entry| entry.unwrap())
            .filter_map(|entry| {
                let name = entry.file_name().into_string().unwrap();
                let included = if roots {
                    name.starts_with("p-")
                } else {
                    matches!(name.as_str(), "KEY" | "MANIFEST" | "CERTIFICATES")
                        || name.starts_with("j-")
                };
                included.then(|| (name, fs::read(entry.path()).unwrap()))
            })
            .collect()
    }
    fn remove_roots(&self) {
        let roots = self.files(true);
        assert!(!roots.is_empty());
        for name in roots.keys() {
            fs::remove_file(self.root.join(HISTORY_DATABASE).join(name)).unwrap();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn packed_history_native_phases_and_exact_tail_retry_preserve_authority() {
    let fixture = Fixture::new();
    let checkpoint = fixture.run("create").unwrap();
    assert_eq!(checkpoint["frontier"], 100);
    assert_eq!(checkpoint["verified_history_versions"], 198);
    assert_eq!(
        fixture.run("open").unwrap()["v1_state_digest"],
        checkpoint["v1_state_digest"]
    );
    let prefix = fixture.files(false);
    for name in ["KEY", "MANIFEST", "CERTIFICATES"] {
        assert!(prefix.contains_key(name));
    }
    assert!(prefix.keys().any(|name| name.starts_with("j-")));
    assert!(fixture.run("create").is_err());
    assert!(fixture.run("recover").is_err());
    assert!(
        fixture.files(false) == prefix,
        "checkpoint refusal changed source"
    );
    let tail = fixture.run("tail").unwrap();
    assert_eq!(tail["frontier"], 101);
    assert_eq!(tail["history_verified_through_revision"], 100);
    assert_eq!(tail["verified_history_versions"], 198);
    assert!(tail["v1_state_digest"].is_null());
    assert_eq!(tail["derived_terminal_pending"], true);
    let source = fixture.files(false);
    assert!(source["CERTIFICATES"].starts_with(&prefix["CERTIFICATES"]));
    assert!(fixture.run("tail").is_err());
    assert!(fixture.run("open").is_err());
    let recovered = fixture.run("recover").unwrap();
    assert_eq!(recovered["selected_base_revision"], 100);
    assert_eq!(recovered["suffix_groups"], 1);
    assert_eq!(recovered["verified_history_versions"], 200);
    let roots = fixture.files(true);
    let retried = fixture.run("recover").unwrap();
    assert_eq!(retried["selected_base_revision"], 101);
    assert_eq!(retried["suffix_groups"], 0);
    assert_eq!(retried["v1_state_digest"], recovered["v1_state_digest"]);
    assert!(
        fixture.files(true) == roots,
        "no-op recovery republished roots"
    );
    assert!(
        fixture.files(false) == source,
        "recovery/retry changed source"
    );
    let rebuilt = fixture.run("rebuild").unwrap();
    assert_eq!(rebuilt["origin_suffix_groups"], 100);
    assert_eq!(rebuilt["v1_state_digest"], recovered["v1_state_digest"]);
    assert_eq!(
        fixture.run("open").unwrap()["v1_state_digest"],
        recovered["v1_state_digest"]
    );
    assert!(
        fixture.files(false) == source,
        "origin recovery changed source"
    );
}

#[test]
fn packed_history_native_cache_loss_requires_explicit_origin_at_both_frontiers() {
    let fixture = Fixture::new();
    let checkpoint = fixture.run("create").unwrap();
    let source = fixture.files(false);
    fixture.remove_roots();
    assert!(fixture.run("open").is_err());
    let rebuilt = fixture.run("rebuild").unwrap();
    assert_eq!(rebuilt["origin_suffix_groups"], 99);
    assert_eq!(rebuilt["v1_state_digest"], checkpoint["v1_state_digest"]);
    assert!(
        fixture.files(false) == source,
        "checkpoint origin changed source"
    );
    fixture.run("tail").unwrap();
    let source = fixture.files(false);
    fixture.remove_roots();
    assert!(fixture.run("recover").is_err());
    assert!(fixture.run("open").is_err());
    let rebuilt = fixture.run("rebuild").unwrap();
    assert_eq!(rebuilt["origin_suffix_groups"], 100);
    assert_eq!(rebuilt["verified_history_versions"], 200);
    assert!(
        fixture.files(false) == source,
        "terminal origin changed source"
    );
}

#[test]
fn packed_history_native_wrong_profile_key_and_committed_corruption_fail_closed() {
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
        .write_all(b"wrong synthetic password")
        .unwrap();
    for phase in ["open", "tail", "recover", "rebuild", "resume"] {
        assert!(
            run(
                &fixture.root,
                &fixture.password,
                Bm06Profile::new(1).unwrap(),
                phase
            )
            .is_err()
        );
        assert!(run(&fixture.root, &wrong, Bm06Profile::new(2).unwrap(), phase).is_err());
        assert!(fixture.files(true) == roots, "wrong binding changed roots");
        assert!(
            fixture.files(false) == source,
            "wrong binding changed source"
        );
    }
    let path = fixture.root.join(HISTORY_DATABASE).join("CERTIFICATES");
    let mut bytes = fs::read(&path).unwrap();
    bytes[100 * 4161 + 137] ^= 1;
    fs::write(&path, &bytes).unwrap();
    for phase in ["open", "tail", "recover", "rebuild", "resume"] {
        assert!(fixture.run(phase).is_err());
        assert!(
            fs::read(&path).unwrap() == bytes,
            "committed corruption changed source"
        );
        assert!(
            fixture.files(true) == roots,
            "committed corruption changed roots"
        );
    }
}

#[test]
fn packed_history_native_admission_precedes_filesystem_access() {
    let absent = Path::new("absent-packed-history-admission");
    for records in [3, 100000] {
        for phase in [
            "create",
            "open",
            "tail",
            "recover",
            "rebuild",
            "resume",
            "tail-crash-probe",
        ] {
            assert_eq!(
                run(absent, absent, Bm06Profile::new(records).unwrap(), phase)
                    .unwrap_err()
                    .code(),
                "USTE_BM06_PACKED_DEVELOPMENT_LIMIT"
            );
        }
    }
    assert_eq!(
        run(absent, absent, Bm06Profile::new(2).unwrap(), "unexpected")
            .unwrap_err()
            .code(),
        "USTE_BM06_PACKED_PHASE"
    );
}

#[test]
fn packed_history_native_bootstrap_rejects_foreign_principal_key_and_request() {
    for variant in 0..3 {
        let fixture = Fixture::new();
        let profile = Bm06Profile::new(2).unwrap();
        let mut fs = ObservedFileSystem::new(open_filesystem(&fixture.root).unwrap());
        let mut adapter =
            PortableRecoveryAdapter::new(credential::read_password(&fixture.password).unwrap());
        let vault = KeyVault::create(scope().database(), &mut adapter, OsEntropy).unwrap();
        let mut raw = CommitCoordinator::create(
            &mut fs,
            scope(),
            retention().unwrap(),
            EntryName::new(HISTORY_DATABASE).unwrap(),
            vault,
            OsEntropy,
            GraphState::new(scope()),
        )
        .unwrap();
        let request = if variant == 2 {
            encode_transaction(&GraphTransaction::new(
                scope(),
                profile.batch(scope(), 2).unwrap(),
            ))
            .unwrap()
        } else {
            policy_bytes().unwrap()
        };
        raw.commit(
            &mut fs,
            TransactionRequest {
                principal: if variant == 0 {
                    uste_policy::PrincipalDigest::from_bytes([99; 32])
                } else {
                    PRINCIPAL
                },
                idempotency_key: if variant == 1 {
                    IdempotencyKey::from_bytes([99; 16])
                } else {
                    bootstrap_id(profile, IdempotencyKey::from_bytes)
                },
                transaction_id: bootstrap_id(profile, TransactionId::from_bytes),
                canonical_request: &request,
                blob_inventory: None,
            },
            &mut SystemClock::new(),
            &NeverCancel,
        )
        .unwrap();
        drop(raw);
        drop(fs);
        let source = fixture.files(false);
        assert!(fixture.run("resume").is_err());
        assert!(
            fixture.files(false) == source,
            "rejected bootstrap changed source"
        );
        assert!(
            fixture.files(true).is_empty(),
            "rejected bootstrap published roots"
        );
    }
}

#[test]
fn packed_history_native_selected_pack_corruption_never_falls_back() {
    let fixture = Fixture::new();
    fixture.run("create").unwrap();
    fixture.run("tail").unwrap();
    let source = fixture.files(false);
    let roots = fixture.files(true);
    let mut corrupted = 0;
    for entry in fs::read_dir(fixture.root.join(HISTORY_DATABASE)).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name().to_str().unwrap().starts_with("pack-") {
            let mut bytes = fs::read(entry.path()).unwrap();
            bytes[137] ^= 1;
            fs::write(entry.path(), bytes).unwrap();
            corrupted += 1;
        }
    }
    assert!(corrupted > 0);
    assert!(fixture.run("recover").is_err());
    assert!(
        fixture.files(true) == roots,
        "failed selected admission published roots"
    );
    assert!(
        fixture.files(false) == source,
        "failed selected admission changed source"
    );
    let rebuilt = fixture.run("rebuild").unwrap();
    assert_eq!(rebuilt["verified_history_versions"], 200);
    assert_eq!(
        fixture.run("open").unwrap()["v1_state_digest"],
        rebuilt["v1_state_digest"]
    );
    assert!(
        fixture.files(false) == source,
        "explicit pack repair changed source"
    );
}
