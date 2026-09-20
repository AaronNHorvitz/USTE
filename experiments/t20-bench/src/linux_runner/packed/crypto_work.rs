//! Exact deltas for a single owning vault; not device traffic or whole-process accounting.
use super::*;
use uste_crypto::{VaultDecryptReport, VaultEncryptReport};
mod encryption;
use encryption::EncryptionWork;

pub(super) fn encryption_report_json(report: VaultEncryptReport) -> serde_json::Value {
    EncryptionWork::from(report).json()
}

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

/// Closed command-lifetime slots. A consuming handoff does not start another vault lifetime.
#[derive(Clone, Copy)]
pub(super) enum OwnerStage {
    Bootstrap,
    BootstrapResume,
    Construction,
    Rebuild,
    Terminal,
    History,
}
const OWNER_LABELS: [&str; 6] = [
    "bootstrap",
    "bootstrap_resume",
    "construction",
    "rebuild",
    "terminal",
    "history_validation",
];

/// Each completed owner is sampled once immediately before drop (or terminal session return).
/// Fixed-size and checked; no handles, identities, request contents or retained event history.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct OwnerWork {
    owners: [Option<(CryptoWork, EncryptionWork)>; OWNER_LABELS.len()],
    total: CryptoWork,
    encryption_total: EncryptionWork,
}
impl OwnerWork {
    pub(super) fn record(
        &mut self,
        stage: OwnerStage,
        report: VaultDecryptReport,
        encryption: VaultEncryptReport,
    ) -> Result<(), LinuxRunnerError> {
        let slot = stage as usize;
        if self.owners[slot].is_some() {
            return Err(error("USTE_BM01_CRYPTO_OWNER"));
        }
        let work = CryptoWork::from(report);
        let mut total = self.total;
        total.accumulate(work)?;
        let encrypted = EncryptionWork::from(encryption);
        let mut encryption_total = self.encryption_total;
        encryption_total.0.accumulate(encrypted.0)?;
        self.owners[slot] = Some((work, encrypted));
        self.total = total;
        self.encryption_total = encryption_total;
        Ok(())
    }

    pub(super) fn json(self) -> serde_json::Value {
        let mut owners = serde_json::Map::new();
        for (label, work) in OWNER_LABELS.into_iter().zip(self.owners) {
            if let Some((work, _)) = work {
                owners.insert(label.into(), work.json());
            }
        }
        let mut total = self.total.json();
        total["measurement_scope"] =
            serde_json::json!("explicit-command-owner-lifetimes-completed-decrypt-calls");
        total
            .as_object_mut()
            .unwrap()
            .remove("includes_other_vaults");
        total["includes_all_recorded_owners"] = serde_json::json!(true);
        serde_json::json!({
            "measurement_scope": "completed-bm01-command-through-terminal-admission",
            "complete_authenticated_io": false, "physical_device_io": false,
            "includes_key_unwrap": false, "includes_encryption_bytes": false,
            "includes_pre_vault_decode_failures": false,
            "owner_count": owners.len(), "owners": owners, "total": total,
        })
    }

    pub(super) fn history_json(self) -> serde_json::Value {
        let mut report = self.json();
        report["measurement_scope"] =
            serde_json::json!("completed-bm06-command-through-reported-terminal-state");
        report
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owner_work_fixed_slots_sum_exactly_and_refuse_duplicate_or_overflow_atomically() {
        let unit = VaultDecryptReport {
            successful_calls: 1,
            failed_calls: 2,
            authenticated_encoded_bytes: 3,
            returned_plaintext_bytes: 4,
        };
        let mut work = OwnerWork::default();
        assert_eq!(work.json()["owner_count"], 0);
        for stage in [
            OwnerStage::Bootstrap,
            OwnerStage::BootstrapResume,
            OwnerStage::Construction,
            OwnerStage::Rebuild,
            OwnerStage::Terminal,
            OwnerStage::History,
        ] {
            work.record(stage, unit, VaultEncryptReport::default())
                .unwrap();
            let before = work;
            assert!(
                work.record(stage, unit, VaultEncryptReport::default())
                    .is_err()
            );
            assert_eq!(work, before);
        }
        assert_eq!(work.total.0, [6, 12, 18, 24]);
        let json = work.json();
        assert_eq!(json["owner_count"], 6);
        for label in OWNER_LABELS {
            assert_eq!(json["owners"][label]["successful_calls"], 1);
        }
        assert_eq!(json["total"]["successful_calls"], 6);
        let history = work.history_json();
        assert_eq!(history["owners"], json["owners"]);
        assert_eq!(history["total"], json["total"]);
        assert_eq!(history["complete_authenticated_io"], false);
        assert_ne!(history["measurement_scope"], json["measurement_scope"]);
        assert_eq!(json["complete_authenticated_io"], false);
        assert_eq!(json["includes_key_unwrap"], false);
        assert_eq!(json["includes_encryption_bytes"], false);
        for field in 0..4 {
            let mut work = OwnerWork::default();
            work.total.0[field] = u64::MAX;
            let before = work;
            assert!(
                work.record(OwnerStage::Terminal, unit, VaultEncryptReport::default())
                    .is_err()
            );
            assert_eq!(work, before);
        }
    }

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
