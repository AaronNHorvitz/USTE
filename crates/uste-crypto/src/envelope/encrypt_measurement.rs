//! Fixed-size privileged encryption diagnostics; never cryptographic authority.
use crate::CryptoError;
use std::sync::Mutex;

/// Cumulative completed calls through one vault's encryption boundary.
/// Cardinality-sensitive trusted-adapter diagnostics, not filesystem/device I/O.
/// Successful encryption does not imply serialization, storage acceptance or durability.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VaultEncryptReport {
    pub successful_calls: u64,
    pub failed_calls: u64,
    /// Encoded-envelope length represented by successful results, including header/padding/tag.
    pub produced_encoded_bytes: u64,
    /// Exact unpadded input bytes for successful calls only.
    pub accepted_plaintext_bytes: u64,
}

pub(super) struct Measurement(Mutex<Option<VaultEncryptReport>>);
impl Measurement {
    pub(super) fn new() -> Self {
        Self(Mutex::new(Some(VaultEncryptReport::default())))
    }
    pub(super) fn report(&self) -> Result<VaultEncryptReport, CryptoError> {
        self.0
            .lock()
            .map_err(|_| CryptoError::ResourceLimit)?
            .ok_or(CryptoError::ResourceLimit)
    }
    /// Only completed calls are observed. No diagnostic failure replaces an encryption result.
    pub(super) fn record(&self, success: Option<(usize, usize)>) {
        let Ok(mut guard) = self.0.lock() else { return };
        let Some(mut next) = *guard else { return };
        *guard = (|| {
            if let Some((encoded, plaintext)) = success {
                next.successful_calls = next.successful_calls.checked_add(1)?;
                next.produced_encoded_bytes = next
                    .produced_encoded_bytes
                    .checked_add(u64::try_from(encoded).ok()?)?;
                next.accepted_plaintext_bytes = next
                    .accepted_plaintext_bytes
                    .checked_add(u64::try_from(plaintext).ok()?)?;
            } else {
                next.failed_calls = next.failed_calls.checked_add(1)?;
            }
            Some(next)
        })();
    }
}

#[cfg(test)]
mod tests;
