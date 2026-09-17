//! Fixed-width authenticated and key-derivation context.

use core::fmt;
use core::num::NonZeroU64;

use uste_types::{DatabaseId, NamespaceId};

/// Nonzero key epoch. Zero is reserved for an uninitialized database.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct KeyEpoch(NonZeroU64);

impl KeyEpoch {
    pub const FIRST: Self = Self(NonZeroU64::MIN);

    pub const fn new(value: u64) -> Result<Self, KeyEpochError> {
        match NonZeroU64::new(value) {
            Some(value) => Ok(Self(value)),
            None => Err(KeyEpochError::Zero),
        }
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Invalid key-epoch construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyEpochError {
    Zero,
}

impl fmt::Display for KeyEpochError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("key epoch zero is reserved")
    }
}

impl std::error::Error for KeyEpochError {}

/// Opaque object or segment identity used only for cryptographic domain separation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CryptoObjectId([u8; 16]);

impl CryptoObjectId {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// Durable writer incarnation. Restore must allocate and publish a new value before writes.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WriterIncarnationId([u8; 16]);

impl WriterIncarnationId {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// Database-wide or namespace-specific encryption scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scope {
    Database,
    Namespace(NamespaceId),
}

/// Assigned cryptographic object roles. Free-form role strings are not accepted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ObjectRole {
    JournalGroup = 1,
    CommitCertificate = 2,
    BlobChunk = 3,
    Snapshot = 4,
    IndexPage = 5,
    Backup = 6,
    TemporarySpill = 7,
    WorkerOutput = 8,
    CreationManifest = 9,
    BlobInventory = 10,
    BlobManifest = 11,
    BlobInventoryName = 12,
}

/// Public ciphertext padding class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FrameClass {
    Small4KiB = 1,
    Blob64KiB = 2,
}

impl FrameClass {
    #[must_use]
    pub const fn bytes(self) -> usize {
        match self {
            Self::Small4KiB => 4 * 1024,
            Self::Blob64KiB => 64 * 1024,
        }
    }

    pub(crate) const fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            1 => Some(Self::Small4KiB),
            2 => Some(Self::Blob64KiB),
            _ => None,
        }
    }
}

/// Complete fixed-width domain for object-key derivation and authenticated data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CryptoContext {
    database: DatabaseId,
    scope: Scope,
    epoch: KeyEpoch,
    role: ObjectRole,
    object: CryptoObjectId,
    sequence: u64,
    writer: WriterIncarnationId,
    object_format_major: u8,
    object_format_minor: u8,
    frame: FrameClass,
}

impl CryptoContext {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        database: DatabaseId,
        scope: Scope,
        epoch: KeyEpoch,
        role: ObjectRole,
        object: CryptoObjectId,
        sequence: u64,
        writer: WriterIncarnationId,
        object_format_major: u8,
        object_format_minor: u8,
        frame: FrameClass,
    ) -> Self {
        Self {
            database,
            scope,
            epoch,
            role,
            object,
            sequence,
            writer,
            object_format_major,
            object_format_minor,
            frame,
        }
    }

    #[must_use]
    pub const fn role(self) -> ObjectRole {
        self.role
    }

    #[must_use]
    pub const fn database(self) -> DatabaseId {
        self.database
    }

    #[must_use]
    pub const fn epoch(self) -> KeyEpoch {
        self.epoch
    }

    #[must_use]
    pub const fn frame(self) -> FrameClass {
        self.frame
    }

    pub(crate) fn canonical_bytes(self) -> [u8; 88] {
        let mut bytes = [0_u8; 88];
        bytes[..16].copy_from_slice(self.database.as_bytes());
        match self.scope {
            Scope::Database => bytes[16] = 0,
            Scope::Namespace(namespace) => {
                bytes[16] = 1;
                bytes[17..33].copy_from_slice(namespace.as_bytes());
            }
        }
        bytes[33..41].copy_from_slice(&self.epoch.get().to_be_bytes());
        bytes[41] = self.role as u8;
        bytes[42..58].copy_from_slice(self.object.as_bytes());
        bytes[58..66].copy_from_slice(&self.sequence.to_be_bytes());
        bytes[66..82].copy_from_slice(self.writer.as_bytes());
        bytes[82] = self.object_format_major;
        bytes[83] = self.object_format_minor;
        bytes[84] = self.frame as u8;
        // Bytes 85..88 are reserved zero bytes authenticated by format 1.
        bytes
    }
}
