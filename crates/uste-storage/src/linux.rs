//! Supported handle-relative Linux filesystem adapter.
//!
//! Authority enters through a caller-supplied directory descriptor. Every child operation stays
//! beneath that descriptor, rejects symlinks and mount crossings, and validates the opened object
//! type. The `rustix` crate is the audited syscall and initialized-buffer boundary; this crate
//! contains no first-party unsafe code.

use std::{fmt, os::fd::OwnedFd, sync::Arc};

use rustix::{
    fd::AsFd,
    fs::{
        FileType, FlockOperation, Mode, OFlags, RenameFlags, ResolveFlags, fdatasync, flock, fstat,
        fstatfs, fsync, ftruncate, mkdirat, openat2, renameat_with,
    },
    io::{Errno, fcntl_dupfd_cloexec, pread, pwrite},
};

use crate::{
    AdapterError, AdapterErrorKind, EntryName, FileMetadata, FileSystem, OwnershipFileSystem,
};

const RESOLVE_CHILD: ResolveFlags = ResolveFlags::BENEATH
    .union(ResolveFlags::NO_SYMLINKS)
    .union(ResolveFlags::NO_MAGICLINKS)
    .union(ResolveFlags::NO_XDEV);
const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;
const EXT_FAMILY_SUPER_MAGIC: u64 = 0xef53;

/// Cloneable regular-file capability. Debug output never exposes descriptor numbers or paths.
#[derive(Clone)]
pub struct LinuxFile(Arc<OwnedFd>);

impl fmt::Debug for LinuxFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LinuxFile { .. }")
    }
}

/// Cloneable directory capability. Debug output never exposes descriptor numbers or paths.
#[derive(Clone)]
pub struct LinuxDirectory(Arc<OwnedFd>);

impl fmt::Debug for LinuxDirectory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LinuxDirectory { .. }")
    }
}

/// Non-cloneable kernel ownership guard. Closing it or exiting the process releases `flock`.
pub struct LinuxOwnershipGuard(OwnedFd);

impl fmt::Debug for LinuxOwnershipGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _ = self.0.as_fd();
        formatter.write_str("LinuxOwnershipGuard { .. }")
    }
}

/// Linux adapter rooted at one explicitly supplied directory descriptor.
#[derive(Clone, Debug)]
pub struct LinuxFileSystem {
    root: LinuxDirectory,
}

/// Explicit local-filesystem admission profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinuxFilesystemProfile {
    /// Qualified by the T-13 adapter and process-loss matrix on the reference runner.
    Btrfs,
    /// Ext-family admission path; the caller must separately verify that the mount is ext4.
    Ext4Candidate,
}

impl LinuxFileSystem {
    /// Establish authority from an already-open directory descriptor.
    pub fn from_directory(directory: OwnedFd) -> Result<Self, AdapterError> {
        Self::from_directory_for_profile(directory, LinuxFilesystemProfile::Btrfs)
    }

    /// Establish authority under an explicit filesystem profile.
    ///
    /// The ext-family magic does not distinguish ext2/ext3/ext4; callers selecting
    /// `Ext4Candidate` must separately establish that the mounted filesystem is ext4. This entry
    /// point exists for the required qualification trial and is not itself qualification evidence.
    pub fn from_directory_for_profile(
        directory: OwnedFd,
        profile: LinuxFilesystemProfile,
    ) -> Result<Self, AdapterError> {
        let directory = fcntl_dupfd_cloexec(&directory, 0).map_err(map_errno)?;
        require_type(&directory, FileType::Directory)?;
        let filesystem_type = u64::try_from(fstatfs(&directory).map_err(map_errno)?.f_type)
            .map_err(|_| AdapterError::new(AdapterErrorKind::Unsupported))?;
        let expected_type = match profile {
            LinuxFilesystemProfile::Btrfs => BTRFS_SUPER_MAGIC,
            LinuxFilesystemProfile::Ext4Candidate => EXT_FAMILY_SUPER_MAGIC,
        };
        if filesystem_type != expected_type {
            return Err(AdapterErrorKind::Unsupported.into());
        }
        Ok(Self {
            root: LinuxDirectory(Arc::new(directory)),
        })
    }
}

