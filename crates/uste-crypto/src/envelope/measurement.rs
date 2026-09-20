//! Fixed-size privileged vault diagnostics, independent of cryptographic outcomes.
use crate::CryptoError;
use std::sync::Mutex;

/// Cumulative calls through one vault's decrypt boundary, not filesystem/device I/O.
/// Cardinality-sensitive: trusted adapters must authorize before exposing this report.
/// Successful authentication does not imply storage framing, proof or semantic admission.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VaultDecryptReport {
    pub successful_calls: u64,
    pub failed_calls: u64,
    /// Encoded envelope bytes for successful calls, including header, padding and tag.
    pub authenticated_encoded_bytes: u64,
    /// Exact unpadded plaintext bytes returned by successful calls.
    pub returned_plaintext_bytes: u64,
}

pub(super) struct Measurement(Mutex<Option<VaultDecryptReport>>);
impl Measurement {
    pub(super) fn new() -> Self {
        Self(Mutex::new(Some(VaultDecryptReport::default())))
    }
    pub(super) fn report(&self) -> Result<VaultDecryptReport, CryptoError> {
        self.0
            .lock()
            .map_err(|_| CryptoError::ResourceLimit)?
            .ok_or(CryptoError::ResourceLimit)
    }
    /// A poisoned/overflowed diagnostic never changes a decrypt result. Reports fail closed.
    pub(super) fn record(&self, success: Option<(usize, usize)>) {
        let Ok(mut guard) = self.0.lock() else {
            return;
        };
        let Some(mut next) = *guard else { return };
        let checked = (|| {
            if let Some((encoded, plaintext)) = success {
                next.successful_calls = next.successful_calls.checked_add(1)?;
                next.authenticated_encoded_bytes = next
                    .authenticated_encoded_bytes
                    .checked_add(u64::try_from(encoded).ok()?)?;
                next.returned_plaintext_bytes = next
                    .returned_plaintext_bytes
                    .checked_add(u64::try_from(plaintext).ok()?)?;
            } else {
                next.failed_calls = next.failed_calls.checked_add(1)?;
            }
            Some(next)
        })();
        *guard = checked;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_are_exact_and_overflow_is_sticky_and_atomic() {
        let counters = Measurement::new();
        counters.record(Some((4161, 3)));
        counters.record(None);
        counters.record(Some((20545, 16384)));
        assert_eq!(
            counters.report().unwrap(),
            VaultDecryptReport {
                successful_calls: 2,
                failed_calls: 1,
                authenticated_encoded_bytes: 24706,
                returned_plaintext_bytes: 16387,
            }
        );
        for field in 0..4 {
            let counters = Measurement::new();
            let mut report = VaultDecryptReport::default();
            match field {
                0 => report.successful_calls = u64::MAX,
                1 => report.failed_calls = u64::MAX,
                2 => report.authenticated_encoded_bytes = u64::MAX,
                _ => report.returned_plaintext_bytes = u64::MAX,
            }
            *counters.0.lock().unwrap() = Some(report);
            counters.record(if field == 1 { None } else { Some((1, 1)) });
            assert_eq!(counters.report(), Err(CryptoError::ResourceLimit));
            counters.record(Some((1, 1)));
            counters.record(None);
            assert_eq!(counters.report(), Err(CryptoError::ResourceLimit));
        }
    }

    #[test]
    fn concurrent_calls_have_exact_totals_and_poison_refuses_reports() {
        let counters = Measurement::new();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for _ in 0..100 {
                        counters.record(Some((4161, 7)));
                        counters.record(None);
                    }
                });
            }
        });
        assert_eq!(
            counters.report().unwrap(),
            VaultDecryptReport {
                successful_calls: 400,
                failed_calls: 400,
                authenticated_encoded_bytes: 1_664_400,
                returned_plaintext_bytes: 2800,
            }
        );
        std::thread::scope(|scope| {
            assert!(
                scope
                    .spawn(|| {
                        let _guard = counters.0.lock().unwrap();
                        panic!("synthetic diagnostic poison");
                    })
                    .join()
                    .is_err()
            );
        });
        counters.record(Some((1, 1)));
        assert_eq!(counters.report(), Err(CryptoError::ResourceLimit));
    }

    #[test]
    fn invalid_measurement_does_not_change_real_decrypt_or_nonce_results() {
        use crate::{
            CryptoContext, CryptoObjectId, EntropySource, FrameClass, KeyEpoch, KeyVault,
            ObjectRole, Scope, SecretKeyMaterial, WriterIncarnationId,
        };
        use uste_types::DatabaseId;
        struct Fixed;
        impl EntropySource for Fixed {
            fn fill(&mut self, output: &mut [u8]) -> Result<(), crate::EntropyFailure> {
                output.fill(7);
                Ok(())
            }
        }
        let db = DatabaseId::from_bytes([1; 16]);
        let mut vault = KeyVault {
            database: db,
            wrapped: (),
            key: Some(SecretKeyMaterial::from_adapter_bytes([2; 32])),
            session: Some(crate::UnlockedKeySession::new()),
            entropy: Fixed,
            used_nonces: Default::default(),
            nonce_limit: 1,
            measurement: Measurement::new(),
        };
        let context = CryptoContext::new(
            db,
            Scope::Database,
            KeyEpoch::FIRST,
            ObjectRole::JournalGroup,
            CryptoObjectId::from_bytes([3; 16]),
            1,
            WriterIncarnationId::from_bytes([4; 16]),
            1,
            0,
            FrameClass::Small4KiB,
        );
        let envelope = vault.encrypt(context, b"test").unwrap();
        *vault.measurement.0.lock().unwrap() = None;
        assert_eq!(
            vault.decrypt(context, &envelope).unwrap().as_slice(),
            b"test"
        );
        assert_eq!(vault.decrypt_report(), Err(CryptoError::ResourceLimit));
        assert_eq!(
            vault.encrypt(context, b"test").unwrap_err(),
            CryptoError::NonceSessionExhausted
        );
        vault.lock();
        assert_eq!(
            vault.decrypt(context, &envelope).unwrap_err(),
            CryptoError::Locked
        );
        assert_eq!(vault.decrypt_report(), Err(CryptoError::ResourceLimit));
    }
}
