//! Trusted key-adapter interface and fixed portable Argon2id recovery wrapper.

use core::fmt;
use std::collections::BTreeSet;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{Key, KeyInit, XChaCha20Poly1305, XNonce, aead::AeadInOut};
use uste_types::DatabaseId;
use zeroize::Zeroizing;

use crate::{
    CryptoError, ENVELOPE_FORMAT_MAJOR, ENVELOPE_FORMAT_MINOR, EntropySource,
    MAX_NONCES_PER_WRITER_SESSION, SecretKeyMaterial,
};

pub const RECOVERY_ENVELOPE_KIND: u8 = 0x81;
pub const ARGON2_MEMORY_KIB: u32 = 262_144;
pub const ARGON2_ITERATIONS: u32 = 3;
pub const ARGON2_LANES: u32 = 1;

const MAGIC: &[u8; 4] = b"USTE";
const RECOVERY_SUITE: u8 = 1;
const ARGON2ID_V13_PROFILE: u8 = 1;
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 24;
const TAG_BYTES: usize = 16;
const HEADER_BYTES: usize = 66;
pub const RECOVERY_ENVELOPE_HEADER_BYTES: usize = HEADER_BYTES;
const WRAPPED_FRAME_BYTES: usize = 4 * 1024;
const WRAPPED_CIPHERTEXT_BYTES: usize = WRAPPED_FRAME_BYTES + TAG_BYTES;
const RECOVERY_AAD_DOMAIN: &[u8] = b"USTE crypto-v1 recovery-wrap";
const RECOVERY_AAD_BYTES: usize = RECOVERY_AAD_DOMAIN.len() + HEADER_BYTES + 16;
pub const MAX_RECOVERY_PASSWORD_BYTES: usize = 1024;

/// Trusted secret-handling adapter boundary. Implementations must return only redacted errors.
pub trait KeyAdapter {
    type Envelope;

    fn wrap(
        &mut self,
        database: DatabaseId,
        key: &SecretKeyMaterial,
        entropy: &mut dyn EntropySource,
    ) -> Result<Self::Envelope, CryptoError>;

    fn unwrap(
        &mut self,
        database: DatabaseId,
        envelope: &Self::Envelope,
    ) -> Result<SecretKeyMaterial, CryptoError>;
}

/// Zeroizing, non-cloneable recovery credential.
pub struct RecoveryPassword(Zeroizing<Vec<u8>>);

impl RecoveryPassword {
    /// Retain a nonempty bounded credential. Password strength remains operator policy.
    pub fn new(bytes: Vec<u8>) -> Result<Self, CryptoError> {
        let bytes = Zeroizing::new(bytes);
        if bytes.is_empty() || bytes.len() > MAX_RECOVERY_PASSWORD_BYTES {
            return Err(CryptoError::InvalidCredential);
        }
        Ok(Self(bytes))
    }

    fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl fmt::Debug for RecoveryPassword {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecoveryPassword([REDACTED])")
    }
}

/// Structurally validated fixed-profile portable recovery envelope.
pub struct RecoveryEnvelope {
    salt: [u8; SALT_BYTES],
    nonce: [u8; NONCE_BYTES],
    ciphertext: Vec<u8>,
}

impl fmt::Debug for RecoveryEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RecoveryEnvelope")
            .field("ciphertext_len", &self.ciphertext.len())
            .finish_non_exhaustive()
    }
}

impl RecoveryEnvelope {
    #[must_use]
    pub const fn salt(&self) -> &[u8; SALT_BYTES] {
        &self.salt
    }

    #[must_use]
    pub const fn nonce(&self) -> &[u8; NONCE_BYTES] {
        &self.nonce
    }

