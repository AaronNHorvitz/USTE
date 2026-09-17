//! Padded object envelope, nonce-session registry and lockable key vault.

use core::fmt;
use std::collections::BTreeSet;

use chacha20poly1305::{Key, KeyInit, XChaCha20Poly1305, XNonce, aead::AeadInOut};
use uste_types::DatabaseId;
use zeroize::Zeroizing;

use crate::{
    CryptoContext, CryptoError, EntropySource, FrameClass, KeyAdapter, KeyEpoch, ObjectRole,
    SecretBytes, SecretKeyMaterial,
};

pub const ENVELOPE_FORMAT_MAJOR: u8 = 1;
pub const ENVELOPE_FORMAT_MINOR: u8 = 0;
pub const OBJECT_ENVELOPE_KIND: u8 = 0x80;
const XCHACHA20_POLY1305_HKDF_SHA256_SUITE: u8 = 1;
const TAG_BYTES: usize = 16;
const NONCE_BYTES: usize = 24;
const HEADER_BYTES: usize = 49;
pub const OBJECT_ENVELOPE_HEADER_BYTES: usize = HEADER_BYTES;
const INNER_LENGTH_BYTES: usize = 8;
const MAGIC: &[u8; 4] = b"USTE";
const AAD_DOMAIN: &[u8] = b"USTE crypto-v1 AEAD";
const AAD_BYTES: usize = AAD_DOMAIN.len() + HEADER_BYTES + 88;

/// Maximum exact plaintext admitted to one object envelope.
pub const MAX_PLAINTEXT_BYTES: usize = 16 * 1024 * 1024;

/// Maximum nonces tracked in one writer session before durable incarnation rotation is required.
pub const MAX_NONCES_PER_WRITER_SESSION: usize = 1_048_576;

/// Structurally validated encrypted object envelope.
pub struct EncryptedEnvelope {
    frame: FrameClass,
    epoch: KeyEpoch,
    nonce: [u8; NONCE_BYTES],
    ciphertext: Vec<u8>,
}

impl fmt::Debug for EncryptedEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EncryptedEnvelope")
            .field("frame", &self.frame)
            .field("epoch", &self.epoch)
            .field("ciphertext_len", &self.ciphertext.len())
            .finish_non_exhaustive()
    }
}

impl EncryptedEnvelope {
    #[must_use]
    pub const fn frame(&self) -> FrameClass {
        self.frame
    }

    #[must_use]
    pub const fn epoch(&self) -> KeyEpoch {
        self.epoch
    }

    #[must_use]
    pub const fn nonce(&self) -> &[u8; NONCE_BYTES] {
        &self.nonce
    }

    #[must_use]
    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }

    /// Encode the exact format-1 public header and ciphertext bytes.
    pub fn encode(&self) -> Result<Vec<u8>, CryptoError> {
        let header = self.header()?;
        let total = HEADER_BYTES
            .checked_add(self.ciphertext.len())
            .ok_or(CryptoError::ResourceLimit)?;
        let mut encoded = Vec::new();
        encoded
            .try_reserve_exact(total)
            .map_err(|_| CryptoError::RetryableUnavailable)?;
        encoded.extend_from_slice(&header);
        encoded.extend_from_slice(&self.ciphertext);
        Ok(encoded)
    }

    /// Parse format-1 bytes after all lengths have been checked against fixed caps.
    pub fn decode(encoded: &[u8]) -> Result<Self, CryptoError> {
        if encoded.len() < HEADER_BYTES || &encoded[..4] != MAGIC {
            return Err(CryptoError::InvalidEnvelope);
        }
        if encoded[4] != OBJECT_ENVELOPE_KIND
            || encoded[5] != ENVELOPE_FORMAT_MAJOR
            || encoded[6] != ENVELOPE_FORMAT_MINOR
            || encoded[7] != XCHACHA20_POLY1305_HKDF_SHA256_SUITE
        {
            return Err(CryptoError::UnsupportedProfile);
        }
        let frame = FrameClass::from_tag(encoded[8]).ok_or(CryptoError::UnsupportedProfile)?;
        let epoch_value = u64::from_be_bytes(
            encoded[9..17]
                .try_into()
                .map_err(|_| CryptoError::InvalidEnvelope)?,
        );
        let epoch = KeyEpoch::new(epoch_value).map_err(|_| CryptoError::InvalidEnvelope)?;
        let mut nonce = [0_u8; NONCE_BYTES];
        nonce.copy_from_slice(&encoded[17..41]);
        let ciphertext_u64 = u64::from_be_bytes(
            encoded[41..49]
                .try_into()
                .map_err(|_| CryptoError::InvalidEnvelope)?,
        );
        let ciphertext_len =
            usize::try_from(ciphertext_u64).map_err(|_| CryptoError::ResourceLimit)?;
        validate_ciphertext_length(frame, ciphertext_len)?;
        let expected = HEADER_BYTES
            .checked_add(ciphertext_len)
            .ok_or(CryptoError::ResourceLimit)?;
        if encoded.len() != expected {
            return Err(CryptoError::IntegrityFailure);
        }
        let mut ciphertext = Vec::new();
        ciphertext
            .try_reserve_exact(ciphertext_len)
            .map_err(|_| CryptoError::RetryableUnavailable)?;
        ciphertext.extend_from_slice(&encoded[HEADER_BYTES..]);
        Ok(Self {
            frame,
            epoch,
            nonce,
            ciphertext,
        })
    }

    fn header(&self) -> Result<[u8; HEADER_BYTES], CryptoError> {
        object_header(self.frame, self.epoch, self.nonce, self.ciphertext.len())
    }
}

