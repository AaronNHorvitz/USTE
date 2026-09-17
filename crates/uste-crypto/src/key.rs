//! Secret master-key ownership and object-key derivation.

use core::fmt;

use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::{CryptoContext, CryptoError, EntropySource};

const MASTER_KEY_BYTES: usize = 32;
const HKDF_SALT: &[u8] = b"USTE crypto-v1 HKDF-SHA256 extract";
const HKDF_INFO: &[u8] = b"USTE crypto-v1 object-key";
const HKDF_PUBLIC_TOKEN_INFO: &[u8] = b"USTE crypto-v1 public opaque token";

/// Database master-key material. It is non-cloneable and redacted in diagnostics.
pub struct SecretKeyMaterial([u8; MASTER_KEY_BYTES]);

impl SecretKeyMaterial {
    /// Generate a new master key from exactly 32 bytes of the provided entropy capability.
    pub fn generate(entropy: &mut dyn EntropySource) -> Result<Self, CryptoError> {
        let mut bytes = Zeroizing::new([0_u8; MASTER_KEY_BYTES]);
        entropy
            .fill(bytes.as_mut())
            .map_err(|_| CryptoError::RetryableUnavailable)?;
        Ok(Self(*bytes))
    }

    /// Expose key bytes only to a trusted adapter implementation.
    #[must_use]
    pub fn expose_to_adapter(&self) -> &[u8; MASTER_KEY_BYTES] {
        &self.0
    }

    /// Import bytes returned by a trusted adapter.
    #[must_use]
    pub fn from_adapter_bytes(mut bytes: [u8; MASTER_KEY_BYTES]) -> Self {
        let key = Self(bytes);
        bytes.zeroize();
        key
    }

    pub(crate) fn derive(
        &self,
        context: CryptoContext,
    ) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
        let hkdf = Hkdf::<Sha256>::new(Some(HKDF_SALT), &self.0);
        let context_bytes = context.canonical_bytes();
        let mut output = Zeroizing::new([0_u8; 32]);
        hkdf.expand_multi_info(&[HKDF_INFO, &context_bytes], output.as_mut())
            .map_err(|_| CryptoError::InvalidContext)?;
        Ok(output)
    }

    pub(crate) fn derive_public_token(
        &self,
        context: CryptoContext,
        input: &[u8; 32],
    ) -> Result<[u8; 32], CryptoError> {
        let hkdf = Hkdf::<Sha256>::new(Some(HKDF_SALT), &self.0);
        let context_bytes = context.canonical_bytes();
        let mut output = [0_u8; 32];
        hkdf.expand_multi_info(
            &[HKDF_PUBLIC_TOKEN_INFO, &context_bytes, input],
            &mut output,
        )
        .map_err(|_| CryptoError::InvalidContext)?;
        Ok(output)
    }
}

impl fmt::Debug for SecretKeyMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretKeyMaterial([REDACTED])")
    }
}

impl Drop for SecretKeyMaterial {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl ZeroizeOnDrop for SecretKeyMaterial {}

/// Decrypted bytes that zeroize their allocation on drop and redact diagnostics.
pub struct SecretBytes(Zeroizing<Vec<u8>>);

impl SecretBytes {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// Borrow decrypted bytes for immediate authorized processing.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretBytes([REDACTED])")
    }
}
