//! Separately typed packed graph cache; never a v1 root or consumer authorization capability.
use super::*;
mod admission;
pub use admission::{
    PackedGraphAdmissionLimits, PackedGraphAdmissionReport, admit_packed_graph_base,
};
mod preparation;
pub use preparation::{
    GraphPackedLivePublication, GraphPackedLiveSnapshot, GraphPackedLiveState, PackedGraphDelta,
    PackedGraphPreparationLimits, PackedGraphReadReport, PackedGraphStageLimits,
    PackedGraphStageReport, PackedGraphSuffixRecoveryLimits, PackedGraphSuffixRecoveryReport,
    PackedPreparedGraph, prepare_packed_graph_delta, prepare_packed_graph_transaction,
    publish_packed_graph_live_base, recover_packed_graph_suffix, stage_packed_graph_delta,
};
use uste_storage::{
    journal::{CanonicalPackedTree, CertificateAnchorReadLimits},
    ordered_commitment::OrderedCommitment,
    packed_root_manifest::{PackedRootClaims, PackedRootFamily},
    packed_tree_batch::{MAX_BATCH_DELTAS, TreeBatchLimits},
    packed_tree_cursor::TreeCursorLimits,
};

pub const GRAPH_PACKED_PROFILE_V1: [u8; 32] = [
    0xc5, 0x2e, 0xf9, 0xc7, 0xca, 0xfb, 0x1e, 0xc5, 0xc4, 0x6b, 0xff, 0x34, 0xe4, 0x54, 0xd3, 0xdf,
    0xa9, 0x75, 0x02, 0x3e, 0xfa, 0x0a, 0x07, 0xa2, 0xe7, 0xc0, 0x7a, 0x47, 0x73, 0xe3, 0xfa, 0xf8,
];
pub const GRAPH_ORDERED_STATE_PROFILE_V1: [u8; 32] = [
    0x56, 0x7b, 0xd8, 0x4c, 0xd5, 0xee, 0x36, 0x40, 0x02, 0xfc, 0x4a, 0x78, 0x1c, 0x7f, 0xf0, 0x4f,
    0x81, 0xab, 0xaf, 0xfa, 0xe9, 0xc6, 0xf2, 0x38, 0xfe, 0xf9, 0xec, 0x8a, 0xba, 0x33, 0xa4, 0xba,
];

pub struct PackedGraphBase {
    scope: NamespaceRef,
    anchor: (CommitRevision, [u8; 32]),
    reducer: [u8; 32],
    trees: [CanonicalPackedTree; 8],
    counts: [u64; 8],
    policy: Option<NamespacePolicy>,
    source_v1_digest: Option<[u8; 32]>,
}
impl PackedGraphBase {
    pub fn scope(&self) -> NamespaceRef {
        self.scope
    }
    pub fn anchor(&self) -> (CommitRevision, [u8; 32]) {
        self.anchor
    }
    pub fn namespace_policy(&self) -> Option<&NamespacePolicy> {
        self.policy.as_ref()
    }
    /// Cached by exact v1 bridging or cold semantic admission; staged states require export.
    pub fn source_v1_digest(&self) -> Option<&[u8; 32]> {
        self.source_v1_digest.as_ref()
    }
    pub fn families(&self) -> [PackedRootFamily; 8] {
        core::array::from_fn(|i| self.trees[i].family_descriptor())
    }
    pub fn publication_claims(&self) -> PackedRootClaims {
        PackedRootClaims {
            revision: self.anchor.0,
            generation: 1,
            certificate_digest: self.anchor.1,
            reducer_profile: self.reducer,
            state_commitment_profile: GRAPH_ORDERED_STATE_PROFILE_V1,
            state_digest: ordered_state_digest(
                self.scope,
                self.anchor.0,
                self.reducer,
                core::array::from_fn(|i| self.trees[i].family_descriptor().commitment),
            ),
        }
    }
}
fn ordered_state_digest(
    scope: NamespaceRef,
    revision: CommitRevision,
    reducer: [u8; 32],
    families: [OrderedCommitment; 8],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"USTE-GRAPH-ORDERED-STATE-V1\0");
    hash.update(scope.database().as_bytes());
    hash.update(scope.namespace().as_bytes());
    hash.update(revision.get().to_be_bytes());
    hash.update(reducer);
    hash.update(GRAPH_PACKED_PROFILE_V1);
    for (i, family) in families.into_iter().enumerate() {
        hash.update([i as u8 + 1]);
        hash.update(family.entries().to_be_bytes());
        hash.update(family.logical_bytes().to_be_bytes());
        hash.update(family.digest());
    }
    hash.finalize().into()
}

