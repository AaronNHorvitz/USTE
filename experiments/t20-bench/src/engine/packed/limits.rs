//! Fixture work ceilings, not allocations, measured residency or qualification evidence.
use super::*;
use uste_graph::{GraphDiskBaseAdmissionLimits, GraphStateLoadLimits};
use uste_storage::{
    IndexPredecessorLimits, packed_index_pack::PackWriteLimits, packed_tree_batch::TreeBatchLimits,
};

#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub counts: [u64; 8],
    pub legacy: disk::DiskProfileLimits,
    pub lookup: TreeLookupLimits,
    pub cursor: TreeCursorLimits,
    pub family: TreeValidationLimits,
    pub graph: PackedGraphAdmissionLimits,
    pub origin: PackedGraphOriginRecoveryLimits,
    pub preparation: PackedGraphWritePreparationLimits,
    pub publication: PackedGraphWritePublicationLimits,
}
impl Limits {
    pub fn new(profile: Bm01Profile) -> Result<Self, String> {
        let legacy = disk::DiskProfileLimits::new(profile)?;
        let counts = disk::fixture_state_counts(profile);
        Self::from_shape(legacy, counts, 2, 64 * 1024, profile.relationships() * 64)
    }
    pub fn recovery(profile: crate::recovery_materialization::Bm06Profile) -> Result<Self, String> {
        use crate::recovery_materialization::VERSIONS;
        Self::from_shape(
            disk::DiskProfileLimits::recovery(profile)?,
            [profile.records(), profile.events(), 0, 0, 0, 0, 1, 1],
            VERSIONS,
            VERSIONS * 16 * 1024,
            1,
        )
    }
    fn from_shape(
        legacy: disk::DiskProfileLimits,
        counts: [u64; 8],
        history_versions: u64,
        history_bytes: u64,
        reference_visits: u64,
    ) -> Result<Self, String> {
        let entries = counts.into_iter().sum::<u64>() + 1;
        let all_entries = entries + 2 * legacy.groups + 4;
        // Keys are at most 48 bytes (retry principal/key); encoded Patricia paths fit 512.
        // These fixture values fit 16 KiB and at most two value chunks.
        let lookup = TreeLookupLimits {
            maximum_path_branches: 512,
            maximum_pages: 516,
            maximum_encoded_bytes: 516 * 20545,
            maximum_value_bytes: 16 * 1024,
        };
        let pages = all_entries * 8 + 512;
        let cursor = TreeCursorLimits {
            maximum_path_branches: 512,
            maximum_candidates: all_entries + 16,
            maximum_returned_bytes: all_entries * 16 * 1024,
            maximum_pages: pages,
            maximum_encoded_bytes: pages * 20545,
        };
        let family = TreeValidationLimits {
            maximum_path_branches: 512,
            maximum_nodes: 2 * all_entries,
            maximum_logical_bytes: all_entries * 16 * 1024,
            maximum_pages: pages,
            maximum_encoded_bytes: pages * 20545,
        };
        let lookups = counts[0] * 64;
        let graph = PackedGraphAdmissionLimits {
            canonical: family,
            semantic: GraphDiskBaseAdmissionLimits::new(
                GraphStateLoadLimits::new(
                    counts[0],
                    counts[1],
                    1,
                    entries,
                    pages,
                    entries * 16 * 1024,
                )
                .map_err(debug)?,
                history_versions,
                history_bytes,
                reference_visits,
                lookups,
                lookups * 516,
                lookups * 16 * 1024,
                IndexPredecessorLimits::new(516, 16 * 1024).map_err(debug)?,
            )
            .map_err(debug)?,
            scan: cursor,
            lookup,
            maximum_lookup_encoded_bytes: lookups * 516 * 20545,
        };
        let certificates = legacy.blob_recovery.catalog.admission.certificates;
        let batch = TreeBatchLimits {
            maximum_deltas: 512,
            maximum_input_bytes: 8 * 1024 * 1024,
            maximum_dirty_nodes: 16384,
            maximum_path_branches: 512,
            maximum_read_pages: 512 * 516,
            maximum_read_bytes: 512 * 516 * 20545,
            pack: PackWriteLimits {
                maximum_pages: 4096,
                maximum_records: 32768,
                maximum_payload_bytes: 64 * 1024 * 1024,
            },
        };
        let stage = PackedGraphStageLimits {
            certificates,
            batch,
            deltas_per_batch: 512,
            maximum_batches: 4096,
            maximum_read_pages: 4096 * 512 * 516,
            maximum_written_pages: 4096 * 4096,
        };
        let proof = PackedGraphPreparationLimits {
            proof: legacy.preparation,
            lookup,
            maximum_point_lookups: 1_000_000,
            maximum_pages: 516_000_000,
            maximum_encoded_bytes: 516_000_000 * 20545,
            maximum_scan_candidates: 1_000_000,
        };
        let delta = GraphStateDeltaLimits::new(1_000_000, 64 * 1024 * 1024).map_err(debug)?;
        let metadata = PackedMetadataRebaseLimits {
            staging: PackedCoordinatorLimits {
                certificates,
                lookup,
                batch,
                maximum_references: 0,
                maximum_owners: 0,
            },
            maximum_groups: legacy.groups,
            maximum_encoded_bytes: legacy.prefix_bytes,
            certificate_window: 64,
            maximum_publication_attempts: 8,
        };
        Ok(Self {
            counts,
            legacy,
            lookup,
            cursor,
            family,
            graph,
            origin: PackedGraphOriginRecoveryLimits {
                maximum_genesis_encoded_bytes: legacy.prefix_bytes,
                genesis: PackedGraphGenesisLimits {
                    stage,
                    maximum_entries: 16,
                    maximum_logical_bytes: 1024 * 1024,
                },
                suffix: PackedGraphSuffixRecoveryLimits {
                    maximum_revisions: legacy.groups - 1,
                    preparation: proof,
                    deltas: delta,
                    graph: stage,
                    metadata,
                },
            },
            preparation: PackedGraphWritePreparationLimits {
                proof,
                delta,
                certificates,
            },
            publication: PackedGraphWritePublicationLimits {
                stage,
                maximum_attempts: 8,
            },
        })
    }
    pub fn read(self) -> Result<PackedGraphReadLimits, String> {
        Ok(PackedGraphReadLimits {
            current: self.lookup,
            historical: TreeCursorLimits {
                maximum_path_branches: 512,
                maximum_candidates: 2,
                maximum_returned_bytes: 16 * 1024 + 24,
                maximum_pages: 1024,
                maximum_encoded_bytes: 1024 * 20545,
            },
            expansion: Some(
                PackedGraphExpansionLimits::new(
                    516_000_000,
                    516_000_000 * 20545,
                    1_000_000,
                    64 * 1024 * 1024,
                    2_000_000,
                )
                .map_err(debug)?,
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packed_history_shape_keeps_frozen_versions_and_bounded_windows() {
        use crate::recovery_materialization::Bm06Profile;
        for records in [1, 2, 511, 512, 513, 100000] {
            let profile = Bm06Profile::new(records).unwrap();
            let limits = Limits::recovery(profile).unwrap();
            assert_eq!(limits.counts, [records, profile.events(), 0, 0, 0, 0, 1, 1]);
            assert_eq!(limits.legacy.groups, profile.frontier());
            assert_eq!(
                limits.origin.suffix.maximum_revisions,
                profile.frontier() - 1
            );
            assert_eq!(limits.origin.suffix.metadata.certificate_window, 64);
            assert_eq!(limits.origin.suffix.metadata.staging.maximum_owners, 0);
            assert_eq!(limits.legacy.history_group_bytes, 100 * 16 * 1024);
            assert_eq!(limits.read().unwrap().historical.maximum_candidates, 2);
        }
    }
    #[test]
    fn packed_profile_arithmetic_and_read_limits_preserve_the_accepted_range() {
        for entities in 2..=100_000 {
            let profile = Bm01Profile::new(entities).unwrap();
            let limits = Limits::new(profile).unwrap();
            limits.read().unwrap();
            assert_eq!(limits.counts, disk::fixture_state_counts(profile));
            assert_eq!(
                limits.origin.suffix.maximum_revisions + 1,
                materialization_revision_count(profile)
            );
            assert_eq!(limits.origin.suffix.metadata.staging.maximum_owners, 0);
            assert_eq!(limits.origin.suffix.metadata.staging.maximum_references, 0);
            assert_eq!(limits.origin.suffix.metadata.certificate_window, 64);
            assert_eq!(limits.publication.stage.deltas_per_batch, 512);
        }
    }
}
