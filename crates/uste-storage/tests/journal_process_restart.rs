#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::process::ExitStatusExt,
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
use uste_storage::{
    EntryName,
    journal::{CommitInput, CreationOptions, DurableKeyEnvelope, JournalStore},
    linux::LinuxFileSystem,
};
use uste_types::DatabaseId;

const CHILD_MARKER: &str = "USTE_T13_JOURNAL_CHILD";
const DIRECTORY_PATH: &str = "USTE_T13_JOURNAL_DIRECTORY";

#[test]
fn committed_certificate_survives_writer_sigkill_and_replays_exact_bytes() {
    if std::env::var_os(CHILD_MARKER).is_some() {
        child_writer();
        return;
    }

    let scratch = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(&scratch).unwrap();
    let discriminator = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = scratch.join(format!(
        "uste-t13-journal-{}-{discriminator}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    let directory_guard = TestDirectory(directory.clone());
    let database = DatabaseId::from_bytes([0xa1; 16]);

    let mut filesystem = open_filesystem(&directory);
    let store = JournalStore::create(
        &mut filesystem,
        CreationOptions {
            database,
            final_name: name("world"),
        },
        create_vault(database, 10),
        CounterEntropy::new(100),
    )
    .unwrap();
    drop(store);
    drop(filesystem);

    let executable = std::env::current_exe().unwrap();
    let child = Command::new(executable)
        .arg("--exact")
        .arg("committed_certificate_survives_writer_sigkill_and_replays_exact_bytes")
        .arg("--nocapture")
        .env(CHILD_MARKER, "1")
        .env(DIRECTORY_PATH, &directory)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = ChildGuard::new(child);
    let mut readiness = child.stderr().take().unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut byte = [0_u8; 1];
        let result = readiness.read_exact(&mut byte).map(|()| byte);
        let _ = sender.send(result);
    });
    assert_eq!(
        receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("child journal readiness timed out")
            .expect("child journal readiness pipe closed"),
        [b'R']
    );
    let status = child.kill_and_wait();
    assert_eq!(status.signal(), Some(9));

    let mut filesystem = open_filesystem(&directory);
    let mut replayed = Vec::new();
    let (_store, report) = JournalStore::open(
        &mut filesystem,
        &name("world"),
        database,
        CounterEntropy::new(30),
        CounterEntropy::new(300),
        &mut TestKeyAdapter,
        |group| {
            replayed.push((group.revision.get(), group.encoded_group.to_vec()));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(report.frontier.map(|revision| revision.get()), Some(1));
    assert_eq!(
        replayed,
        vec![(1, b"durable before process death\0\xff".to_vec())]
    );
    drop(directory_guard);
}

fn child_writer() {
    let directory = PathBuf::from(std::env::var_os(DIRECTORY_PATH).unwrap());
    let database = DatabaseId::from_bytes([0xa1; 16]);
    let mut filesystem = open_filesystem(&directory);
    let (mut store, report) = JournalStore::open(
        &mut filesystem,
        &name("world"),
        database,
        CounterEntropy::new(20),
        CounterEntropy::new(200),
        &mut TestKeyAdapter,
        |_group| Ok(()),
    )
    .unwrap();
    assert_eq!(report.frontier, None);
    store
        .append_group(
            &mut filesystem,
            CommitInput {
                encoded_group: b"durable before process death\0\xff",
                logical_event_digest: [0xa2; 32],
            },
        )
        .unwrap();

    std::io::stderr().write_all(b"R").unwrap();
    std::io::stderr().flush().unwrap();
    loop {
        thread::sleep(Duration::from_millis(50));
    }
}

fn open_filesystem(directory: &PathBuf) -> LinuxFileSystem {
    let root: std::os::fd::OwnedFd = File::open(directory).unwrap().into();
    LinuxFileSystem::from_directory(root).unwrap()
}

fn name(value: &str) -> EntryName {
    EntryName::new(value).unwrap()
}

#[derive(Debug)]
struct TestEnvelope([u8; 32]);

impl DurableKeyEnvelope for TestEnvelope {
    fn encode_durable(&self) -> Result<Vec<u8>, CryptoError> {
        Ok(self.0.to_vec())
    }

    fn decode_durable(encoded: &[u8]) -> Result<Self, CryptoError> {
        Ok(Self(
            encoded
                .try_into()
                .map_err(|_| CryptoError::InvalidEnvelope)?,
        ))
    }
}

#[derive(Debug)]
struct TestKeyAdapter;

impl KeyAdapter for TestKeyAdapter {
    type Envelope = TestEnvelope;

    fn wrap(
        &mut self,
        _database: DatabaseId,
        key: &SecretKeyMaterial,
        _entropy: &mut dyn EntropySource,
    ) -> Result<Self::Envelope, CryptoError> {
        Ok(TestEnvelope(*key.expose_to_adapter()))
    }

    fn unwrap(
        &mut self,
        _database: DatabaseId,
        envelope: &Self::Envelope,
    ) -> Result<SecretKeyMaterial, CryptoError> {
        Ok(SecretKeyMaterial::from_adapter_bytes(envelope.0))
    }
}

#[derive(Debug)]
struct CounterEntropy(u64);

impl CounterEntropy {
    fn new(seed: u64) -> Self {
        Self(seed)
    }
}

impl EntropySource for CounterEntropy {
    fn fill(&mut self, output: &mut [u8]) -> Result<(), EntropyFailure> {
        self.0 = self.0.checked_add(1).ok_or(EntropyFailure)?;
        let mut block = self.0;
        for chunk in output.chunks_mut(8) {
            let bytes = block.to_be_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
            block = block.checked_add(1).ok_or(EntropyFailure)?;
        }
        Ok(())
    }
}

fn create_vault(database: DatabaseId, seed: u64) -> KeyVault<TestEnvelope, CounterEntropy> {
    KeyVault::create(database, &mut TestKeyAdapter, CounterEntropy::new(seed)).unwrap()
}

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn stderr(&mut self) -> &mut Option<std::process::ChildStderr> {
        &mut self.0.as_mut().unwrap().stderr
    }

    fn kill_and_wait(&mut self) -> ExitStatus {
        let mut child = self.0.take().unwrap();
        child.kill().unwrap();
        child.wait().unwrap()
    }
}

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
        let _ = fs::remove_dir_all(&self.0);
    }
}