/// Lockable master key plus one bounded in-memory writer nonce session.
pub struct KeyVault<W, E> {
    database: DatabaseId,
    wrapped: W,
    key: Option<SecretKeyMaterial>,
    entropy: E,
    used_nonces: BTreeSet<[u8; NONCE_BYTES]>,
    nonce_limit: usize,
}

impl<W, E: EntropySource> KeyVault<W, E> {
    /// Generate and wrap a fresh database master key, initially unlocked.
    pub fn create<A>(
        database: DatabaseId,
        adapter: &mut A,
        mut entropy: E,
    ) -> Result<Self, CryptoError>
    where
        A: KeyAdapter<Envelope = W>,
    {
        let key = SecretKeyMaterial::generate(&mut entropy)?;
        let wrapped = adapter.wrap(database, &key, &mut entropy)?;
        Ok(Self {
            database,
            wrapped,
            key: Some(key),
            entropy,
            used_nonces: BTreeSet::new(),
            nonce_limit: MAX_NONCES_PER_WRITER_SESSION,
        })
    }

    /// Construct a locked vault around a persisted adapter envelope.
    #[must_use]
    pub fn from_locked(database: DatabaseId, wrapped: W, entropy: E) -> Self {
        Self {
            database,
            wrapped,
            key: None,
            entropy,
            used_nonces: BTreeSet::new(),
            nonce_limit: MAX_NONCES_PER_WRITER_SESSION,
        }
    }

    /// Borrow the persistable wrapped key; no plaintext key is exposed.
    #[must_use]
    pub const fn wrapped(&self) -> &W {
        &self.wrapped
    }

    #[must_use]
    pub const fn is_locked(&self) -> bool {
        self.key.is_none()
    }

    /// Unlock with the trusted adapter. Failure leaves the vault locked.
    pub fn unlock<A>(&mut self, adapter: &mut A) -> Result<(), CryptoError>
    where
        A: KeyAdapter<Envelope = W>,
    {
        if self.key.is_some() {
            return Ok(());
        }
        let key = adapter.unwrap(self.database, &self.wrapped)?;
        self.key = Some(key);
        Ok(())
    }

    /// Drop and best-effort zeroize the unlocked key. Repeated locking is harmless.
    pub fn lock(&mut self) {
        self.key = None;
    }

    /// Encrypt and pad an object, consuming a fresh entropy-supplied nonce.
    pub fn encrypt(
        &mut self,
        context: CryptoContext,
        plaintext: &[u8],
    ) -> Result<EncryptedEnvelope, CryptoError> {
        if context.database() != self.database {
            return Err(CryptoError::InvalidContext);
        }
        if plaintext.len() > MAX_PLAINTEXT_BYTES {
            return Err(CryptoError::ResourceLimit);
        }
        let key = self.key.as_ref().ok_or(CryptoError::Locked)?;
        if self.used_nonces.len() >= self.nonce_limit {
            return Err(CryptoError::NonceSessionExhausted);
        }
        let mut nonce = [0_u8; NONCE_BYTES];
        self.entropy
            .fill(&mut nonce)
            .map_err(|_| CryptoError::RetryableUnavailable)?;
        if !self.used_nonces.insert(nonce) {
            return Err(CryptoError::IntegrityFailure);
        }
        encrypt(key, context, nonce, plaintext)
    }

