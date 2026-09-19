//! Trusted fixture-specific work bounds. Constructing these is not qualifying-size execution.
use super::*;

pub(crate) const CACHE_BYTES: usize = 64 * 1024 * 1024;
// Decision 0015: maximum 16 MiB plaintext plus one small encrypted envelope of padding/overhead.
const MAX_ENCODED_GROUP: u64 = uste_crypto::MAX_PLAINTEXT_BYTES as u64 + 4_161;
// visit_committed_range debits the fixed certificate as well as the group envelope.
const MAX_CERTIFIED_GROUP: u64 = MAX_ENCODED_GROUP + 4_161;
const ENTRY_BYTES: u64 = 16 * 1024;

#[derive(Clone, Copy)]
pub(crate) struct DiskProfileLimits {
    pub groups: u64,
    pub prefix_bytes: u64,
    pub suffix_bytes: u64,
    pub blob_recovery: uste_storage::journal::BlobRecoveryLimits,
    pub transaction_run: IndexRunReadLimits,
    pub metadata: uste_txn::CoordinatorMetadataLoadLimits,
    pub graph: GraphDiskBaseAdmissionLimits,
    pub preparation: GraphDiskPreparationLimits,
    pub merge_read: IndexRunReadLimits,
    pub merge: IndexRunMergeLimits,
}

