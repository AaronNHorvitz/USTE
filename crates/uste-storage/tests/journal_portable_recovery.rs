use uste_crypto::{
    CryptoError, KeyVault, OsEntropy, PortableRecoveryAdapter, RecoveryEnvelope, RecoveryPassword,
};
use uste_storage::{
    EntryName,
    journal::{CommitInput, CreationOptions, JournalStore, StorageError},
    memory::MemoryFileSystem,
};
use uste_types::DatabaseId;

fn password(bytes: &[u8]) -> PortableRecoveryAdapter {
    PortableRecoveryAdapter::new(RecoveryPassword::new(bytes.to_vec()).unwrap())
}

#[test]
fn portable_recovery_wrapper_is_persisted_and_unlocks_the_real_journal() {
    let database = DatabaseId::from_bytes([0xd1; 16]);
    let mut filesystem = MemoryFileSystem::default();
    let mut creation_adapter = password(b"correct portable recovery credential");
    let vault: KeyVault<RecoveryEnvelope, OsEntropy> =
        KeyVault::create(database, &mut creation_adapter, OsEntropy).unwrap();
    let mut store = JournalStore::create(
        &mut filesystem,
        CreationOptions {
            database,
            final_name: EntryName::new("portable").unwrap(),
        },
        vault,
        OsEntropy,
    )
    .unwrap();
    store
        .append_group(
            &mut filesystem,
            CommitInput {
                encoded_group: b"real portable wrapper and object encryption\0\xff",
                logical_event_digest: [0xd2; 32],
            },
        )
        .unwrap();
    drop(store);
    filesystem.restart().unwrap();

    let wrong = JournalStore::open(
        &mut filesystem,
        &EntryName::new("portable").unwrap(),
        database,
        OsEntropy,
        OsEntropy,
        &mut password(b"wrong portable recovery credential"),
        |_group| Ok(()),
    )
    .unwrap_err();
    assert_eq!(wrong, StorageError::Crypto(CryptoError::KeyUnavailable));

    let mut replayed = Vec::new();
    let (_store, report) = JournalStore::open(
        &mut filesystem,
        &EntryName::new("portable").unwrap(),
        database,
        OsEntropy,
        OsEntropy,
        &mut password(b"correct portable recovery credential"),
        |group| {
            assert_eq!(group.logical_event_digest, [0xd2; 32]);
            replayed.push(group.encoded_group.to_vec());
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(report.frontier.map(|revision| revision.get()), Some(1));
    assert_eq!(
        replayed,
        vec![b"real portable wrapper and object encryption\0\xff".to_vec()]
    );
}