    #[must_use]
    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }

    pub fn encode(&self) -> Result<Vec<u8>, CryptoError> {
        let header = self.header()?;
        let mut encoded = Vec::new();
        encoded
            .try_reserve_exact(HEADER_BYTES + self.ciphertext.len())
            .map_err(|_| CryptoError::RetryableUnavailable)?;
        encoded.extend_from_slice(&header);
        encoded.extend_from_slice(&self.ciphertext);
        Ok(encoded)
    }

    pub fn decode(encoded: &[u8]) -> Result<Self, CryptoError> {
        if encoded.len() < HEADER_BYTES || &encoded[..4] != MAGIC {
            return Err(CryptoError::InvalidEnvelope);
        }
        if encoded[4] != RECOVERY_ENVELOPE_KIND
            || encoded[5] != ENVELOPE_FORMAT_MAJOR
            || encoded[6] != ENVELOPE_FORMAT_MINOR
            || encoded[7] != RECOVERY_SUITE
            || encoded[8] != ARGON2ID_V13_PROFILE
        {
            return Err(CryptoError::UnsupportedProfile);
        }
        let memory = u32::from_be_bytes(
            encoded[9..13]
                .try_into()
                .map_err(|_| CryptoError::InvalidEnvelope)?,
        );
        let iterations = u32::from_be_bytes(
            encoded[13..17]
                .try_into()
                .map_err(|_| CryptoError::InvalidEnvelope)?,
        );
        if memory != ARGON2_MEMORY_KIB
            || iterations != ARGON2_ITERATIONS
            || u32::from(encoded[17]) != ARGON2_LANES
        {
            return Err(CryptoError::UnsupportedProfile);
        }
        let mut salt = [0_u8; SALT_BYTES];
        salt.copy_from_slice(&encoded[18..34]);
        let mut nonce = [0_u8; NONCE_BYTES];
        nonce.copy_from_slice(&encoded[34..58]);
        let ciphertext_u64 = u64::from_be_bytes(
            encoded[58..66]
                .try_into()
                .map_err(|_| CryptoError::InvalidEnvelope)?,
        );
        let ciphertext_len =
            usize::try_from(ciphertext_u64).map_err(|_| CryptoError::ResourceLimit)?;
        if ciphertext_len != WRAPPED_CIPHERTEXT_BYTES
            || encoded.len() != HEADER_BYTES + ciphertext_len
        {
            return Err(CryptoError::InvalidEnvelope);
        }
        let mut ciphertext = Vec::new();
        ciphertext
            .try_reserve_exact(ciphertext_len)
            .map_err(|_| CryptoError::RetryableUnavailable)?;
        ciphertext.extend_from_slice(&encoded[HEADER_BYTES..]);
        Ok(Self {
            salt,
            nonce,
            ciphertext,
        })
    }

    fn header(&self) -> Result<[u8; HEADER_BYTES], CryptoError> {
        if self.ciphertext.len() != WRAPPED_CIPHERTEXT_BYTES {
            return Err(CryptoError::InvalidEnvelope);
        }
        recovery_header(self.salt, self.nonce)
    }
}

/// Fixed-profile portable recovery adapter. This is not an OS key-store implementation.
pub struct PortableRecoveryAdapter {
    password: RecoveryPassword,
    used_nonces: BTreeSet<[u8; NONCE_BYTES]>,
}

impl PortableRecoveryAdapter {
    #[must_use]
    pub fn new(password: RecoveryPassword) -> Self {
        Self {
            password,
            used_nonces: BTreeSet::new(),
        }
    }
}

impl fmt::Debug for PortableRecoveryAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PortableRecoveryAdapter([REDACTED])")
    }
}

impl KeyAdapter for PortableRecoveryAdapter {
    type Envelope = RecoveryEnvelope;

    fn wrap(
        &mut self,
        database: DatabaseId,
        key: &SecretKeyMaterial,
        entropy: &mut dyn EntropySource,
    ) -> Result<Self::Envelope, CryptoError> {
        if self.used_nonces.len() >= MAX_NONCES_PER_WRITER_SESSION {
            return Err(CryptoError::NonceSessionExhausted);
        }
        let mut salt = [0_u8; SALT_BYTES];
        entropy
            .fill(&mut salt)
            .map_err(|_| CryptoError::RetryableUnavailable)?;
        let mut nonce = [0_u8; NONCE_BYTES];
        entropy
            .fill(&mut nonce)
            .map_err(|_| CryptoError::RetryableUnavailable)?;
        if !self.used_nonces.insert(nonce) {
            return Err(CryptoError::IntegrityFailure);
        }
        let wrapping_key = derive_wrapping_key(self.password.as_slice(), &salt)?;
        let mut padded = Zeroizing::new(Vec::new());
        padded
            .try_reserve_exact(WRAPPED_CIPHERTEXT_BYTES)
            .map_err(|_| CryptoError::RetryableUnavailable)?;
        padded.resize(WRAPPED_FRAME_BYTES, 0);
        padded[..32].copy_from_slice(key.expose_to_adapter());
        let header = recovery_header(salt, nonce)?;
        let aad = recovery_aad(&header, database);
        let cipher_key: &Key = wrapping_key
            .as_slice()
            .try_into()
            .map_err(|_| CryptoError::InvalidContext)?;
        let cipher = XChaCha20Poly1305::new(cipher_key);
        let nonce_ref = XNonce::from(nonce);
        cipher
            .encrypt_in_place(&nonce_ref, &aad, &mut *padded)
            .map_err(|_| CryptoError::IntegrityFailure)?;
        debug_assert_eq!(padded.len(), WRAPPED_CIPHERTEXT_BYTES);
        Ok(RecoveryEnvelope {
            salt,
            nonce,
            ciphertext: std::mem::take(&mut *padded),
        })
    }

    fn unwrap(
        &mut self,
        database: DatabaseId,
        envelope: &Self::Envelope,
    ) -> Result<SecretKeyMaterial, CryptoError> {
        let header = envelope.header().map_err(|_| CryptoError::KeyUnavailable)?;
        let aad = recovery_aad(&header, database);
        let wrapping_key = derive_wrapping_key(self.password.as_slice(), &envelope.salt)?;
        decrypt_wrapped_key(&wrapping_key, envelope, &aad)
    }
}

