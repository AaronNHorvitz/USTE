//! Capability-oriented filesystem, clock and randomness interfaces.

use core::fmt;

use uste_types::UtcInstant;

pub const MAX_ENTRY_NAME_BYTES: usize = 255;

/// One validated path component interpreted relative to an already authorized directory handle.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntryName(String);

impl EntryName {
    pub fn new(value: impl Into<String>) -> Result<Self, EntryNameError> {
        let value = value.into();
        if value.is_empty() {
            return Err(EntryNameError::Empty);
        }
        if value.len() > MAX_ENTRY_NAME_BYTES {
            return Err(EntryNameError::TooLong);
        }
        if value == "." || value == ".." {
            return Err(EntryNameError::Reserved);
        }
        if value.bytes().any(|byte| byte == b'/' || byte == 0) {
            return Err(EntryNameError::SeparatorOrNul);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryNameError {
    Empty,
    TooLong,
    Reserved,
    SeparatorOrNul,
}

impl fmt::Display for EntryNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "storage entry name is empty",
            Self::TooLong => "storage entry name exceeds fixed limit",
            Self::Reserved => "storage entry name is reserved",
            Self::SeparatorOrNul => "storage entry name contains a separator or NUL",
        })
    }
}

impl std::error::Error for EntryNameError {}

/// Content-free adapter error classification used at the durability boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AdapterErrorKind {
    Interrupted,
    NoSpace,
    QuotaExceeded,
    PermissionDenied,
    AlreadyExists,
    NotFound,
    UnexpectedEof,
    ZeroProgress,
    ResourceLimit,
    Unsupported,
    AdapterContract,
    InjectedCrash,
    StaleHandle,
    ScriptExhausted,
    Io,
}

/// Stable error without a path, payload, platform message or other sensitive detail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdapterError {
    kind: AdapterErrorKind,
}

impl AdapterError {
    #[must_use]
    pub const fn new(kind: AdapterErrorKind) -> Self {
        Self { kind }
    }

    #[must_use]
    pub const fn kind(self) -> AdapterErrorKind {
        self.kind
    }

    #[must_use]
    pub const fn code(self) -> &'static str {
        match self.kind {
            AdapterErrorKind::Interrupted => "USTE_ADAPTER_INTERRUPTED",
            AdapterErrorKind::NoSpace => "USTE_ADAPTER_NO_SPACE",
            AdapterErrorKind::QuotaExceeded => "USTE_ADAPTER_QUOTA_EXCEEDED",
            AdapterErrorKind::PermissionDenied => "USTE_ADAPTER_PERMISSION_DENIED",
            AdapterErrorKind::AlreadyExists => "USTE_ADAPTER_ALREADY_EXISTS",
            AdapterErrorKind::NotFound => "USTE_ADAPTER_NOT_FOUND",
            AdapterErrorKind::UnexpectedEof => "USTE_ADAPTER_UNEXPECTED_EOF",
            AdapterErrorKind::ZeroProgress => "USTE_ADAPTER_ZERO_PROGRESS",
            AdapterErrorKind::ResourceLimit => "USTE_ADAPTER_RESOURCE_LIMIT",
            AdapterErrorKind::Unsupported => "USTE_ADAPTER_UNSUPPORTED",
            AdapterErrorKind::AdapterContract => "USTE_ADAPTER_CONTRACT",
            AdapterErrorKind::InjectedCrash => "USTE_ADAPTER_INJECTED_CRASH",
            AdapterErrorKind::StaleHandle => "USTE_ADAPTER_STALE_HANDLE",
            AdapterErrorKind::ScriptExhausted => "USTE_ADAPTER_SCRIPT_EXHAUSTED",
            AdapterErrorKind::Io => "USTE_ADAPTER_IO",
        }
    }
}

impl fmt::Display for AdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for AdapterError {}

