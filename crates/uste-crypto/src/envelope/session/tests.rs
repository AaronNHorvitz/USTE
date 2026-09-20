use crate::{CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial};
use uste_types::DatabaseId;
struct NoEntropy;
impl EntropySource for NoEntropy {
    fn fill(&mut self, _: &mut [u8]) -> Result<(), EntropyFailure> {
        Err(EntropyFailure)
    }
}
struct Adapter(bool);
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
        if self.0 {
            Err(CryptoError::RetryableUnavailable)
        } else {
            Ok(SecretKeyMaterial::from_adapter_bytes([7; 32]))
        }
    }
}
#[test]
fn unlocked_cache_sessions_change_on_unlock_not_redundant_unlock_or_failed_unlock() {
    let database = DatabaseId::from_bytes([1; 16]);
    let mut vault = KeyVault::from_locked(database, (), NoEntropy);
    assert!(matches!(vault.unlocked_session(), Err(CryptoError::Locked)));
    assert_eq!(
        vault.unlock(&mut Adapter(true)),
        Err(CryptoError::RetryableUnavailable)
    );
    assert!(vault.unlocked_session().is_err());
    vault.unlock(&mut Adapter(false)).unwrap();
    let initial = vault.unlocked_session().unwrap().clone();
    vault.unlock(&mut Adapter(true)).unwrap(); // Already unlocked: adapter is not consulted.
    assert!(initial.same_session(vault.unlocked_session().unwrap()));
    vault.used_nonces.insert([3; 24]);
    vault.lock();
    assert!(vault.unlocked_session().is_err());
    assert!(vault.unlock(&mut Adapter(true)).is_err());
    vault.unlock(&mut Adapter(false)).unwrap();
    assert!(!initial.same_session(vault.unlocked_session().unwrap()));
    assert_eq!(vault.used_nonces.len(), 1); // Never reset the existing nonce-session safety bound.
    let mut other = KeyVault::from_locked(database, (), NoEntropy);
    other.unlock(&mut Adapter(false)).unwrap();
    assert!(
        !other
            .unlocked_session()
            .unwrap()
            .same_session(vault.unlocked_session().unwrap())
    );
    assert_eq!(format!("{initial:?}"), "UnlockedKeySession([REDACTED])");
}
