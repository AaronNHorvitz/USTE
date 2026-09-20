//! Exact deltas for a single owning vault; not device traffic or whole-process accounting.
use super::*;
use uste_crypto::VaultDecryptReport;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct CryptoWork([u64; 4]);
impl From<VaultDecryptReport> for CryptoWork {
    fn from(report: VaultDecryptReport) -> Self {
        Self([
            report.successful_calls,
            report.failed_calls,
            report.authenticated_encoded_bytes,
            report.returned_plaintext_bytes,
        ])
    }
}
impl CryptoWork {
    pub(super) fn delta(self, before: Self) -> Result<Self, LinuxRunnerError> {
        let mut next = Self::default();
        for (index, value) in next.0.iter_mut().enumerate() {
            *value = self.0[index]
                .checked_sub(before.0[index])
                .ok_or_else(|| error("USTE_BM01_CRYPTO_COUNTER"))?;
        }
        Ok(next)
    }
    pub(super) fn accumulate(&mut self, other: Self) -> Result<(), LinuxRunnerError> {
        let mut next = *self;
        for (index, value) in next.0.iter_mut().enumerate() {
            *value = value
                .checked_add(other.0[index])
                .ok_or_else(|| error("USTE_BM01_CRYPTO_COUNTER"))?;
        }
        *self = next;
        Ok(())
    }
    pub(super) fn json(self) -> serde_json::Value {
        serde_json::json!({
            "measurement_scope": "single-owner-vault-completed-decrypt-calls",
            "physical_device_io": false, "complete_authenticated_io": false,
            "includes_key_unwrap": false, "includes_other_vaults": false,
            "successful_calls": self.0[0], "failed_calls": self.0[1],
            "authenticated_encoded_bytes": self.0[2], "returned_plaintext_bytes": self.0[3],
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vault_work_deltas_check_every_field_and_overflow_is_atomic() {
        let unit = CryptoWork::from(VaultDecryptReport {
            successful_calls: 1,
            failed_calls: 2,
            authenticated_encoded_bytes: 3,
            returned_plaintext_bytes: 4,
        });
        assert_eq!(unit.0, [1, 2, 3, 4]);
        let mut total = unit;
        total.accumulate(unit).unwrap();
        assert_eq!(total.delta(unit).unwrap(), unit);
        for field in 0..4 {
            let mut large = unit;
            large.0[field] = u64::MAX;
            let before = large;
            assert!(large.accumulate(unit).is_err());
            assert_eq!(large, before);
            assert!(unit.delta(large).is_err());
        }
        assert_eq!(unit.json()["complete_authenticated_io"], false);
        assert_eq!(unit.json()["physical_device_io"], false);
        assert_eq!(unit.json()["includes_other_vaults"], false);
    }
}