impl DiskProfileLimits {
    pub fn new(profile: Bm01Profile) -> Result<Self, String> {
        // Profile validation bounds E <= 100,000 and R = 10E. The arithmetic below is also
        // checked so changing that contract cannot silently wrap a work allowance.
        let mul = |a: u64, b: u64| a.checked_mul(b).ok_or("disk profile limit overflow");
        let add = |a: u64, b: u64| a.checked_add(b).ok_or("disk profile limit overflow");
        let counts = fixture_state_counts(profile);
        let entries = counts.into_iter().try_fold(1_u64, add)?;
        let largest_family = *counts.iter().max().ok_or("missing fixture families")?;
        let groups = materialization_revision_count(profile);
        let proof_bytes = |count: u64| mul(mul(count, add(count, 1)?)? / 2, 4_161);
        let prefix_bytes = add(mul(groups, MAX_CERTIFIED_GROUP)?, proof_bytes(groups)?)?;
        // This closed graph fixture prohibits blob inventories. The storage catalog therefore
        // has exactly one metadata entry, not a hidden all-history blob map or a relaxed cap.
        let blob_run = IndexRunReadLimits::new(1, 1, 68).map_err(debug)?;
        let blob_recovery = uste_storage::journal::BlobRecoveryLimits {
            catalog: uste_storage::journal::BlobMetadataRebuildLimits {
                admission: uste_storage::journal::BlobMetadataAdmissionLimits {
                    maximum_blobs: 0,
                    maximum_namespaces: 0,
                    maximum_inventories: 0,
                    maximum_reference_bindings: 0,
                    run: blob_run,
                    // One binary-search visit plus one entry-read visit, even on cache hit.
                    lookup: IndexGetLimits::new(2, 48).map_err(debug)?,
                    certificates: uste_storage::journal::CertificateAnchorReadLimits::new(
                        groups,
                        mul(groups, 4_161)?,
                    )
                    .map_err(debug)?,
                    maximum_journal_groups: groups,
                    maximum_journal_encoded_bytes: prefix_bytes,
                },
                merge: IndexRunMergeLimits::new(blob_run, 1, 68, 1, 68).map_err(debug)?,
                maximum_merge_output_bytes: 68,
            },
            catalog_recovery: uste_storage::journal::BlobCatalogRecovery::AdmitOrRebuild,
            maximum_verified_blob_bytes_per_pass: 0,
            maximum_uncommitted_segment_tails: usize::try_from(groups).map_err(debug)?,
        };
        let transaction_run =
            IndexRunReadLimits::new(add(mul(groups, 2)?, 16)?, groups, mul(groups, 256)?)
                .map_err(debug)?;
        let metadata = uste_txn::CoordinatorMetadataLoadLimits::new(
            groups,
            0,
            add(groups, 1)?,
            add(mul(groups, 2)?, 32)?,
            mul(add(groups, 1)?, 256)?,
        )
        .map_err(debug)?;
        // Each fixture key/value fits below 16 KiB. Two pages per entry plus fixed per-run
        // headroom is conservative; these are scan/work ceilings, not buffer reservations.
        let scan = GraphStateLoadLimits::new(
            counts[0],
            counts[1],
            1,
            entries,
            add(mul(entries, 2)?, 128)?,
            mul(entries, ENTRY_BYTES)?,
        )
        .map_err(debug)?;
        // Includes history/current closure and owner-local secondary checks. Keep substantial
        // declared headroom; measurements, not these bounds, determine campaign performance.
        let lookups = mul(counts[0], 32)?;
        let graph = GraphDiskBaseAdmissionLimits::new(
            scan,
            2,
            64 * 1024,
            mul(profile.relationships(), 64)?,
            lookups,
            mul(lookups, 64)?,
            mul(lookups, ENTRY_BYTES)?,
            IndexPredecessorLimits::new(64, ENTRY_BYTES as usize).map_err(debug)?,
        )
        .map_err(debug)?;
        let batch = MAX_TRANSACTION_OPERATIONS as u64;
        let relationship_batch = profile.relationships().min(batch);
        let proofs = add(
            add(
                relationship_batch,
                mul(relationship_batch, 2)?.min(profile.entities()),
            )?,
            1,
        )?
        .max(add(profile.entities(), 1)?.min(batch));
        // The fixed null-property fixture needs no ReadView history or deletion reverse bucket.
        // Its stored record proofs plus ID accounting fit within 1 KiB each; allow 1 MiB policy
        // headroom. General domain requests must not be routed through these fixture limits.
        let preparation = GraphDiskPreparationLimits::new(
            proofs,
            1_000_000,
            0,
            0,
            add(mul(proofs, 1024)?, 1024 * 1024)?,
        )
        .map_err(debug)?;
        let merge_read = IndexRunReadLimits::new(
            add(mul(largest_family, 2)?, 16)?,
            largest_family,
            mul(largest_family, ENTRY_BYTES)?,
        )
        .map_err(debug)?;
        let merge = IndexRunMergeLimits::new(
            merge_read,
            1_000_000,
            64 * 1024 * 1024,
            largest_family,
            mul(largest_family, ENTRY_BYTES)?,
        )
        .map_err(debug)?;
        Ok(Self {
            groups,
            prefix_bytes,
            suffix_bytes: add(
                mul(groups - 1, MAX_CERTIFIED_GROUP)?,
                proof_bytes(groups - 1)?,
            )?,
            blob_recovery,
            transaction_run,
            metadata,
            graph,
            preparation,
            merge_read,
            merge,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_bounds_validate_the_full_accepted_range_without_database_work() {
        for entities in 2..=100_000 {
            DiskProfileLimits::new(Bm01Profile::new(entities).unwrap()).unwrap();
        }
        let limits = DiskProfileLimits::new(Bm01Profile::qualifying()).unwrap();
        assert_eq!(limits.groups, 212);
        assert_eq!(limits.blob_recovery.catalog.admission.maximum_blobs, 0);
        assert_eq!(
            limits
                .blob_recovery
                .catalog
                .admission
                .maximum_reference_bindings,
            0
        );
        assert_eq!(limits.blob_recovery.maximum_verified_blob_bytes_per_pass, 0);
        assert_eq!(limits.blob_recovery.catalog.maximum_merge_output_bytes, 68);
        assert_eq!(
            limits
                .blob_recovery
                .catalog
                .admission
                .lookup
                .maximum_page_visits(),
            2
        );
        assert_eq!(
            limits
                .blob_recovery
                .catalog
                .admission
                .maximum_journal_encoded_bytes,
            limits.prefix_bytes
        );
        assert_eq!(limits.prefix_bytes, 212 * 16_785_538 + 212 * 213 / 2 * 4161);
        assert_eq!(limits.suffix_bytes, 211 * 16_785_538 + 211 * 212 / 2 * 4161);
        assert_eq!(limits.transaction_run.maximum_entries(), 212);
        assert_eq!(limits.merge_read.maximum_entries(), 3_000_000);
        assert_eq!(
            limits.preparation,
            GraphDiskPreparationLimits::new(30_001, 1_000_000, 0, 0, 30_001 * 1024 + 1024 * 1024,)
                .unwrap()
        );
        assert!(limits.prefix_bytes > 128 * 1024 * 1024);
    }

    #[test]
    fn certified_range_allowance_includes_the_maximum_group_and_its_certificate() {
        use uste_storage::journal::{CommitInput, CreationOptions, JournalStore, StorageError};
        let mut fs = MemoryFileSystem::new(128 * 1024 * 1024);
        let vault =
            KeyVault::create(scope().database(), &mut TestKeyAdapter, CounterEntropy(71)).unwrap();
        let mut store = JournalStore::create(
            &mut fs,
            CreationOptions {
                database: scope().database(),
                final_name: EntryName::new("range-boundary").unwrap(),
            },
            vault,
            CounterEntropy(300),
        )
        .unwrap();
        let payload = vec![0x55; uste_crypto::MAX_PLAINTEXT_BYTES];
        let committed = store
            .append_group(
                &mut fs,
                CommitInput {
                    encoded_group: &payload,
                    logical_event_digest: [9; 32],
                },
            )
            .unwrap();
        for bytes in [MAX_ENCODED_GROUP, MAX_CERTIFIED_GROUP - 1] {
            assert_eq!(
                store.visit_committed_range(
                    &mut fs,
                    committed.revision,
                    committed.revision,
                    1,
                    bytes,
                    |_, _| panic!("short byte budget must fail before callback")
                ),
                Err(StorageError::ResourceLimit)
            );
        }
        let mut visited = 0;
        store
            .visit_committed_range(
                &mut fs,
                committed.revision,
                committed.revision,
                1,
                MAX_CERTIFIED_GROUP,
                |_, group| {
                    assert_eq!(group.encoded_group, payload);
                    visited += 1;
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(visited, 1);
    }

    #[test]
    fn fixture_stored_records_fit_the_declared_per_proof_byte_allowance() {
        use uste_graph::{
            EntityLifecycle, EntityRecord, EvidenceRecord, Record, RelationshipRecord,
            encode_stored_record,
        };
        let revision = uste_types::CommitRevision::new(u64::MAX).unwrap();
        let materializer = Materializer::new(Bm01Profile::qualifying());
        let from = entity_ref(scope(), materializer, 99_998);
        let to = entity_ref(scope(), materializer, 99_999);
        let mut records = vec![
            Record::Entity(EntityRecord {
                id: from,
                version: RecordVersion::FIRST,
                lifecycle: EntityLifecycle::Active,
                entity_type: text("bm01-entity-v1").unwrap(),
                schema_version: 1,
                properties: Value::Null,
                created_revision: revision,
                modified_revision: revision,
            }),
            Record::Evidence(EvidenceRecord {
                id: evidence_ref(scope()),
                version: RecordVersion::FIRST,
                digest: [255; 32],
                locator: text("bm01-uste-graph-v1").unwrap(),
                created_revision: revision,
            }),
        ];
        for relationship_type in ["bm01-uniform-v1", "bm01-hub-v1", "bm01-ring-v1"] {
            for (version, status) in [
                (1, AssertionStatus::Proposed),
                (2, AssertionStatus::Accepted),
            ] {
                records.push(Record::Relationship(RelationshipRecord {
                    id: relationship_ref(scope(), materializer, 999_999),
                    version: RecordVersion::new(version).unwrap(),
                    from,
                    to,
                    relationship_type: text(relationship_type).unwrap(),
                    properties: Value::Null,
                    evidence: vec![evidence_ref(scope())],
                    status,
                    valid_time: ValidTime::Unknown,
                    correction_of: None,
                    recorded_revision: revision,
                    modified_revision: revision,
                }));
            }
        }
        for record in records {
            // Covers stored bytes plus repeated 16-byte required/pending identity accounting.
            assert!(encode_stored_record(&record).unwrap().len() + 64 <= 1024);
        }
    }
}
