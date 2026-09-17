//! Release-build encrypted streaming/RSS probe for the T-15 blob profile.

#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::{
    error::Error,
    fs::{self, File},
    os::fd::OwnedFd,
    path::Path,
    time::Instant,
};

use sha2::{Digest, Sha256};
use uste_crypto::{KeyVault, OsEntropy, PortableRecoveryAdapter, RecoveryPassword};
use uste_storage::{
    BLOB_CHUNK_BYTES, BlobInventory, EntryName,
    journal::{CommitInput, CreationOptions, JournalStore},
    linux::LinuxFileSystem,
};
use uste_types::{DatabaseId, NamespaceId, NamespaceRef};

const PASSWORD: &[u8] = b"uste-t15-local-stream-probe";

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let root = arguments
        .next()
        .ok_or("usage: blob_stream_probe ROOT BYTES")?;
    let logical_bytes: u64 = arguments
        .next()
        .ok_or("usage: blob_stream_probe ROOT BYTES")?
        .to_string_lossy()
        .parse()?;
    if arguments.next().is_some() || logical_bytes == 0 {
        return Err("usage: blob_stream_probe ROOT BYTES (BYTES must be nonzero)".into());
    }
    let root = Path::new(&root);
    let database = DatabaseId::from_bytes([0xd1; 16]);
    let scope = NamespaceRef::new(database, NamespaceId::from_bytes([0xd2; 16]));
    let database_name = EntryName::new("stream-probe")?;
    let started = Instant::now();

    let mut filesystem = open_filesystem(root)?;
    let mut adapter = password_adapter()?;
    let vault = KeyVault::create(database, &mut adapter, OsEntropy)?;
    let mut store = JournalStore::create(
        &mut filesystem,
        CreationOptions {
            database,
            final_name: database_name.clone(),
        },
        vault,
        OsEntropy,
    )?;
    let mut upload = store.start_blob_upload(scope)?;
    let mut block = vec![0_u8; BLOB_CHUNK_BYTES];
    for (index, byte) in block.iter_mut().enumerate() {
        *byte = u8::try_from((index.wrapping_mul(131) + 17) % 251)?;
    }
    let mut expected = Sha256::new();
    let mut written = 0_u64;
    while written < logical_bytes {
        let count =
            usize::try_from((logical_bytes - written).min(u64::try_from(BLOB_CHUNK_BYTES)?))?;
        store.write_blob_upload(&mut filesystem, &mut upload, &block[..count])?;
        expected.update(&block[..count]);
        written += u64::try_from(count)?;
    }
    let reference = store.finish_blob_upload(&mut filesystem, &mut upload)?;
    let inventory = BlobInventory::new(scope, [reference])?;
    store.append_group_with_inventory(
        &mut filesystem,
        CommitInput {
            encoded_group: b"T-15 release-build stream probe",
            logical_event_digest: [0xd3; 32],
        },
        &inventory,
    )?;
    let ingest_seconds = started.elapsed().as_secs_f64();
    drop(store);
    drop(filesystem);

    let reopen_started = Instant::now();
    let mut filesystem = open_filesystem(root)?;
    let mut adapter = password_adapter()?;
    let (store, report) = JournalStore::open(
        &mut filesystem,
        &database_name,
        database,
        OsEntropy,
        OsEntropy,
        &mut adapter,
        |group| {
            if group.blob_inventory_digest != inventory.digest()
                || group.blob_inventory != Some(&inventory)
            {
                return Err(uste_storage::journal::StorageError::IntegrityFailure);
            }
            Ok(())
        },
    )?;
    if report.frontier.is_none() {
        return Err("probe commit was not recovered".into());
    }
    let recovery_seconds = reopen_started.elapsed().as_secs_f64();

    let verify_started = Instant::now();
    let mut actual = Sha256::new();
    let mut output = vec![0_u8; BLOB_CHUNK_BYTES];
    let mut offset = 0_u64;
    loop {
        let count = store.read_blob_range(&mut filesystem, reference, offset, &mut output)?;
        if count == 0 {
            break;
        }
        actual.update(&output[..count]);
        offset += u64::try_from(count)?;
    }
    let actual: [u8; 32] = actual.finalize().into();
    let expected: [u8; 32] = expected.finalize().into();
    if offset != logical_bytes || actual != expected || reference.content_digest() != expected {
        return Err("probe round-trip digest mismatch".into());
    }
    let verify_seconds = verify_started.elapsed().as_secs_f64();
    let disk_bytes = directory_bytes(root)?;
    println!(
        "{{\"logical_bytes\":{logical_bytes},\"disk_bytes\":{disk_bytes},\"ingest_seconds\":{ingest_seconds:.6},\"recovery_seconds\":{recovery_seconds:.6},\"verify_seconds\":{verify_seconds:.6},\"sha256\":\"{}\"}}",
        hex(&actual)
    );
    Ok(())
}

fn open_filesystem(root: &Path) -> Result<LinuxFileSystem, Box<dyn Error>> {
    let root: OwnedFd = File::open(root)?.into();
    Ok(LinuxFileSystem::from_directory(root)?)
}

fn password_adapter() -> Result<PortableRecoveryAdapter, Box<dyn Error>> {
    Ok(PortableRecoveryAdapter::new(RecoveryPassword::new(
        PASSWORD.to_vec(),
    )?))
}

fn directory_bytes(path: &Path) -> Result<u64, Box<dyn Error>> {
    let mut total = 0_u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            total = total
                .checked_add(directory_bytes(&entry.path())?)
                .ok_or("disk byte total overflow")?;
        } else if metadata.is_file() {
            total = total
                .checked_add(metadata.len())
                .ok_or("disk byte total overflow")?;
        }
    }
    Ok(total)
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}
