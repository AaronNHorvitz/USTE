//! Separate successful encryption output accounting, never durable or device bytes.
use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct EncryptionWork(pub(super) CryptoWork);
impl From<VaultEncryptReport> for EncryptionWork {
    fn from(report: VaultEncryptReport) -> Self {
        Self(CryptoWork([
            report.successful_calls,
            report.failed_calls,
            report.produced_encoded_bytes,
            report.accepted_plaintext_bytes,
        ]))
    }
}
impl EncryptionWork {
    pub(super) fn json(self) -> serde_json::Value {
        serde_json::json!({
            "measurement_scope": "single-owner-vault-completed-encrypt-calls",
            "successful_calls": self.0.0[0], "failed_calls": self.0.0[1],
            "produced_encoded_bytes": self.0.0[2], "accepted_plaintext_bytes": self.0.0[3],
            "measures_durable_bytes": false, "physical_device_io": false,
            "includes_failed_call_bytes": false,
        })
    }
}
impl OwnerWork {
    pub(in super::super) fn encryption_json(self, history: bool) -> serde_json::Value {
        let mut owners = serde_json::Map::new();
        for (label, work) in OWNER_LABELS.into_iter().zip(self.owners) {
            if let Some((_, encrypted)) = work {
                owners.insert(label.into(), encrypted.json());
            }
        }
        let mut total = self.encryption_total.json();
        total["measurement_scope"] =
            serde_json::json!("explicit-command-owner-lifetimes-completed-encrypt-calls");
        serde_json::json!({
            "measurement_scope": if history {
                "completed-bm06-command-through-reported-terminal-state"
            } else { "completed-bm01-command-through-terminal-admission" },
            "complete_authenticated_io": false, "physical_device_io": false,
            "includes_key_wrap": false, "includes_failed_command_owners": false,
            "includes_failed_call_bytes": false, "measures_durable_bytes": false,
            "owner_count": owners.len(), "owners": owners, "total": total,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paired_owner_reports_commit_atomically_and_keep_encryption_separate() {
        let decrypt = VaultDecryptReport::default();
        let encrypt = VaultEncryptReport {
            successful_calls: 1,
            failed_calls: 2,
            produced_encoded_bytes: 4161,
            accepted_plaintext_bytes: 16,
        };
        let mut work = OwnerWork::default();
        for stage in [
            OwnerStage::Bootstrap,
            OwnerStage::BootstrapResume,
            OwnerStage::Construction,
            OwnerStage::Rebuild,
            OwnerStage::Terminal,
            OwnerStage::History,
        ] {
            work.record(stage, decrypt, encrypt).unwrap();
            let before = work;
            assert!(work.record(stage, decrypt, encrypt).is_err());
            assert_eq!(work, before);
        }
        assert_eq!(work.encryption_total.0.0, [6, 12, 24966, 96]);
        assert_eq!(work.json()["total"]["successful_calls"], 0);
        for history in [false, true] {
            let json = work.encryption_json(history);
            assert_eq!(json["owner_count"], 6);
            assert_eq!(json["total"]["produced_encoded_bytes"], 24966);
            assert_eq!(json["measures_durable_bytes"], false);
            assert_eq!(json["complete_authenticated_io"], false);
            for label in OWNER_LABELS {
                assert_eq!(json["owners"][label]["accepted_plaintext_bytes"], 16);
            }
        }
        for field in 0..4 {
            let mut work = OwnerWork::default();
            work.encryption_total.0.0[field] = u64::MAX;
            let before = work;
            let nonzero_decrypt = VaultDecryptReport {
                successful_calls: 1,
                failed_calls: 2,
                authenticated_encoded_bytes: 4161,
                returned_plaintext_bytes: 16,
            };
            assert!(
                work.record(OwnerStage::Terminal, nonzero_decrypt, encrypt)
                    .is_err()
            );
            assert_eq!(work, before);
            let mut work = OwnerWork::default();
            work.total.0[field] = u64::MAX;
            let before = work;
            assert!(
                work.record(OwnerStage::Terminal, nonzero_decrypt, encrypt)
                    .is_err()
            );
            assert_eq!(work, before);
        }
    }
}