impl From<AdapterErrorKind> for AdapterError {
    fn from(kind: AdapterErrorKind) -> Self {
        Self::new(kind)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileMetadata {
    pub len: u64,
}

/// Filesystem authority relative to opaque directory and file handles.
///
/// Implementations must not resolve `EntryName` through ambient working-directory state. A zero
/// byte count from `write_at` is not progress. Callers decide which `Interrupted` operations are
/// safe to retry; flush and rename failures are not silently retried by this trait.
pub trait FileSystem {
    type File: Clone;
    type Directory: Clone;

    fn root(&self) -> Self::Directory;

    fn create_directory(
        &mut self,
        parent: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::Directory, AdapterError>;

    fn open_directory(
        &mut self,
        parent: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::Directory, AdapterError>;

    fn create_new(
        &mut self,
        directory: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::File, AdapterError>;

    fn open_existing(
        &mut self,
        directory: &Self::Directory,
        name: &EntryName,
    ) -> Result<Self::File, AdapterError>;

    fn metadata(&mut self, file: &Self::File) -> Result<FileMetadata, AdapterError>;

    fn read_at(
        &mut self,
        file: &Self::File,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, AdapterError>;

    fn write_at(
        &mut self,
        file: &Self::File,
        offset: u64,
        input: &[u8],
    ) -> Result<usize, AdapterError>;

    fn sync_data(&mut self, file: &Self::File) -> Result<(), AdapterError>;

    fn sync_all(&mut self, file: &Self::File) -> Result<(), AdapterError>;

    fn rename_no_replace(
        &mut self,
        source_directory: &Self::Directory,
        source: &EntryName,
        destination_directory: &Self::Directory,
        destination: &EntryName,
    ) -> Result<(), AdapterError>;

    fn sync_directory(&mut self, directory: &Self::Directory) -> Result<(), AdapterError>;
}

/// Test/reopen capability that atomically discards process-local state and invalidates handles.
pub trait RestartableFileSystem: FileSystem {
    fn restart(&mut self) -> Result<(), AdapterError>;
}

/// Checked positional write loop. Only `Interrupted` is retried automatically.
pub fn write_all_at<F: FileSystem>(
    filesystem: &mut F,
    file: &F::File,
    offset: u64,
    input: &[u8],
) -> Result<(), AdapterError> {
    let mut written = 0_usize;
    while written < input.len() {
        let delta = u64::try_from(written)
            .map_err(|_| AdapterError::new(AdapterErrorKind::ResourceLimit))?;
        let position = offset
            .checked_add(delta)
            .ok_or_else(|| AdapterError::new(AdapterErrorKind::ResourceLimit))?;
        match filesystem.write_at(file, position, &input[written..]) {
            Ok(0) => return Err(AdapterErrorKind::ZeroProgress.into()),
            Ok(count) if count <= input.len() - written => written += count,
            Ok(_) => return Err(AdapterErrorKind::AdapterContract.into()),
            Err(error) if error.kind() == AdapterErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Checked positional read loop. Only `Interrupted` is retried automatically.
pub fn read_exact_at<F: FileSystem>(
    filesystem: &mut F,
    file: &F::File,
    offset: u64,
    output: &mut [u8],
) -> Result<(), AdapterError> {
    let mut read = 0_usize;
    while read < output.len() {
        let delta =
            u64::try_from(read).map_err(|_| AdapterError::new(AdapterErrorKind::ResourceLimit))?;
        let position = offset
            .checked_add(delta)
            .ok_or_else(|| AdapterError::new(AdapterErrorKind::ResourceLimit))?;
        match filesystem.read_at(file, position, &mut output[read..]) {
            Ok(0) => return Err(AdapterErrorKind::UnexpectedEof.into()),
            Ok(count) if count <= output.len() - read => read += count,
            Ok(_) => return Err(AdapterErrorKind::AdapterContract.into()),
            Err(error) if error.kind() == AdapterErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockObservation {
    pub wall_utc: UtcInstant,
    pub monotonic_ticks: u64,
}

/// Explicit time capability. Wall time may repeat or move backward and is never commit ordering.
pub trait Clock {
    fn observe(&mut self) -> Result<ClockObservation, AdapterError>;
}

/// Explicit randomness capability for non-cryptographic storage identities and fault injection.
/// Security-sensitive key/nonces continue to use `uste-crypto::EntropySource`.
pub trait RandomSource {
    fn fill(&mut self, output: &mut [u8]) -> Result<(), AdapterError>;
}