#[cfg(test)]
mod vectors {
    use super::*;
    use uste_storage::ordered_commitment::{CommitmentContext, empty_commitment};
    use uste_types::{DatabaseId, NamespaceId};
    #[test]
    fn packed_graph_profile_and_ordered_state_literal_vector() {
        assert_eq!(
            <[u8; 32]>::from(Sha256::digest(b"USTE graph-packed-v1")),
            GRAPH_PACKED_PROFILE_V1
        );
        assert_eq!(
            <[u8; 32]>::from(Sha256::digest(b"USTE graph-ordered-state-v1")),
            GRAPH_ORDERED_STATE_PROFILE_V1
        );
        let scope = NamespaceRef::new(
            DatabaseId::from_bytes([1; 16]),
            NamespaceId::from_bytes([2; 16]),
        );
        let families = core::array::from_fn(|i| {
            empty_commitment(
                CommitmentContext::new(scope, GRAPH_PACKED_PROFILE_V1, i as u8 + 1).unwrap(),
            )
        });
        let actual =
            ordered_state_digest(scope, CommitRevision::new(7).unwrap(), [3; 32], families);
        assert_eq!(
            actual,
            [
                0xbc, 0x4a, 0x4f, 0x2e, 0x69, 0xc0, 0xfc, 0xf4, 0x22, 0x6b, 0xdb, 0x0b, 0x88, 0x3d,
                0xeb, 0x9e, 0xc6, 0x07, 0x3a, 0x9b, 0xb3, 0x57, 0x33, 0xe3, 0x82, 0x3b, 0xe7, 0x71,
                0xb2, 0x06, 0x4a, 0xbf
            ]
        );
        assert_ne!(
            ordered_state_digest(scope, CommitRevision::new(8).unwrap(), [3; 32], families),
            actual
        );
        assert_ne!(
            ordered_state_digest(scope, CommitRevision::new(7).unwrap(), [4; 32], families),
            actual
        );
        let mut changed = families;
        changed.swap(0, 1);
        assert_ne!(
            ordered_state_digest(scope, CommitRevision::new(7).unwrap(), [3; 32], changed),
            actual
        );
    }
}