impl FileSystem for LinuxFileSystem {
    type File = LinuxFile;
    type Directory = LinuxDirectory;

    fn root(&self) -> Self::Directory {
        self.root.clone()
    }

    fn create_directory(
        &mut self,
        parent: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::Directory, AdapterError> {
        mkdirat(parent.0.as_ref(), name.as_str(), Mode::RWXU).map_err(map_errno)?;
        open_directory(parent, name)
    }

    fn open_directory(
        &mut self,
        parent: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::Directory, AdapterError> {
        open_directory(parent, name)
    }

    fn create_new(
        &mut self,
        directory: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::File, AdapterError> {
        let fd = openat2(
            directory.0.as_ref(),
            name.as_str(),
            OFlags::RDWR
                | OFlags::CLOEXEC
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CREATE
                | OFlags::EXCL,
            Mode::RUSR | Mode::WUSR,
            RESOLVE_CHILD,
        )
        .map_err(map_open_errno)?;
        require_type(&fd, FileType::RegularFile)?;
        Ok(LinuxFile(Arc::new(fd)))
    }

    fn open_existing(
        &mut self,
        directory: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::File, AdapterError> {
        open_regular(directory, name)
    }

    fn metadata(&mut self, file: &Self::File) -> Result<FileMetadata, AdapterError> {
        let stat = fstat(file.0.as_ref()).map_err(map_errno)?;
        let len = u64::try_from(stat.st_size)
            .map_err(|_| AdapterError::new(AdapterErrorKind::ResourceLimit))?;
        Ok(FileMetadata { len })
    }

    fn read_at(
        &mut self,
        file: &Self::File,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, AdapterError> {
        pread(file.0.as_ref(), output, offset).map_err(map_errno)
    }

    fn write_at(
        &mut self,
        file: &Self::File,
        offset: u64,
        input: &[u8],
    ) -> Result<usize, AdapterError> {
        pwrite(file.0.as_ref(), input, offset).map_err(map_errno)
    }

    fn set_len(&mut self, file: &Self::File, len: u64) -> Result<(), AdapterError> {
        ftruncate(file.0.as_ref(), len).map_err(map_errno)
    }

    fn sync_data(&mut self, file: &Self::File) -> Result<(), AdapterError> {
        fdatasync(file.0.as_ref()).map_err(map_errno)
    }

    fn sync_all(&mut self, file: &Self::File) -> Result<(), AdapterError> {
        fsync(file.0.as_ref()).map_err(map_errno)
    }

    fn rename_no_replace(
        &mut self,
        source_directory: &Self::Directory,
        source: &EntryName,
        destination_directory: &Self::Directory,
        destination: &EntryName,
    ) -> Result<(), AdapterError> {
        renameat_with(
            source_directory.0.as_ref(),
            source.as_str(),
            destination_directory.0.as_ref(),
            destination.as_str(),
            RenameFlags::NOREPLACE,
        )
        .map_err(map_rename_errno)
    }

    fn sync_directory(&mut self, directory: &Self::Directory) -> Result<(), AdapterError> {
        fsync(directory.0.as_ref()).map_err(map_errno)
    }
}

impl OwnershipFileSystem for LinuxFileSystem {
    type OwnershipGuard = LinuxOwnershipGuard;

    fn try_lock_exclusive(
        &mut self,
        directory: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::OwnershipGuard, AdapterError> {
        let fd = open_regular_fd(directory, name)?;
        flock(&fd, FlockOperation::NonBlockingLockExclusive).map_err(map_lock_errno)?;
        Ok(LinuxOwnershipGuard(fd))
    }
}

fn open_directory(
    parent: &LinuxDirectory,
    name: &EntryName,
) -> Result<LinuxDirectory, AdapterError> {
    let fd = openat2(
        parent.0.as_ref(),
        name.as_str(),
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
        RESOLVE_CHILD,
    )
    .map_err(map_open_errno)?;
    require_type(&fd, FileType::Directory)?;
    Ok(LinuxDirectory(Arc::new(fd)))
}

fn open_regular(directory: &LinuxDirectory, name: &EntryName) -> Result<LinuxFile, AdapterError> {
    Ok(LinuxFile(Arc::new(open_regular_fd(directory, name)?)))
}

fn open_regular_fd(directory: &LinuxDirectory, name: &EntryName) -> Result<OwnedFd, AdapterError> {
    let fd = openat2(
        directory.0.as_ref(),
        name.as_str(),
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
        RESOLVE_CHILD,
    )
    .map_err(map_open_errno)?;
    require_type(&fd, FileType::RegularFile)?;
    Ok(fd)
}

fn require_type(fd: &OwnedFd, expected: FileType) -> Result<(), AdapterError> {
    let stat = fstat(fd).map_err(map_errno)?;
    if FileType::from_raw_mode(stat.st_mode) != expected {
        return Err(AdapterErrorKind::WrongEntryType.into());
    }
    Ok(())
}

fn map_open_errno(error: Errno) -> AdapterError {
    if error == Errno::XDEV {
        return AdapterErrorKind::CrossDevice.into();
    }
    if matches!(error, Errno::LOOP | Errno::NOTDIR | Errno::ISDIR) {
        return AdapterErrorKind::WrongEntryType.into();
    }
    if matches!(error, Errno::NOSYS | Errno::INVAL | Errno::NOTSUP) {
        return AdapterErrorKind::Unsupported.into();
    }
    map_errno(error)
}

fn map_rename_errno(error: Errno) -> AdapterError {
    if error == Errno::XDEV {
        return AdapterErrorKind::CrossDevice.into();
    }
    if matches!(error, Errno::NOSYS | Errno::INVAL | Errno::NOTSUP) {
        return AdapterErrorKind::Unsupported.into();
    }
    map_errno(error)
}

fn map_lock_errno(error: Errno) -> AdapterError {
    // Linux exposes EWOULDBLOCK as the same numeric value as EAGAIN.
    if error == Errno::AGAIN {
        return AdapterErrorKind::OwnershipConflict.into();
    }
    map_errno(error)
}

fn map_errno(error: Errno) -> AdapterError {
    let kind = if error == Errno::INTR {
        AdapterErrorKind::Interrupted
    } else if error == Errno::NOSPC {
        AdapterErrorKind::NoSpace
    } else if error == Errno::DQUOT {
        AdapterErrorKind::QuotaExceeded
    } else if matches!(error, Errno::ACCESS | Errno::PERM | Errno::ROFS) {
        AdapterErrorKind::PermissionDenied
    } else if error == Errno::EXIST {
        AdapterErrorKind::AlreadyExists
    } else if error == Errno::NOENT {
        AdapterErrorKind::NotFound
    } else if matches!(
        error,
        Errno::MFILE
            | Errno::NFILE
            | Errno::NOMEM
            | Errno::FBIG
            | Errno::OVERFLOW
            | Errno::NAMETOOLONG
    ) {
        AdapterErrorKind::ResourceLimit
    } else if error == Errno::STALE {
        AdapterErrorKind::StaleHandle
    } else {
        AdapterErrorKind::Io
    };
    kind.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errno_mapping_is_stable_and_contextual() {
        assert_eq!(map_errno(Errno::NOSPC).kind(), AdapterErrorKind::NoSpace);
        assert_eq!(
            map_open_errno(Errno::LOOP).kind(),
            AdapterErrorKind::WrongEntryType
        );
        assert_eq!(
            map_open_errno(Errno::XDEV).kind(),
            AdapterErrorKind::CrossDevice
        );
        assert_eq!(
            map_rename_errno(Errno::XDEV).kind(),
            AdapterErrorKind::CrossDevice
        );
        assert_eq!(
            map_lock_errno(Errno::WOULDBLOCK).kind(),
            AdapterErrorKind::OwnershipConflict
        );
    }
}