fn decrypt_wrapped_key(
    wrapping_key: &[u8; 32],
    envelope: &RecoveryEnvelope,
    aad: &[u8],
) -> Result<SecretKeyMaterial, CryptoError> {
    let cipher_key: &Key = wrapping_key
        .as_slice()
        .try_into()
        .map_err(|_| CryptoError::InvalidContext)?;
    let cipher = XChaCha20Poly1305::new(cipher_key);
    let nonce = XNonce::from(envelope.nonce);
    let mut padded = Zeroizing::new(Vec::new());
    padded
        .try_reserve_exact(envelope.ciphertext.len())
        .map_err(|_| CryptoError::RetryableUnavailable)?;
    padded.extend_from_slice(&envelope.ciphertext);
    cipher
        .decrypt_in_place(&nonce, aad, &mut *padded)
        .map_err(|_| CryptoError::KeyUnavailable)?;
    if padded.len() != WRAPPED_FRAME_BYTES || padded[32..].iter().any(|byte| *byte != 0) {
        return Err(CryptoError::KeyUnavailable);
    }
    let mut key = Zeroizing::new([0_u8; 32]);
    key.copy_from_slice(&padded[..32]);
    Ok(SecretKeyMaterial::from_adapter_bytes(*key))
}

fn derive_wrapping_key(
    password: &[u8],
    salt: &[u8; SALT_BYTES],
) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    let params = Params::new(ARGON2_MEMORY_KIB, ARGON2_ITERATIONS, ARGON2_LANES, Some(32))
        .map_err(|_| CryptoError::InvalidContext)?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut output = Zeroizing::new([0_u8; 32]);
    argon2
        .hash_password_into(password, salt, output.as_mut())
        .map_err(|_| CryptoError::RetryableUnavailable)?;
    Ok(output)
}

fn recovery_header(
    salt: [u8; SALT_BYTES],
    nonce: [u8; NONCE_BYTES],
) -> Result<[u8; HEADER_BYTES], CryptoError> {
    let mut header = [0_u8; HEADER_BYTES];
    header[..4].copy_from_slice(MAGIC);
    header[4] = RECOVERY_ENVELOPE_KIND;
    header[5] = ENVELOPE_FORMAT_MAJOR;
    header[6] = ENVELOPE_FORMAT_MINOR;
    header[7] = RECOVERY_SUITE;
    header[8] = ARGON2ID_V13_PROFILE;
    header[9..13].copy_from_slice(&ARGON2_MEMORY_KIB.to_be_bytes());
    header[13..17].copy_from_slice(&ARGON2_ITERATIONS.to_be_bytes());
    header[17] = u8::try_from(ARGON2_LANES).map_err(|_| CryptoError::InvalidContext)?;
    header[18..34].copy_from_slice(&salt);
    header[34..58].copy_from_slice(&nonce);
    header[58..66].copy_from_slice(
        &u64::try_from(WRAPPED_CIPHERTEXT_BYTES)
            .map_err(|_| CryptoError::ResourceLimit)?
            .to_be_bytes(),
    );
    Ok(header)
}

fn recovery_aad(header: &[u8; HEADER_BYTES], database: DatabaseId) -> [u8; RECOVERY_AAD_BYTES] {
    let mut aad = [0_u8; RECOVERY_AAD_BYTES];
    let header_start = RECOVERY_AAD_DOMAIN.len();
    let database_start = header_start + HEADER_BYTES;
    aad[..header_start].copy_from_slice(RECOVERY_AAD_DOMAIN);
    aad[header_start..database_start].copy_from_slice(header);
    aad[database_start..].copy_from_slice(database.as_bytes());
    aad
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authenticated_nonzero_recovery_padding_is_rejected() {
        let database = DatabaseId::from_bytes([0x91; 16]);
        let salt = [0x92; SALT_BYTES];
        let nonce = [0x93; NONCE_BYTES];
        let header = recovery_header(salt, nonce).unwrap();
        let aad = recovery_aad(&header, database);
        let wrapping_key = Zeroizing::new([0x94; 32]);
        let cipher_key: &Key = wrapping_key.as_slice().try_into().unwrap();
        let cipher = XChaCha20Poly1305::new(cipher_key);
        let nonce_ref = XNonce::from(nonce);
        let mut malformed = vec![0_u8; WRAPPED_FRAME_BYTES];
        malformed[..32].fill(0x95);
        malformed[32] = 1;
        malformed.reserve_exact(TAG_BYTES);
        cipher
            .encrypt_in_place(&nonce_ref, &aad, &mut malformed)
            .unwrap();
        let envelope = RecoveryEnvelope {
            salt,
            nonce,
            ciphertext: malformed,
        };

        assert_eq!(
            decrypt_wrapped_key(&wrapping_key, &envelope, &aad).unwrap_err(),
            CryptoError::KeyUnavailable
        );
    }
}