#[derive(Clone, Copy)]
pub struct PackedGraphBridgeLimits {
    pub certificates: CertificateAnchorReadLimits,
    /// Aggregate source family scan budget, not a per-family allowance.
    pub source: IndexRunReadLimits,
    pub batch: TreeBatchLimits,
    pub entries_per_batch: usize,
    pub maximum_batches: u64,
    pub maximum_packed_read_pages: u64,
    pub maximum_written_pages: u64,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackedGraphBridgeReport {
    pub source_entries: u64,
    pub source_logical_bytes: u64,
    pub source_pages: u64,
    pub batches: u64,
    pub peak_batch_entries: usize,
    pub packed_read_pages: u64,
    pub written_pages: u64,
}

/// Trusted cache bridge. No publication or complete source-state reconstruction occurs here.
pub fn bridge_graph_base_to_packed<F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    fs: &mut F,
    source: &GraphDiskBase,
    transaction: &RecoveredFrontierTransaction,
    limits: PackedGraphBridgeLimits,
) -> Result<(PackedGraphBase, PackedGraphBridgeReport), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let root = &source.root.root;
    if source.scope() != recovery.scope()
        || source.revision() != transaction.revision()
        || root.certificate_digest() != transaction.certificate_digest()
    {
        return Err(GraphDiskError::RootStateMismatch);
    }
    if limits.entries_per_batch == 0
        || limits.entries_per_batch > MAX_BATCH_DELTAS
        || limits.entries_per_batch > limits.batch.maximum_deltas
    {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    let mut required = PackedGraphBridgeReport::default();
    for family in 1..=8 {
        let run = root.runs().find(|run| run.family() == family);
        let entries = run.map_or(0, |run| run.entry_count());
        required.batches = checked_sum(
            required.batches,
            entries.max(1).div_ceil(limits.entries_per_batch as u64),
        )?;
        if let Some(run) = run {
            required.source_entries = checked_sum(required.source_entries, entries)?;
            required.source_pages = checked_sum(required.source_pages, run.page_count())?;
        }
    }
    if required.batches > limits.maximum_batches
        || required.source_entries > limits.source.maximum_entries()
        || required.source_pages > limits.source.maximum_pages()
    {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    // Authenticate the receipt before any cache output, including empty-family stages.
    recovery.packed_indexes_with_io(fs, transaction, limits.certificates)?;
    let mut trees = Vec::with_capacity(8);
    let mut report = PackedGraphBridgeReport::default();
    for family in 1..=8 {
        let mut cursor = if root.runs().any(|run| run.family() == family) {
            let remaining = IndexRunReadLimits::new(
                limits.source.maximum_pages() - report.source_pages,
                limits.source.maximum_entries() - report.source_entries,
                limits.source.maximum_logical_bytes() - report.source_logical_bytes,
            )?;
            Some(recovery.open_index_run_cursor(fs, root, family, remaining)?)
        } else {
            None
        };
        let mut tree = None;
        let mut done = false;
        while !done {
            let mut deltas = Vec::new();
            deltas
                .try_reserve_exact(limits.entries_per_batch)
                .map_err(|_| GraphDiskError::Storage(StorageError::ResourceLimit))?;
            let mut batch_bytes = 0_u64;
            for _ in 0..limits.entries_per_batch {
                let entry = if let Some(cursor) = &mut cursor {
                    recovery.next_index_run_entry(fs, cursor)?
                } else {
                    None
                };
                let Some(entry) = entry else {
                    done = true;
                    break;
                };
                batch_bytes =
                    checked_sum(batch_bytes, (entry.key.len() + entry.value.len()) as u64)?;
                if batch_bytes
                    > limits
                        .batch
                        .maximum_input_bytes
                        .min(uste_storage::packed_tree_batch::MAX_BATCH_INPUT_BYTES)
                {
                    return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
                }
                deltas.push(IndexDelta::new(entry.key, None, Some(entry.value))?);
            }
            if deltas.is_empty() && tree.is_some() {
                break;
            }
            let mut batch_limits = limits.batch;
            batch_limits.maximum_read_pages = batch_limits.maximum_read_pages.min(
                limits
                    .maximum_packed_read_pages
                    .saturating_sub(report.packed_read_pages),
            );
            if !deltas.is_empty() {
                batch_limits.pack.maximum_pages = batch_limits.pack.maximum_pages.min(
                    limits
                        .maximum_written_pages
                        .saturating_sub(report.written_pages),
                );
            }
            let staged = recovery
                .packed_indexes_with_io(fs, transaction, limits.certificates)?
                .stage(
                    fs,
                    GRAPH_PACKED_PROFILE_V1,
                    family,
                    tree.as_ref(),
                    &deltas,
                    batch_limits,
                )?;
            let work = staged.report();
            report.batches += 1;
            report.peak_batch_entries = report.peak_batch_entries.max(deltas.len());
            report.packed_read_pages = checked_sum(report.packed_read_pages, work.read_pages)?;
            report.written_pages = checked_sum(report.written_pages, work.written_pages)?;
            tree = Some(staged.tree().clone());
        }
        if let Some(cursor) = cursor {
            let read = recovery.finish_index_run_cursor(cursor)?;
            report.source_entries = checked_sum(report.source_entries, read.entries)?;
            report.source_logical_bytes =
                checked_sum(report.source_logical_bytes, read.logical_bytes)?;
            report.source_pages = checked_sum(report.source_pages, read.stats.pages_read)?;
        }
        trees.push(tree.ok_or(GraphDiskError::IndexCorrupt)?);
    }
    if report.source_entries != required.source_entries
        || report.source_pages != required.source_pages
        || report.batches != required.batches
    {
        return Err(GraphDiskError::IndexCorrupt);
    }
    let trees = trees.try_into().map_err(|_| GraphDiskError::IndexCorrupt)?;
    Ok((
        PackedGraphBase {
            scope: source.scope(),
            anchor: (source.revision(), *root.certificate_digest()),
            reducer: *root.reducer_profile(),
            trees,
            counts: source.counts,
            policy: source.current_policy.clone(),
            source_v1_digest: Some(*root.logical_state_digest()),
        },
        report,
    ))
}

/// Reproduce the frozen v1 digest, never substitute the ordered commitment for it.
pub fn packed_graph_v1_digest<F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    fs: &mut F,
    base: &PackedGraphBase,
    transaction: &RecoveredFrontierTransaction,
    certificates: CertificateAnchorReadLimits,
    limits: TreeCursorLimits,
    maximum_history_group_bytes: u64,
) -> Result<[u8; 32], GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if base.scope != recovery.scope()
        || base.anchor != (transaction.revision(), *transaction.certificate_digest())
    {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let maintenance = recovery.packed_indexes_with_io(fs, transaction, certificates)?;
    for tree in &base.trees {
        maintenance.validate_tree_binding(tree)?;
    }
    let mut validator = MergedGraphStateValidator::new(
        base.scope,
        base.anchor.0,
        base.counts,
        maximum_history_group_bytes,
    )?;
    let mut remaining = limits;
    for (i, tree) in base.trees.iter().enumerate() {
        let mut cursor = maintenance.cursor(tree, b"", None, remaining)?;
        while let Some(entry) = maintenance.next(fs, &mut cursor)? {
            validator
                .observe(i as u8 + 1, entry.key(), entry.value())
                .map_err(GraphDiskError::Storage)?;
        }
        let work = cursor.report();
        remaining.maximum_candidates -= work.candidates;
        remaining.maximum_returned_bytes -= work.returned_bytes;
        remaining.maximum_pages -= work.pages;
        remaining.maximum_encoded_bytes -= work.encoded_bytes;
        validator.finish_family(i as u8 + 1)?;
    }
    validator.finish()
}
