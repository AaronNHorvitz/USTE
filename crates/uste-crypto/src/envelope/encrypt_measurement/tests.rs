use super::*;
use crate::{
    CryptoContext, CryptoObjectId, EntropyFailure, EntropySource, FrameClass, KeyAdapter, KeyEpoch,
    KeyVault, ObjectRole, Scope, SecretKeyMaterial, WriterIncarnationId,
};
use uste_types::DatabaseId;

#[test]
fn encryption_totals_are_exact_and_every_overflow_is_sticky() {
    let counters = Measurement::new();
    counters.record(Some((4161, 3)));
    counters.record(None);
    counters.record(Some((20545, 16384)));
    counters.record(Some((4161, 0)));
    assert_eq!(
        counters.report().unwrap(),
        VaultEncryptReport {
            successful_calls: 3,
            failed_calls: 1,
            produced_encoded_bytes: 28867,
            accepted_plaintext_bytes: 16387,
        }
    );
    for field in 0..4 {
        let counters = Measurement::new();
        let mut report = VaultEncryptReport::default();
        match field {
            0 => report.successful_calls = u64::MAX,
            1 => report.failed_calls = u64::MAX,
            2 => report.produced_encoded_bytes = u64::MAX,
            _ => report.accepted_plaintext_bytes = u64::MAX,
        }
        *counters.0.lock().unwrap() = Some(report);
        counters.record(if field == 1 { None } else { Some((1, 1)) });
        assert_eq!(counters.report(), Err(CryptoError::ResourceLimit));
        counters.record(None);
        counters.record(Some((1, 1)));
        assert_eq!(counters.report(), Err(CryptoError::ResourceLimit));
    }
}

#[test]
fn encryption_measurement_completed_calls_are_coherent_across_threads() {
    let counters = Measurement::new();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let counters = &counters;
            scope.spawn(move || {
                for _ in 0..100 {
                    counters.record(Some((4161, 7)));
                    counters.record(None);
                }
            });
        }
    });
    assert_eq!(
        counters.report().unwrap(),
        VaultEncryptReport {
            successful_calls: 400,
            failed_calls: 400,
            produced_encoded_bytes: 1_664_400,
            accepted_plaintext_bytes: 2800,
        }
    );
}

struct Sequence(u8);
impl EntropySource for Sequence {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), EntropyFailure> {
        self.0 += 1;
        out.fill(self.0);
        Ok(())
    }
}
struct Adapter;
impl KeyAdapter for Adapter {
    type Envelope = ();
    fn wrap(
        &mut self,
        _: DatabaseId,
        _: &SecretKeyMaterial,
        _: &mut dyn EntropySource,
    ) -> Result<(), CryptoError> {
        Ok(())
    }
    fn unwrap(&mut self, _: DatabaseId, _: &()) -> Result<SecretKeyMaterial, CryptoError> {
        Ok(SecretKeyMaterial::from_adapter_bytes([2; 32]))
    }
}

#[test]
fn invalid_encryption_measurement_never_changes_ciphertext_errors_or_nonce_state() {
    for poison in [false, true] {
        let db = DatabaseId::from_bytes([1; 16]);
        let mut vault = KeyVault::from_locked(db, (), Sequence(0));
        vault.nonce_limit = 2;
        vault.unlock(&mut Adapter).unwrap();
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
        let first = vault.encrypt(context, b"first").unwrap();
        if poison {
            std::thread::scope(|scope| {
                let counters = &vault.encryption_measurement;
                assert!(
                    scope
                        .spawn(move || {
                            let _guard = counters.0.lock().unwrap();
                            panic!("synthetic encryption diagnostic poison");
                        })
                        .join()
                        .is_err()
                );
            });
        } else {
            *vault.encryption_measurement.0.lock().unwrap() = Some(VaultEncryptReport {
                successful_calls: u64::MAX,
                ..VaultEncryptReport::default()
            });
        }
        let second = vault.encrypt(context, b"second").unwrap();
        assert_eq!(vault.encrypt_report(), Err(CryptoError::ResourceLimit));
        assert_eq!(vault.nonce_report().issued_nonces, 2);
        assert_eq!(vault.decrypt(context, &first).unwrap().as_slice(), b"first");
        assert_eq!(
            vault.decrypt(context, &second).unwrap().as_slice(),
            b"second"
        );
        assert_eq!(vault.decrypt_report().unwrap().successful_calls, 2);
        assert_eq!(
            vault.encrypt(context, b"exhausted").unwrap_err(),
            CryptoError::NonceSessionExhausted
        );
        vault.lock();
        assert_eq!(
            vault.encrypt(context, b"locked").unwrap_err(),
            CryptoError::Locked
        );
        vault.unlock(&mut Adapter).unwrap();
        assert_eq!(
            vault.encrypt(context, b"still exhausted").unwrap_err(),
            CryptoError::NonceSessionExhausted
        );
        assert_eq!(vault.encrypt_report(), Err(CryptoError::ResourceLimit));
        assert_eq!(vault.nonce_report().issued_nonces, 2);
    }
}
