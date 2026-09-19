//! Receipt-bound private graph staging. Publication remains a separate terminal operation.
use super::*;
mod live;
pub use live::{
    GraphPackedLivePublication, GraphPackedLiveSnapshot, GraphPackedLiveState,
    PackedGraphExpansionLimits, PackedGraphReadLimits, PackedGraphSuffixRecoveryLimits,
    PackedGraphSuffixRecoveryReport, PackedGraphWritePreparationLimits,
    PackedGraphWritePublicationLimits, publish_packed_graph_live_base, recover_packed_graph_suffix,
};

pub struct PackedGraphDelta {
    prepared: PackedPreparedGraph,
    data: GraphStateFamilyDelta,
}
impl PackedGraphDelta {
    fn matches_base(&self, base: &PackedGraphBase) -> bool {
        let p = &self.prepared;
        p.prepared.scope == base.scope
            && p.base_anchor == base.anchor
            && p.base_digest == base.publication_claims().state_digest
            && p.base_counts == base.counts
            && p.base_policy == base.policy
            && p.prepared.base_revision == Some(base.anchor.0)
            && p.prepared.base_policy_version == base.policy.as_ref().map(NamespacePolicy::version)
            && self.data.scope == base.scope
            && self.data.base_revision == base.anchor.0
            && Some(self.data.revision) == base.anchor.0.checked_next().ok()
            && self.data.revision == p.prepared.revision
            && self.data.result_digest == p.prepared.result_digest
    }
    pub fn revision(&self) -> CommitRevision {
        self.data.revision
    }
    pub fn result_digest(&self) -> [u8; 32] {
        self.data.result_digest
    }
    pub fn delta_count(&self) -> u64 {
        self.data.deltas
    }
    pub fn logical_bytes(&self) -> u64 {
        self.data.logical_bytes
    }
}
pub fn prepare_packed_graph_delta(
    prepared: PackedPreparedGraph,
    limits: GraphStateDeltaLimits,
) -> Result<PackedGraphDelta, GraphDiskError> {
    let data = build_graph_family_deltas(
        prepared.base_counts,
        prepared.base_policy.as_ref(),
        &prepared.prepared,
        limits,
    )?;
    Ok(PackedGraphDelta { prepared, data })
}

#[derive(Clone, Copy)]
pub struct PackedGraphStageLimits {
    pub certificates: CertificateAnchorReadLimits,
    pub batch: TreeBatchLimits,
    pub deltas_per_batch: usize,
    pub maximum_batches: u64,
    pub maximum_read_pages: u64,
    pub maximum_written_pages: u64,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackedGraphStageReport {
    pub batches: u64,
    pub read_pages: u64,
    pub written_pages: u64,
    pub peak_batch_deltas: usize,
}

pub fn stage_packed_graph_delta<F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    fs: &mut F,
    base: &PackedGraphBase,
    transaction: &RecoveredFrontierTransaction,
    plan: &PackedGraphDelta,
    limits: PackedGraphStageLimits,
) -> Result<(PackedGraphBase, PackedGraphStageReport), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let prepared = &plan.prepared;
    let data = &plan.data;
    if base.scope != recovery.scope()
        || base.scope != prepared.prepared.scope
        || data.scope != base.scope
        || data.base_revision != base.anchor.0
        || base.anchor != prepared.base_anchor
        || base.counts != prepared.base_counts
        || base.policy != prepared.base_policy
        || base.publication_claims().state_digest != prepared.base_digest
        || base.anchor.0.checked_next().ok() != Some(transaction.revision())
        || data.revision != transaction.revision()
        || data.result_digest != transaction.outcome().result_digest
    {
        return Err(GraphDiskError::RootStateMismatch);
    }
    prepared
        .prepared
        .validate_external_request(
            transaction.canonical_request(),
            transaction.blob_inventory(),
            transaction.revision(),
        )
        .map_err(|_| GraphDiskError::RootStateMismatch)?;
    admit_stage(plan, limits)?;
    let mut maintenance = recovery.packed_indexes_with_io(fs, transaction, limits.certificates)?;
    stage_on_maintenance(&mut maintenance, fs, base, plan, limits)
}