    /// Authenticate, decrypt, validate padding and return zeroizing plaintext bytes.
    pub fn decrypt(
        &self,
        context: CryptoContext,
        envelope: &EncryptedEnvelope,
    ) -> Result<SecretBytes, CryptoError> {
        if context.database() != self.database {
            return Err(CryptoError::IntegrityFailure);
        }
        let key = self.key.as_ref().ok_or(CryptoError::Locked)?;
        decrypt(key, context, envelope)
    }

    /// Derive a deterministic public opaque identifier under the dedicated name-token role.
    ///
    /// The returned bytes are safe to expose as an on-disk name but are not an encryption key.
    pub fn derive_opaque_identifier(
        &self,
        context: CryptoContext,
        input: &[u8; 32],
    ) -> Result<[u8; 32], CryptoError> {
        if context.database() != self.database
            || !matches!(
                context.role(),
                ObjectRole::BlobInventoryName | ObjectRole::IndexName
            )
        {
            return Err(CryptoError::InvalidContext);
        }
        let key = self.key.as_ref().ok_or(CryptoError::Locked)?;
        key.derive_public_token(context, input)
    }
}

fn encrypt(
    master: &SecretKeyMaterial,
    context: CryptoContext,
    nonce: [u8; NONCE_BYTES],
    plaintext: &[u8],
) -> Result<EncryptedEnvelope, CryptoError> {
    let frame = context.frame();
    let padded_len = padded_length(frame, plaintext.len())?;
    let ciphertext_len = padded_len
        .checked_add(TAG_BYTES)
        .ok_or(CryptoError::ResourceLimit)?;
    let mut padded = Zeroizing::new(Vec::new());
    padded
        .try_reserve_exact(ciphertext_len)
        .map_err(|_| CryptoError::RetryableUnavailable)?;
    padded.extend_from_slice(
        &u64::try_from(plaintext.len())
            .map_err(|_| CryptoError::ResourceLimit)?
            .to_be_bytes(),
    );
    padded.extend_from_slice(plaintext);
    padded.resize(padded_len, 0);

    let header = object_header(frame, context.epoch(), nonce, ciphertext_len)?;
    let aad = associated_data(&header, context);
    let derived = master.derive(context)?;
    let cipher_key: &Key = derived
        .as_slice()
        .try_into()
        .map_err(|_| CryptoError::InvalidContext)?;
    let cipher = XChaCha20Poly1305::new(cipher_key);
    let nonce_ref = XNonce::from(nonce);
    cipher
        .encrypt_in_place(&nonce_ref, &aad, &mut *padded)
        .map_err(|_| CryptoError::IntegrityFailure)?;
    debug_assert_eq!(padded.len(), ciphertext_len);
    Ok(EncryptedEnvelope {
        frame,
        epoch: context.epoch(),
        nonce,
        ciphertext: std::mem::take(&mut *padded),
    })
}

fn decrypt(
    master: &SecretKeyMaterial,
    context: CryptoContext,
    envelope: &EncryptedEnvelope,
) -> Result<SecretBytes, CryptoError> {
    if context.epoch() != envelope.epoch || context.frame() != envelope.frame {
        return Err(CryptoError::IntegrityFailure);
    }
    validate_ciphertext_length(envelope.frame, envelope.ciphertext.len())?;
    let header = envelope.header()?;
    let aad = associated_data(&header, context);
    let derived = master.derive(context)?;
    let cipher_key: &Key = derived
        .as_slice()
        .try_into()
        .map_err(|_| CryptoError::InvalidContext)?;
    let cipher = XChaCha20Poly1305::new(cipher_key);
    let nonce_ref = XNonce::from(envelope.nonce);
    let mut padded = Zeroizing::new(Vec::new());
    padded
        .try_reserve_exact(envelope.ciphertext.len())
        .map_err(|_| CryptoError::RetryableUnavailable)?;
    padded.extend_from_slice(&envelope.ciphertext);
    cipher
        .decrypt_in_place(&nonce_ref, &aad, &mut *padded)
        .map_err(|_| CryptoError::IntegrityFailure)?;
    if padded.len() < INNER_LENGTH_BYTES {
        return Err(CryptoError::IntegrityFailure);
    }
    let actual_u64 = u64::from_be_bytes(
        padded[..INNER_LENGTH_BYTES]
            .try_into()
            .map_err(|_| CryptoError::IntegrityFailure)?,
    );
    let actual = usize::try_from(actual_u64).map_err(|_| CryptoError::IntegrityFailure)?;
    if actual > MAX_PLAINTEXT_BYTES || actual > padded.len() - INNER_LENGTH_BYTES {
        return Err(CryptoError::IntegrityFailure);
    }
    let end = INNER_LENGTH_BYTES + actual;
    if padded[end..].iter().any(|byte| *byte != 0) {
        return Err(CryptoError::IntegrityFailure);
    }
    padded.copy_within(INNER_LENGTH_BYTES..end, 0);
    padded.truncate(actual);
    Ok(SecretBytes::new(std::mem::take(&mut *padded)))
}

