//! Stable redacted public errors.

use core::fmt;

/// Content-free cryptographic boundary error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CryptoError {
    /// Authentication failed. Wrong keys, context and damaged bytes are intentionally indistinct.
    IntegrityFailure,
    /// Entropy or a transient resource was unavailable; no output was produced.
    RetryableUnavailable,
    /// No valid wrapped or unlocked key is available.
    KeyUnavailable,
    /// The vault is locked.
    Locked,
    /// The envelope/profile version is not supported.
    UnsupportedProfile,
    /// A fixed size, count or session bound was exceeded.
    ResourceLimit,
    /// The writer session must durably rotate before another nonce can be issued.
    NonceSessionExhausted,
    /// Public structural fields are invalid before cryptographic processing.
    InvalidEnvelope,
    /// A context is internally inconsistent with the selected database or envelope.
    InvalidContext,
    /// A credential violates the bounded adapter input contract.
    InvalidCredential,
}

impl CryptoError {
    /// Stable log-safe code. It contains no upstream diagnostic or sensitive input.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::IntegrityFailure => "USTE_CRYPTO_INTEGRITY_FAILURE",
            Self::RetryableUnavailable => "USTE_CRYPTO_RETRYABLE_UNAVAILABLE",
            Self::KeyUnavailable => "USTE_CRYPTO_KEY_UNAVAILABLE",
            Self::Locked => "USTE_CRYPTO_LOCKED",
            Self::UnsupportedProfile => "USTE_CRYPTO_UNSUPPORTED_PROFILE",
            Self::ResourceLimit => "USTE_CRYPTO_RESOURCE_LIMIT",
            Self::NonceSessionExhausted => "USTE_CRYPTO_NONCE_SESSION_EXHAUSTED",
            Self::InvalidEnvelope => "USTE_CRYPTO_INVALID_ENVELOPE",
            Self::InvalidContext => "USTE_CRYPTO_INVALID_CONTEXT",
            Self::InvalidCredential => "USTE_CRYPTO_INVALID_CREDENTIAL",
        }
    }
}

impl fmt::Display for CryptoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for CryptoError {}