fn admit_stage(
    plan: &PackedGraphDelta,
    limits: PackedGraphStageLimits,
) -> Result<u64, GraphDiskError> {
    if limits.deltas_per_batch == 0
        || limits.deltas_per_batch > MAX_BATCH_DELTAS
        || limits.deltas_per_batch > limits.batch.maximum_deltas
    {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    let required = plan.data.families.iter().try_fold(0_u64, |sum, family| {
        checked_sum(
            sum,
            family.len().max(1).div_ceil(limits.deltas_per_batch) as u64,
        )
    })?;
    if required > limits.maximum_batches {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    Ok(required)
}

fn stage_on_maintenance<F, W, E, I>(
    maintenance: &mut PackedIndexMaintenance<'_, F, W, E, I>,
    fs: &mut F,
    base: &PackedGraphBase,
    plan: &PackedGraphDelta,
    limits: PackedGraphStageLimits,
) -> Result<(PackedGraphBase, PackedGraphStageReport), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if !plan.matches_base(base) || maintenance.anchor().0 != plan.revision() {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let required = admit_stage(plan, limits)?;
    let prepared = &plan.prepared;
    let data = &plan.data;
    let expected_counts = parse_metadata(
        &metadata_value_from_counts(data.revision, data.target_counts),
        data.revision,
    )?
    .family_counts()?;
    for tree in &base.trees {
        maintenance.validate_tree_binding(tree)?;
    }
    let mut trees = Vec::with_capacity(8);
    let mut report = PackedGraphStageReport::default();
    for (i, deltas) in data.families.iter().enumerate() {
        let mut tree = base.trees[i].clone();
        let chunks = deltas.len().max(1).div_ceil(limits.deltas_per_batch);
        for chunk in 0..chunks {
            let start = chunk * limits.deltas_per_batch;
            let end = (start + limits.deltas_per_batch).min(deltas.len());
            let deltas = &deltas[start..end];
            let mut batch = limits.batch;
            batch.maximum_read_pages = batch
                .maximum_read_pages
                .min(limits.maximum_read_pages.saturating_sub(report.read_pages));
            // Unique deletion of every remaining key can only produce an empty root or fail CAS.
            // Like an empty delta slice, it needs no pack, even when the aggregate budget is zero.
            let removes_entire_tree = deltas.len() as u64
                == tree.family_descriptor().commitment.entries()
                && deltas.iter().all(|delta| delta.after().is_none());
            if !deltas.is_empty() && !removes_entire_tree {
                batch.pack.maximum_pages = batch.pack.maximum_pages.min(
                    limits
                        .maximum_written_pages
                        .saturating_sub(report.written_pages),
                );
            }
            let staged = maintenance.stage(
                fs,
                GRAPH_PACKED_PROFILE_V1,
                i as u8 + 1,
                Some(&tree),
                deltas,
                batch,
            )?;
            let work = staged.report();
            report.batches += 1;
            report.read_pages = checked_sum(report.read_pages, work.read_pages)?;
            report.written_pages = checked_sum(report.written_pages, work.written_pages)?;
            report.peak_batch_deltas = report.peak_batch_deltas.max(deltas.len());
            tree = staged.tree().clone();
        }
        if tree.family_descriptor().commitment.entries() != expected_counts[i] {
            return Err(GraphDiskError::IndexCorrupt);
        }
        trees.push(tree);
    }
    if report.batches != required {
        return Err(GraphDiskError::IndexCorrupt);
    }
    let trees = trees.try_into().map_err(|_| GraphDiskError::IndexCorrupt)?;
    Ok((
        PackedGraphBase {
            scope: base.scope,
            anchor: maintenance.anchor(),
            reducer: base.reducer,
            trees,
            counts: data.target_counts,
            policy: prepared
                .prepared
                .policy_change
                .clone()
                .or_else(|| prepared.base_policy.clone()),
            source_v1_digest: None,
        },
        report,
    ))
}
