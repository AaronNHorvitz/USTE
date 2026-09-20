//! Strict `crypto-v1` authenticated-encryption and trusted key-adapter boundary.
//!
//! USTE does not implement cryptographic primitives. This crate fixes context construction,
//! envelope admission, nonce-session behavior, redacted errors and key ownership around the
//! selected RustCrypto implementations.

#![forbid(unsafe_code)]

mod context;
mod entropy;
mod envelope;
mod error;
mod key;
mod recovery;

pub use context::{
    CryptoContext, CryptoObjectId, FrameClass, KeyEpoch, KeyEpochError, ObjectRole, Scope,
    WriterIncarnationId,
};
pub use entropy::{EntropyFailure, EntropySource, OsEntropy};
pub use envelope::{
    ENVELOPE_FORMAT_MAJOR, ENVELOPE_FORMAT_MINOR, EncryptedEnvelope, KeyVault,
    MAX_NONCES_PER_WRITER_SESSION, MAX_PLAINTEXT_BYTES, OBJECT_ENVELOPE_HEADER_BYTES,
    OBJECT_ENVELOPE_KIND, UnlockedKeySession, VaultDecryptReport, VaultNonceReport,
};
pub use error::CryptoError;
pub use key::{SecretBytes, SecretKeyMaterial};
pub use recovery::{
    ARGON2_ITERATIONS, ARGON2_LANES, ARGON2_MEMORY_KIB, KeyAdapter, MAX_RECOVERY_PASSWORD_BYTES,
    PortableRecoveryAdapter, RECOVERY_ENVELOPE_HEADER_BYTES, RECOVERY_ENVELOPE_KIND,
    RecoveryEnvelope, RecoveryPassword,
};