fn object_header(
    frame: FrameClass,
    epoch: KeyEpoch,
    nonce: [u8; NONCE_BYTES],
    ciphertext_len: usize,
) -> Result<[u8; HEADER_BYTES], CryptoError> {
    let ciphertext_len = u64::try_from(ciphertext_len).map_err(|_| CryptoError::ResourceLimit)?;
    let mut header = [0_u8; HEADER_BYTES];
    header[..4].copy_from_slice(MAGIC);
    header[4] = OBJECT_ENVELOPE_KIND;
    header[5] = ENVELOPE_FORMAT_MAJOR;
    header[6] = ENVELOPE_FORMAT_MINOR;
    header[7] = XCHACHA20_POLY1305_HKDF_SHA256_SUITE;
    header[8] = frame as u8;
    header[9..17].copy_from_slice(&epoch.get().to_be_bytes());
    header[17..41].copy_from_slice(&nonce);
    header[41..49].copy_from_slice(&ciphertext_len.to_be_bytes());
    Ok(header)
}

fn associated_data(header: &[u8; HEADER_BYTES], context: CryptoContext) -> [u8; AAD_BYTES] {
    let context = context.canonical_bytes();
    let mut aad = [0_u8; AAD_BYTES];
    let header_start = AAD_DOMAIN.len();
    let context_start = header_start + HEADER_BYTES;
    aad[..header_start].copy_from_slice(AAD_DOMAIN);
    aad[header_start..context_start].copy_from_slice(header);
    aad[context_start..].copy_from_slice(&context);
    aad
}

fn padded_length(frame: FrameClass, plaintext_len: usize) -> Result<usize, CryptoError> {
    let needed = INNER_LENGTH_BYTES
        .checked_add(plaintext_len)
        .ok_or(CryptoError::ResourceLimit)?;
    let frame_bytes = frame.bytes();
    needed
        .checked_add(frame_bytes - 1)
        .map(|length| length / frame_bytes * frame_bytes)
        .ok_or(CryptoError::ResourceLimit)
}

fn validate_ciphertext_length(frame: FrameClass, length: usize) -> Result<(), CryptoError> {
    let payload = length
        .checked_sub(TAG_BYTES)
        .ok_or(CryptoError::InvalidEnvelope)?;
    if payload == 0 || !payload.is_multiple_of(frame.bytes()) {
        return Err(CryptoError::InvalidEnvelope);
    }
    let maximum = padded_length(frame, MAX_PLAINTEXT_BYTES)?
        .checked_add(TAG_BYTES)
        .ok_or(CryptoError::ResourceLimit)?;
    if length > maximum {
        return Err(CryptoError::ResourceLimit);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use uste_types::{DatabaseId, NamespaceId};

    use super::KeyVault;
    use crate::{
        CryptoContext, CryptoError, CryptoObjectId, EntropyFailure, EntropySource, FrameClass,
        KeyEpoch, ObjectRole, Scope, SecretKeyMaterial, WriterIncarnationId,
    };

    struct NoEntropy;

    impl EntropySource for NoEntropy {
        fn fill(&mut self, _output: &mut [u8]) -> Result<(), EntropyFailure> {
            Err(EntropyFailure)
        }
    }

    #[test]
    fn nonce_session_exhaustion_is_bounded_and_precedes_entropy() {
        let database = DatabaseId::from_bytes([1; 16]);
        let mut vault = KeyVault {
            database,
            wrapped: (),
            key: Some(SecretKeyMaterial::from_adapter_bytes([2; 32])),
            entropy: NoEntropy,
            used_nonces: Default::default(),
            nonce_limit: 0,
        };
        let context = CryptoContext::new(
            database,
            Scope::Namespace(NamespaceId::from_bytes([3; 16])),
            KeyEpoch::FIRST,
            ObjectRole::JournalGroup,
            CryptoObjectId::from_bytes([4; 16]),
            0,
            WriterIncarnationId::from_bytes([5; 16]),
            1,
            0,
            FrameClass::Small4KiB,
        );
        assert_eq!(
            vault.encrypt(context, b"refused").unwrap_err(),
            CryptoError::NonceSessionExhausted
        );
    }
}
