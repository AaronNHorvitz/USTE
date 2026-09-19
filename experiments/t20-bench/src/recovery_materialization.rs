//! Deterministic BM-06 event history, not a recovery campaign or resource qualification.
use uste_graph::{Expected, NewEntity, NewRecord, Operation, RecordVersion};
use uste_types::{BoundedString, NamespaceRef, RecordId, RecordRef, Value};

pub const PROFILE: &str = "bm06-materialization-v1";
pub const VERSIONS: u64 = 100;
pub const PAYLOAD_BYTES: usize = 4096;
pub const BATCH_RECORDS: u64 = 512;
pub const QUALIFYING_RECORDS: u64 = 100_000;
pub const ACCEPTED_SEED: [u8; 32] = [
    0x8f, 0x41, 0xd0, 0xa5, 0x2b, 0x40, 0xf1, 0x3f, 0x4a, 0x77, 0xbc, 0x3b, 0xea, 0xe2, 0x02, 0x6a,
    0x8b, 0xc4, 0x2a, 0xd4, 0x8d, 0x12, 0xce, 0x53, 0xd9, 0x2e, 0x29, 0xf6, 0x12, 0x11, 0x10, 0x06,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bm06Profile {
    records: u64,
}

impl Bm06Profile {
    pub fn new(records: u64) -> Result<Self, &'static str> {
        if !(1..=QUALIFYING_RECORDS).contains(&records) {
            return Err("BM-06 records must be between 1 and 100000");
        }
        Ok(Self { records })
    }
    pub fn records(self) -> u64 {
        self.records
    }
    pub fn events(self) -> u64 {
        self.records * VERSIONS
    }
    pub fn batches_per_version(self) -> u64 {
        self.records.div_ceil(BATCH_RECORDS)
    }
    /// Revision one is reserved for the normal durable authorization policy bootstrap.
    pub fn frontier(self) -> u64 {
        1 + VERSIONS * self.batches_per_version()
    }
    /// The last full generation before the final update generation.
    pub fn checkpoint_revision(self) -> u64 {
        1 + (VERSIONS - 1) * self.batches_per_version()
    }
    pub fn history_payload_bytes(self) -> u64 {
        self.events() * PAYLOAD_BYTES as u64
    }
    pub fn event_revision(self, ordinal: u64) -> Result<u64, &'static str> {
        if ordinal >= self.events() {
            return Err("BM-06 event ordinal outside profile");
        }
        Ok(2 + ordinal / self.records * self.batches_per_version()
            + ordinal % self.records / BATCH_RECORDS)
    }

    pub fn record_ref(self, scope: NamespaceRef, record: u64) -> Result<RecordRef, &'static str> {
        if record >= self.records {
            return Err("BM-06 record ordinal outside profile");
        }
        let mut id = [0_u8; 16];
        // A fixed type/profile-separated prefix and ordinal are collision-free, not an opaque
        // identity scheme for real user data. These are synthetic fixture IDs only.
        id[..8].copy_from_slice(b"BM06ENT1");
        id[8..].copy_from_slice(&record.to_be_bytes());
        Ok(RecordRef::new(
            scope.database(),
            scope.namespace(),
            RecordId::from_bytes(id),
        ))
    }

    /// Exactly byte-compatible with the accepted synthetic-v1 events identity stream.
    pub fn event_identity(self, ordinal: u64) -> Result<[u8; 32], &'static str> {
        if ordinal >= self.events() {
            return Err("BM-06 event ordinal outside profile");
        }
        let mut hash = blake3::Hasher::new_keyed(&ACCEPTED_SEED);
        hash.update(b"USTE synthetic-v1 record");
        hash.update(b"events");
        hash.update(&ordinal.to_le_bytes());
        Ok(*hash.finalize().as_bytes())
    }

    /// One bounded, nonconstant payload. The ordinal is explicit; the remaining bytes are a
    /// domain-separated BLAKE3 XOF of the accepted synthetic event identity.
    pub fn payload(self, ordinal: u64) -> Result<[u8; PAYLOAD_BYTES], &'static str> {
        let identity = self.event_identity(ordinal)?;
        let mut output = [0_u8; PAYLOAD_BYTES];
        output[..8].copy_from_slice(&ordinal.to_le_bytes());
        let mut hash = blake3::Hasher::new_derive_key("USTE BM-06 materialization-v1 payload");
        hash.update(&identity);
        hash.finalize_xof().fill(&mut output[8..]);
        Ok(output)
    }

    /// At most 512 distinct records and 2 MiB raw payload; no batch crosses a generation.
    /// Resume is random access by durable revision, without retaining earlier operations.
    pub fn batch(self, scope: NamespaceRef, revision: u64) -> Result<Vec<Operation>, String> {
        if !(2..=self.frontier()).contains(&revision) {
            return Err("BM-06 materialization revision outside profile".into());
        }
        let batch = revision - 2;
        let generation = batch / self.batches_per_version();
        let first = batch % self.batches_per_version() * BATCH_RECORDS;
        (first..(first + BATCH_RECORDS).min(self.records))
            .map(|record| {
                let target = self.record_ref(scope, record)?;
                let properties =
                    Value::bytes(self.payload(generation * self.records + record)?.to_vec())
                        .map_err(|_| "BM-06 payload encoding refused")?;
                Ok(if generation == 0 {
                    Operation::Create {
                        expected: Expected::Absent,
                        record: NewRecord::Entity(NewEntity {
                            id: target,
                            entity_type: BoundedString::new(PROFILE.into())
                                .map_err(|_| "BM-06 type encoding refused")?,
                            schema_version: 1,
                            properties,
                        }),
                    }
                } else {
                    Operation::ReplaceEntity {
                        target,
                        expected: Expected::Version(
                            RecordVersion::new(generation).map_err(|_| "BM-06 version refused")?,
                        ),
                        properties,
                    }
                })
            })
            .collect()
    }

    /// Hashes the 48-byte synthetic records, not the 4096-byte materialized payloads or their
    /// canonical transaction encodings. It never claims to hash a constructed database.
    pub fn synthetic_stream_digest(self) -> [u8; 32] {
        let mut hash = blake3::Hasher::new_derive_key("USTE synthetic-v1 dataset");
        hash.update(b"USTE-SYNTHETIC-V1\0");
        hash.update(&[6]);
        hash.update(b"events");
        hash.update(&self.events().to_le_bytes());
        for ordinal in 0..self.events() {
            hash.update(&ordinal.to_le_bytes());
            hash.update(
                &self
                    .event_identity(ordinal)
                    .expect("profile-bounded ordinal"),
            );
            hash.update(&(ordinal / 10_000).to_le_bytes());
        }
        *hash.finalize().as_bytes()
    }

    pub fn manifest(self) -> String {
        let digest: String = self
            .synthetic_stream_digest()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        format!(
            "{{\"profile\":\"{PROFILE}\",\"engine_benchmark\":false,\"qualification\":\"{}\",\"records\":{},\"events\":{},\"versions_per_record\":{VERSIONS},\"payload_bytes_per_event\":{PAYLOAD_BYTES},\"history_payload_bytes\":{},\"batch_records\":{BATCH_RECORDS},\"frontier_revision\":{},\"checkpoint_revision\":{},\"tail_events\":{},\"tail_revisions\":{},\"synthetic_stream_digest\":\"{digest}\",\"canonical_request_stream_digest\":null,\"database_materialized\":false,\"recovery_trials_run\":0}}\n",
            if self.records == QUALIFYING_RECORDS {
                "qualifying-size-fixture-only"
            } else {
                "nonqualifying-small-scale"
            },
            self.records,
            self.events(),
            self.history_payload_bytes(),
            self.frontier(),
            self.checkpoint_revision(),
            self.records,
            self.batches_per_version(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uste_graph::{GraphState, GraphTransaction, Record, encode_transaction};
    use uste_txn::{CheckpointState, TransactionState};
    use uste_types::{CommitRevision, DatabaseId, NamespaceId};

    fn scope() -> NamespaceRef {
        NamespaceRef::new(
            DatabaseId::from_bytes([6; 16]),
            NamespaceId::from_bytes([6; 16]),
        )
    }

    #[test]
    fn bm06_exact_profile_has_ten_million_real_versions_and_a_bounded_tail() {
        let profile = Bm06Profile::new(100_000).unwrap();
        assert_eq!(profile.events(), 10_000_000);
        assert_eq!(profile.history_payload_bytes(), 40_960_000_000);
        assert!(profile.history_payload_bytes() > 24 * 1024 * 1024 * 1024);
        assert_eq!(profile.frontier(), 19_601);
        assert_eq!(profile.checkpoint_revision(), 19_405);
        assert_eq!(profile.batches_per_version(), 196);
        for bad in [0, 100_001, u64::MAX] {
            assert!(Bm06Profile::new(bad).is_err());
        }
        assert!(profile.batch(scope(), 1).is_err());
        assert!(profile.batch(scope(), 19_602).is_err());
        assert!(profile.payload(10_000_000).is_err());
        assert!(profile.record_ref(scope(), 100_000).is_err());
        assert!(profile.event_revision(u64::MAX).is_err());
        for revision in [2, 197, 198, 19_405, 19_406, 19_601] {
            let batch = profile.batch(scope(), revision).unwrap();
            assert!(!batch.is_empty() && batch.len() <= 512);
            let encoded = encode_transaction(&GraphTransaction::new(scope(), batch)).unwrap();
            assert!(encoded.len() < 3 * 1024 * 1024);
        }
    }

    #[test]
    fn bm06_bounded_batches_resume_exactly_across_generation_boundaries() {
        for records in [1, 511, 512, 513, 100_000] {
            let profile = Bm06Profile::new(records).unwrap();
            for ordinal in [0, records - 1, records, profile.events() - 1] {
                let revision = profile.event_revision(ordinal).unwrap();
                let operations = profile.batch(scope(), revision).unwrap();
                let position = (ordinal % records % BATCH_RECORDS) as usize;
                let target = profile.record_ref(scope(), ordinal % records).unwrap();
                let properties = Value::bytes(profile.payload(ordinal).unwrap().to_vec()).unwrap();
                match &operations[position] {
                    Operation::Create {
                        record: NewRecord::Entity(entity),
                        expected,
                    } => {
                        assert_eq!(*expected, Expected::Absent);
                        assert_eq!(entity.id, target);
                        assert_eq!(entity.properties, properties);
                        assert!(ordinal < records);
                    }
                    Operation::ReplaceEntity {
                        target: actual,
                        expected,
                        properties: actual_properties,
                    } => {
                        assert_eq!(*actual, target);
                        assert_eq!(
                            *expected,
                            Expected::Version(RecordVersion::new(ordinal / records).unwrap())
                        );
                        assert_eq!(*actual_properties, properties);
                    }
                    _ => panic!("unexpected fixture operation"),
                }
            }
        }
    }

    #[test]
    fn bm06_history_and_checkpoint_suffix_match_literal_version_oracle() {
        let profile = Bm06Profile::new(2).unwrap();
        let mut state = GraphState::new(scope());
        // Exercise the actual policy-only bootstrap shape. This reducer unit test still does
        // not claim authorized durable publication or native recovery measurement.
        let bootstrap = encode_transaction(&GraphTransaction::with_policy_mutation(
            scope(),
            Vec::new(),
            uste_graph::DurablePolicyMutation::Install {
                policy: crate::engine::benchmark_policy(scope()).unwrap(),
            },
        ))
        .unwrap();
        let prepared = state
            .prepare(&bootstrap, None, CommitRevision::FIRST)
            .unwrap();
        state.publish(prepared);
        let mut resumed = None;
        for revision in 2..=profile.frontier() {
            let request = encode_transaction(&GraphTransaction::new(
                scope(),
                profile.batch(scope(), revision).unwrap(),
            ))
            .unwrap();
            let revision = CommitRevision::new(revision).unwrap();
            let prepared = state.prepare(&request, None, revision).unwrap();
            state.publish(prepared);
            if let Some(recovered) = &mut resumed {
                let recovered: &mut GraphState = recovered;
                let prepared = recovered.prepare(&request, None, revision).unwrap();
                recovered.publish(prepared);
            }
            if revision.get() == profile.checkpoint_revision() {
                let encoded = state.encode_current_checkpoint().unwrap();
                resumed = Some(GraphState::decode_checkpoint(scope(), revision, &encoded).unwrap());
            }
        }
        let snapshot = state.snapshot();
        assert_eq!(snapshot.records().len(), 2);
        assert_eq!(resumed.unwrap().snapshot(), snapshot);
        for generation in 0..100 {
            for record in 0..2 {
                let at = CommitRevision::new(generation + 2).unwrap();
                let Record::Entity(actual) = snapshot
                    .record_at(at, profile.record_ref(scope(), record).unwrap())
                    .unwrap()
                    .unwrap()
                else {
                    panic!("expected entity history");
                };
                assert_eq!(actual.version.get(), generation + 1);
                assert_eq!(actual.created_revision, CommitRevision::new(2).unwrap());
                assert_eq!(actual.modified_revision, at);
                let Value::Bytes(bytes) = &actual.properties else {
                    panic!("expected bytes")
                };
                assert_eq!(bytes.as_slice().len(), 4096);
                assert_eq!(
                    &bytes.as_slice()[..8],
                    &(generation * 2 + record).to_le_bytes()
                );
            }
        }
    }

    #[test]
    fn bm06_materialization_stream_golden() {
        let profile = Bm06Profile::new(2).unwrap();
        let mut hash = blake3::Hasher::new_derive_key("USTE BM-06 canonical-request-stream-v1");
        for revision in 2..=profile.frontier() {
            let request = encode_transaction(&GraphTransaction::new(
                scope(),
                profile.batch(scope(), revision).unwrap(),
            ))
            .unwrap();
            hash.update(&revision.to_le_bytes());
            hash.update(&(request.len() as u64).to_le_bytes());
            hash.update(&request);
        }
        assert_eq!(
            hash.finalize().to_hex().as_str(),
            "23f20f640042954cbf498ab4d63cc8e1eee0b2a184e0c0076ad838aab961daf9"
        );
        assert_eq!(
            blake3::Hash::from(profile.synthetic_stream_digest())
                .to_hex()
                .as_str(),
            "b0cc0a82916763e1a764bd1e5611261de407487a5936677ce87cd9d0046ce0fe"
        );
        assert_eq!(
            blake3::hash(&profile.payload(0).unwrap()).to_hex().as_str(),
            "b9585a3cdd160dd844fcda780638a9efa6e1d95d35c1da10b62fe0edc528297e"
        );
    }

    #[test]
    fn bm06_exact_synthetic_stream_matches_independent_generator_golden() {
        assert_eq!(
            blake3::Hash::from(Bm06Profile::new(100_000).unwrap().synthetic_stream_digest())
                .to_hex()
                .as_str(),
            "c2d3b2fa0ce5d228213d03eb3fe4885fb11f41edcd6bc6935a8fb4b9702d5c3a"
        );
    }
}
