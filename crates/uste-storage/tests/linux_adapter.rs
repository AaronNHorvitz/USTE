#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::{
    fs::{self, File},
    os::unix::{fs::PermissionsExt, fs::symlink},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use uste_storage::{
    AdapterErrorKind, EntryName, FileSystem, OwnershipFileSystem, linux::LinuxFileSystem,
    read_exact_at, write_all_at,
};

fn name(value: &str) -> EntryName {
    EntryName::new(value).unwrap()
}

fn filesystem() -> (TestDirectory, LinuxFileSystem) {
    let scratch = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(&scratch).unwrap();
    let discriminator = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = scratch.join(format!(
        "uste-linux-adapter-{}-{discriminator}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    let root: std::os::fd::OwnedFd = File::open(&directory).unwrap().into();
    let filesystem = LinuxFileSystem::from_directory(root).unwrap();
    (TestDirectory(directory), filesystem)
}

#[test]
fn positional_io_modes_and_no_replace_are_real() {
    let (directory, mut filesystem) = filesystem();
    let root = filesystem.root();
    let database = filesystem
        .create_directory(&root, &name("database"))
        .unwrap();
    let group = filesystem.create_new(&database, &name("group")).unwrap();
    write_all_at(&mut filesystem, &group, 4, b"bytes").unwrap();
    write_all_at(&mut filesystem, &group, 0, b"head").unwrap();
    filesystem.sync_data(&group).unwrap();
    filesystem.sync_directory(&database).unwrap();
    assert_eq!(filesystem.metadata(&group).unwrap().len, 9);
    let mut exact = [0_u8; 9];
    read_exact_at(&mut filesystem, &group, 0, &mut exact).unwrap();
    assert_eq!(&exact, b"headbytes");

    filesystem.set_len(&group, 4).unwrap();
    filesystem.sync_all(&group).unwrap();
    assert_eq!(filesystem.metadata(&group).unwrap().len, 4);

    let destination = filesystem
        .create_new(&database, &name("destination"))
        .unwrap();
    write_all_at(&mut filesystem, &destination, 0, b"kept").unwrap();
    assert_eq!(
        filesystem
            .rename_no_replace(&database, &name("group"), &database, &name("destination"))
            .unwrap_err()
            .kind(),
        AdapterErrorKind::AlreadyExists
    );
    let mut kept = [0_u8; 4];
    read_exact_at(&mut filesystem, &destination, 0, &mut kept).unwrap();
    assert_eq!(&kept, b"kept");

    let file_mode = fs::metadata(directory.0.join("database/group"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    let directory_mode = fs::metadata(directory.0.join("database"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(file_mode, 0o600);
    assert_eq!(directory_mode, 0o700);
}

#[test]
fn symlinks_are_rejected_and_lock_lifetime_is_independent() {
    let (directory, mut filesystem) = filesystem();
    let root = filesystem.root();
    let lock = filesystem.create_new(&root, &name("LOCK")).unwrap();
    filesystem.sync_all(&lock).unwrap();
    filesystem.sync_directory(&root).unwrap();

    let first = filesystem.try_lock_exclusive(&root, &name("LOCK")).unwrap();
    assert_eq!(
        filesystem
            .try_lock_exclusive(&root, &name("LOCK"))
            .unwrap_err()
            .kind(),
        AdapterErrorKind::OwnershipConflict
    );
    drop(first);
    let second = filesystem.try_lock_exclusive(&root, &name("LOCK")).unwrap();
    drop(second);

    fs::write(directory.0.join("outside"), b"outside").unwrap();
    symlink("outside", directory.0.join("link")).unwrap();
    assert_eq!(
        filesystem
            .open_existing(&root, &name("link"))
            .unwrap_err()
            .kind(),
        AdapterErrorKind::WrongEntryType
    );
}

struct TestDirectory(PathBuf);

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
