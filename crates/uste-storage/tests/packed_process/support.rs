use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyEpoch, KeyVault, SecretKeyMaterial,
    WriterIncarnationId,
};
use uste_storage::{
    AdapterError, EntryName, FileMetadata, FileSystem, IndexDelta,
    journal::StorageError,
    linux::{LinuxDirectory, LinuxFile, LinuxFileSystem, LinuxFilesystemProfile},
    ordered_commitment::{self, CommitmentContext},
    packed_index_pack::PackWriteLimits,
    packed_index_page::{ENCODED_PAGE_BYTES, PackedPageContext},
    packed_root_manifest::*,
    packed_tree_batch::{StagedTreeBatch, TreeBatchLimits, stage_batch},
    packed_tree_cursor::{PackedTreeCursor, TreeCursorLimits},
    packed_tree_lookup::TreeReadContext,
    packed_tree_validation::{TreeValidationLimits, validate_tree},
    read_exact_at, write_all_at,
};
use uste_types::{CommitRevision, DatabaseId, NamespaceId, NamespaceRef};

const TEST_NAME: &str = "packed_tree_process_sigkill_preserves_old_and_synced_new_roots";
const DIRECTORY: &str = "USTE_PACKED_PROCESS_DIRECTORY";
const PHASE: &str = "USTE_PACKED_PROCESS_PHASE";
struct Adapter;
impl KeyAdapter for Adapter {
    type Envelope = [u8; 32];
    fn wrap(
        &mut self,
        _: DatabaseId,
        key: &SecretKeyMaterial,
        _: &mut dyn EntropySource,
    ) -> Result<Self::Envelope, CryptoError> {
        Ok(*key.expose_to_adapter())
    }
    fn unwrap(
        &mut self,
        _: DatabaseId,
        key: &Self::Envelope,
    ) -> Result<SecretKeyMaterial, CryptoError> {
        Ok(SecretKeyMaterial::from_adapter_bytes(*key))
    }
}
struct Entropy(u64);
impl EntropySource for Entropy {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), EntropyFailure> {
        self.0 += 1;
        for (i, chunk) in out.chunks_mut(8).enumerate() {
            chunk.copy_from_slice(&(self.0 + i as u64).to_be_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}
fn view(revision: u64) -> TreeReadContext {
    TreeReadContext {
        scope: NamespaceRef::new(
            DatabaseId::from_bytes([1; 16]),
            NamespaceId::from_bytes([2; 16]),
        ),
        profile: [3; 32],
        family: 4,
        revision: CommitRevision::new(revision).unwrap(),
    }
}
fn root_context(revision: u64) -> PackedRootContext {
    let c = view(revision);
    PackedRootContext {
        scope: c.scope,
        profile: c.profile,
        epoch: KeyEpoch::FIRST,
        writer: WriterIncarnationId::from_bytes([20 + revision as u8; 16]),
        object: [80 + revision as u8; 16],
    }
}
fn name(s: &str) -> EntryName {
    EntryName::new(s).unwrap()
}
fn open(path: &Path) -> LinuxFileSystem {
    let directory = File::open(path).unwrap().into();
    match std::env::var("USTE_T13_TEST_PROFILE").as_deref() {
        Ok("ext4") => LinuxFileSystem::from_directory_for_profile(
            directory,
            LinuxFilesystemProfile::Ext4Candidate,
        )
        .unwrap(),
        Err(std::env::VarError::NotPresent) => LinuxFileSystem::from_directory(directory).unwrap(),
        other => panic!("unsupported test profile: {other:?}"),
    }
}
fn values(revision: u64) -> Vec<(Vec<u8>, Vec<u8>)> {
    (1..=32_u8)
        .map(|key| {
            (
                vec![key],
                if revision == 2 && key == 16 {
                    vec![99; 50_000]
                } else {
                    vec![key; 128]
                },
            )
        })
        .collect()
}
fn limits() -> TreeBatchLimits {
    TreeBatchLimits {
        maximum_deltas: 64,
        maximum_input_bytes: 1024 * 1024,
        maximum_dirty_nodes: 2048,
        maximum_path_branches: 128,
        maximum_read_pages: 1024,
        maximum_read_bytes: 1024 * ENCODED_PAGE_BYTES as u64,
        pack: PackWriteLimits {
            maximum_pages: 128,
            maximum_records: 1024,
            maximum_payload_bytes: 1024 * 1024,
        },
    }
}
fn manifest_bytes(
    vault: &mut KeyVault<[u8; 32], Entropy>,
    stage: &StagedTreeBatch,
    revision: u64,
) -> Vec<u8> {
    seal_manifest(
        vault,
        root_context(revision),
        PackedRootClaims {
            revision: view(revision).revision,
            generation: revision,
            certificate_digest: [revision as u8; 32],
            reducer_profile: [30; 32],
            state_commitment_profile: [31; 32],
            state_digest: [32; 32],
        },
        &[PackedRootFamily {
            family: 4,
            commitment: stage.logical_root(),
            root: stage.root().map(|r| r.location),
        }],
    )
    .unwrap()
}
fn write_synced<F: FileSystem>(fs: &mut F, filename: &str, bytes: &[u8]) {
    let directory = fs.root();
    let file = fs.create_new(&directory, &name(filename)).unwrap();
    write_all_at(fs, &file, 0, bytes).unwrap();
    fs.set_len(&file, bytes.len() as u64).unwrap();
    fs.sync_all(&file).unwrap();
    fs.sync_directory(&directory).unwrap();
}
fn read_manifest<F: FileSystem>(
    fs: &mut F,
    vault: &KeyVault<[u8; 32], Entropy>,
    revision: u64,
) -> Result<PackedRootManifest, StorageError> {
    let directory = fs.root();
    let file = fs.open_existing(
        &directory,
        &name(if revision == 1 {
            "fixture-old"
        } else {
            "fixture-new"
        }),
    )?;
    if fs.metadata(&file)?.len != ENCODED_MANIFEST_BYTES as u64 {
        return Err(StorageError::IntegrityFailure);
    }
    let mut encoded = vec![0; ENCODED_MANIFEST_BYTES];
    read_exact_at(fs, &file, 0, &mut encoded)?;
    open_manifest(vault, root_context(revision), &encoded)
}
fn vault(path: &Path, seed: u64) -> KeyVault<[u8; 32], Entropy> {
    let wrapped: [u8; 32] = fs::read(path.join("test-wrapped-key"))
        .unwrap()
        .try_into()
        .unwrap();
    let mut vault = KeyVault::from_locked(view(1).scope.database(), wrapped, Entropy(seed));
    vault.unlock(&mut Adapter).unwrap();
    vault
}
fn verify(fs: &mut LinuxFileSystem, vault: &KeyVault<[u8; 32], Entropy>, revision: u64) {
    let manifest = read_manifest(fs, vault, revision).unwrap();
    let family = manifest.families()[0];
    let directory = fs.root();
    let receipt = validate_tree(
        fs,
        &directory,
        vault,
        view(revision),
        family.commitment,
        family.root,
        TreeValidationLimits {
            maximum_path_branches: 128,
            maximum_nodes: 128,
            maximum_logical_bytes: 1024 * 1024,
            maximum_pages: 1024,
            maximum_encoded_bytes: 1024 * ENCODED_PAGE_BYTES as u64,
        },
    )
    .unwrap();
    assert_eq!(receipt.report().entries, 32);
    let mut cursor = PackedTreeCursor::new(
        view(revision),
        family.commitment,
        family.root,
        b"",
        None,
        TreeCursorLimits {
            maximum_path_branches: 128,
            maximum_candidates: 128,
            maximum_returned_bytes: 1024 * 1024,
            maximum_pages: 1024,
            maximum_encoded_bytes: 1024 * ENCODED_PAGE_BYTES as u64,
        },
    )
    .unwrap();
    let mut actual = Vec::new();
    while let Some(entry) = cursor.next(fs, &directory, vault).unwrap() {
        actual.push((entry.key().to_vec(), entry.value().to_vec()));
    }
    assert_eq!(actual, values(revision));
}
fn ready_and_wait() -> ! {
    std::io::stderr().write_all(b"R").unwrap();
    std::io::stderr().flush().unwrap();
    // Parent retains the pipe and kills this child. EOF also exits if the parent disappears.
    let mut byte = [0];
    let _ = std::io::stdin().read_exact(&mut byte);
    std::process::exit(78)
}
fn child(path: &Path, phase: &str) {
    let mut fs = PauseFileSystem {
        inner: open(path),
        pause_after_write: phase == "pack-write",
    };
    let mut vault = vault(path, 1000);
    let old = read_manifest(&mut fs, &vault, 1).unwrap();
    let family = old.families()[0];
    let c = view(1);
    let directory = fs.root();
    let stage = stage_batch(
        &mut fs,
        &directory,
        &mut vault,
        &mut Entropy(9000),
        c,
        family.commitment,
        family.root,
        &[IndexDelta::new(vec![16], Some(vec![16; 128]), Some(vec![99; 50_000])).unwrap()],
        PackedPageContext {
            scope: c.scope,
            profile: c.profile,
            family: c.family,
            creation_revision: view(2).revision,
            epoch: KeyEpoch::FIRST,
            writer: WriterIncarnationId::from_bytes([12; 16]),
            object: [0; 16],
            page: 0,
        },
        limits(),
    )
    .unwrap();
    assert!(stage.report().written_nodes < 63);
    if phase == "pack-synced" {
        ready_and_wait();
    }
    let encoded = manifest_bytes(&mut vault, &stage, 2);
    if phase == "manifest-partial" {
        let file = fs.create_new(&directory, &name("fixture-new")).unwrap();
        write_all_at(&mut fs, &file, 0, &encoded[..encoded.len() / 2]).unwrap();
        fs.sync_all(&file).unwrap();
        fs.sync_directory(&directory).unwrap();
        ready_and_wait();
    }
    assert_eq!(phase, "manifest-synced");
    write_synced(&mut fs, "fixture-new", &encoded);
    ready_and_wait();
}
pub fn run() {
    if let Some(phase) = std::env::var_os(PHASE) {
        child(
            &PathBuf::from(std::env::var_os(DIRECTORY).unwrap()),
            &phase.to_string_lossy(),
        );
        return;
    }
    let scratch = std::env::var_os("USTE_T13_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")));
    fs::create_dir_all(&scratch).unwrap();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    for phase in [
        "pack-write",
        "pack-synced",
        "manifest-partial",
        "manifest-synced",
    ] {
        let path = scratch.join(format!(
            "uste-packed-process-{}-{stamp}-{phase}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        let guard = TestDirectory(path.clone());
        let mut fs = open(&path);
        let mut key =
            KeyVault::create(view(1).scope.database(), &mut Adapter, Entropy(10)).unwrap();
        write_synced(&mut fs, "test-wrapped-key", key.wrapped());
        let deltas: Vec<_> = values(1)
            .into_iter()
            .map(|(key, value)| IndexDelta::new(key, None, Some(value)).unwrap())
            .collect();
        let c = view(1);
        let empty = ordered_commitment::empty_commitment(
            CommitmentContext::new(c.scope, c.profile, c.family).unwrap(),
        );
        let directory = fs.root();
        let initial = stage_batch(
            &mut fs,
            &directory,
            &mut key,
            &mut Entropy(5000),
            c,
            empty,
            None,
            &deltas,
            PackedPageContext {
                scope: c.scope,
                profile: c.profile,
                family: c.family,
                creation_revision: c.revision,
                epoch: KeyEpoch::FIRST,
                writer: WriterIncarnationId::from_bytes([11; 16]),
                object: [0; 16],
                page: 0,
            },
            limits(),
        )
        .unwrap();
        let encoded = manifest_bytes(&mut key, &initial, 1);
        write_synced(&mut fs, "fixture-old", &encoded);
        drop(key);
        drop(fs);
        let process = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env(PHASE, phase)
            .env(DIRECTORY, &path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut child = ChildGuard(Some(process));
        let mut pipe = child.0.as_mut().unwrap().stderr.take().unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut byte = [0];
            let result = pipe.read_exact(&mut byte).map(|()| byte);
            let _ = sender.send(result);
        });
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(10))
                .expect("packed child readiness timeout")
                .expect("packed child pipe closed"),
            [b'R']
        );
        let process = child.0.as_mut().unwrap();
        process.kill().unwrap();
        let status = process.wait().unwrap();
        child.0.take();
        assert_eq!(status.signal(), Some(9));
        let mut fs = open(&path);
        let key = vault(&path, 2000);
        verify(&mut fs, &key, 1);
        if phase == "manifest-synced" {
            verify(&mut fs, &key, 2);
        } else {
            assert!(read_manifest(&mut fs, &key, 2).is_err());
        }
        drop(key);
        drop(fs);
        drop(guard);
    }
}
struct ChildGuard(Option<Child>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
struct TestDirectory(PathBuf);
impl Drop for TestDirectory {
    fn drop(&mut self) {
        // Only this uniquely created test directory, and only its known fixture file classes.
        if let Ok(entries) = fs::read_dir(&self.0) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("pack-")
                    || ["fixture-old", "fixture-new", "test-wrapped-key"].contains(&name.as_ref())
                {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        let _ = fs::remove_dir(&self.0);
    }
}

struct PauseFileSystem {
    inner: LinuxFileSystem,
    pause_after_write: bool,
}
impl FileSystem for PauseFileSystem {
    type File = LinuxFile;
    type Directory = LinuxDirectory;
    fn root(&self) -> Self::Directory {
        self.inner.root()
    }
    fn create_directory(
        &mut self,
        d: &Self::Directory,
        n: &EntryName,
    ) -> Result<Self::Directory, AdapterError> {
        self.inner.create_directory(d, n)
    }
    fn open_directory(
        &mut self,
        d: &Self::Directory,
        n: &EntryName,
    ) -> Result<Self::Directory, AdapterError> {
        self.inner.open_directory(d, n)
    }
    fn create_new(
        &mut self,
        d: &Self::Directory,
        n: &EntryName,
    ) -> Result<Self::File, AdapterError> {
        self.inner.create_new(d, n)
    }
    fn open_existing(
        &mut self,
        d: &Self::Directory,
        n: &EntryName,
    ) -> Result<Self::File, AdapterError> {
        self.inner.open_existing(d, n)
    }
    fn metadata(&mut self, f: &Self::File) -> Result<FileMetadata, AdapterError> {
        self.inner.metadata(f)
    }
    fn read_at(&mut self, f: &Self::File, o: u64, b: &mut [u8]) -> Result<usize, AdapterError> {
        self.inner.read_at(f, o, b)
    }
    fn write_at(&mut self, f: &Self::File, o: u64, b: &[u8]) -> Result<usize, AdapterError> {
        let written = self.inner.write_at(f, o, b)?;
        if self.pause_after_write {
            assert!(written > 0 && written <= b.len());
            ready_and_wait();
        }
        Ok(written)
    }
    fn set_len(&mut self, f: &Self::File, n: u64) -> Result<(), AdapterError> {
        self.inner.set_len(f, n)
    }
    fn sync_data(&mut self, f: &Self::File) -> Result<(), AdapterError> {
        self.inner.sync_data(f)
    }
    fn sync_all(&mut self, f: &Self::File) -> Result<(), AdapterError> {
        self.inner.sync_all(f)
    }
    fn rename_no_replace(
        &mut self,
        a: &Self::Directory,
        b: &EntryName,
        c: &Self::Directory,
        d: &EntryName,
    ) -> Result<(), AdapterError> {
        self.inner.rename_no_replace(a, b, c, d)
    }
    fn remove_file(&mut self, d: &Self::Directory, n: &EntryName) -> Result<(), AdapterError> {
        self.inner.remove_file(d, n)
    }
    fn sync_directory(&mut self, d: &Self::Directory) -> Result<(), AdapterError> {
        self.inner.sync_directory(d)
    }
}
