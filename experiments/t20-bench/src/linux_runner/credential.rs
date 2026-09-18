use std::{fs::File, io::Read, path::Path};

use rustix::{
    fs::{FileType, Mode, OFlags, fstat, open},
    process::geteuid,
};
use uste_crypto::{MAX_RECOVERY_PASSWORD_BYTES, RecoveryPassword};
use zeroize::Zeroizing;

use super::LinuxRunnerError;

/// Read an exact portable-recovery credential through one validated descriptor.
///
/// Newlines are data and are deliberately not trimmed. Error codes never include the path,
/// metadata or credential bytes.
pub(super) fn read_password(path: &Path) -> Result<RecoveryPassword, LinuxRunnerError> {
    let descriptor = open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|_| LinuxRunnerError::new("USTE_BM01_CREDENTIAL_OPEN"))?;
    let stat =
        fstat(&descriptor).map_err(|_| LinuxRunnerError::new("USTE_BM01_CREDENTIAL_METADATA"))?;
    let valid_size = usize::try_from(stat.st_size)
        .ok()
        .is_some_and(|size| (1..=MAX_RECOVERY_PASSWORD_BYTES).contains(&size));
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_uid != geteuid().as_raw()
        || stat.st_nlink != 1
        || stat.st_mode & 0o077 != 0
        || !valid_size
    {
        return Err(LinuxRunnerError::new("USTE_BM01_CREDENTIAL_POLICY"));
    }

    let mut bytes = Zeroizing::new(Vec::new());
    bytes
        .try_reserve_exact(MAX_RECOVERY_PASSWORD_BYTES + 1)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_RESOURCE_LIMIT"))?;
    let mut file = File::from(descriptor);
    file.by_ref()
        .take(u64::try_from(MAX_RECOVERY_PASSWORD_BYTES + 1).expect("small bound"))
        .read_to_end(&mut bytes)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_CREDENTIAL_READ"))?;
    if bytes.len() != usize::try_from(stat.st_size).unwrap_or(usize::MAX)
        || bytes.is_empty()
        || bytes.len() > MAX_RECOVERY_PASSWORD_BYTES
    {
        return Err(LinuxRunnerError::new("USTE_BM01_CREDENTIAL_CHANGED"));
    }
    let owned = core::mem::take(&mut *bytes);
    RecoveryPassword::new(owned).map_err(|_| LinuxRunnerError::new("USTE_BM01_CREDENTIAL_POLICY"))
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, OpenOptions},
        io::Write,
        os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _, symlink},
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::read_password;

    static NEXT: AtomicU64 = AtomicU64::new(1);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let ordinal = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "uste-t20-credential-{}-{ordinal}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn write(path: &Path, bytes: &[u8], mode: u32) {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .unwrap();
        file.write_all(bytes).unwrap();
        file.sync_all().unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn exact_owner_only_password_is_accepted_without_trimming() {
        let directory = TestDirectory::new();
        let path = directory.path("password");
        write(&path, b"exact bytes including newline\n", 0o600);
        assert_eq!(
            format!("{:?}", read_password(&path).unwrap()),
            "RecoveryPassword([REDACTED])"
        );
    }

    #[test]
    fn permissions_size_type_and_symlink_fail_with_content_free_codes() {
        let directory = TestDirectory::new();
        let permissive = directory.path("permissive");
        write(&permissive, b"secret", 0o640);
        assert_eq!(
            read_password(&permissive).unwrap_err().code(),
            "USTE_BM01_CREDENTIAL_POLICY"
        );

        let oversized = directory.path("oversized");
        write(&oversized, &vec![b'x'; 1025], 0o600);
        assert_eq!(
            read_password(&oversized).unwrap_err().code(),
            "USTE_BM01_CREDENTIAL_POLICY"
        );

        let link = directory.path("link");
        symlink(&permissive, &link).unwrap();
        assert_eq!(
            read_password(&link).unwrap_err().code(),
            "USTE_BM01_CREDENTIAL_OPEN"
        );

        let directory_path = directory.path("directory");
        fs::create_dir(&directory_path).unwrap();
        assert_eq!(
            read_password(&directory_path).unwrap_err().code(),
            "USTE_BM01_CREDENTIAL_POLICY"
        );
    }
}
