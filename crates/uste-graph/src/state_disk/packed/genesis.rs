//! Private first-transaction staging, without v1 derived roots or journal changes.
use super::*;
use uste_txn::RecoveredGenesis;

#[derive(Clone, Copy)]
pub struct PackedGraphGenesisLimits {
    pub stage: PackedGraphStageLimits,
    pub maximum_entries: u64,
    pub maximum_logical_bytes: u64,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackedGraphGenesisReport {
    pub entries: u64,
    pub logical_bytes: u64,
    pub stage: PackedGraphStageReport,
}

/// Trusted private cache staging. Only the already-verified first transaction is resident.
/// No journal append or root publication; failure may leave unreachable immutable scratch packs.
pub fn stage_packed_graph_genesis<F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    fs: &mut F,
    genesis: &RecoveredGenesis<GraphState>,
    limits: PackedGraphGenesisLimits,
) -> Result<(PackedGraphBase, PackedGraphGenesisReport), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let snapshot = genesis.state().current_snapshot();
    let transaction = genesis.transaction();
    if snapshot.scope() != recovery.scope()
        || snapshot.revision() != Some(CommitRevision::FIRST)
        || transaction.revision() != CommitRevision::FIRST
    {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let stage = limits.stage;
    stage.validate_cache()?;
    if stage.deltas_per_batch == 0
        || stage.deltas_per_batch > MAX_BATCH_DELTAS
        || stage.deltas_per_batch > stage.batch.maximum_deltas
    {
        return Err(StorageError::ResourceLimit.into());
    }
    let counts = metadata_counts(snapshot)?;
    let expected = parse_metadata(
        &metadata_value_from_counts(CommitRevision::FIRST, counts),
        CommitRevision::FIRST,
    )?
    .family_counts()?;
    let mut required = PackedGraphGenesisReport::default();
    for (index, count) in expected.iter().copied().enumerate() {
        required.stage.batches = checked_sum(
            required.stage.batches,
            count.max(1).div_ceil(stage.deltas_per_batch as u64),
        )?;
        let mut actual = 0_u64;
        for entry in family_entries(snapshot, CommitRevision::FIRST, index as u8 + 1)? {
            let entry = entry?;
            actual = checked_sum(actual, 1)?;
            required.entries = checked_sum(required.entries, 1)?;
            required.logical_bytes = checked_sum(
                required.logical_bytes,
                (entry.key.len() + entry.value.len()) as u64,
            )?;
            if required.entries > limits.maximum_entries
                || required.logical_bytes > limits.maximum_logical_bytes
            {
                return Err(StorageError::ResourceLimit.into());
            }
        }
        if actual != count {
            return Err(GraphDiskError::IndexCorrupt);
        }
    }
    if required.stage.batches > stage.maximum_batches {
        return Err(StorageError::ResourceLimit.into());
    }
    let source_digest = GraphState::logical_state_digest(snapshot)
        .map_err(|_| GraphDiskError::RootStateMismatch)?;
    // Authenticating a borrowed genesis receipt cannot authorize another recovered owner.
    let mut maintenance = recovery.packed_indexes_with_io(fs, transaction, stage.certificates)?;
    let mut trees = Vec::with_capacity(8);
    let mut report = PackedGraphGenesisReport::default();
    for (index, count) in expected.iter().copied().enumerate() {
        let family = index as u8 + 1;
        let mut entries = family_entries(snapshot, CommitRevision::FIRST, family)?;
        let mut tree = None;
        for _ in 0..count.max(1).div_ceil(stage.deltas_per_batch as u64) {
            let mut deltas = Vec::new();
            deltas
                .try_reserve_exact(stage.deltas_per_batch)
                .map_err(|_| StorageError::ResourceLimit)?;
            let mut bytes = 0_u64;
            for _ in 0..stage.deltas_per_batch {
                let Some(entry) = entries.next() else {
                    break;
                };
                let entry = entry?;
                bytes = checked_sum(bytes, (entry.key.len() + entry.value.len()) as u64)?;
                if bytes
                    > stage
                        .batch
                        .maximum_input_bytes
                        .min(uste_storage::packed_tree_batch::MAX_BATCH_INPUT_BYTES)
                {
                    return Err(StorageError::ResourceLimit.into());
                }
                deltas.push(IndexDelta::new(entry.key, None, Some(entry.value))?);
            }
            let mut batch = stage.batch;
            batch.maximum_read_pages = batch.maximum_read_pages.min(
                stage
                    .maximum_read_pages
                    .saturating_sub(report.stage.read_pages),
            );
            if !deltas.is_empty() {
                batch.pack.maximum_pages = batch.pack.maximum_pages.min(
                    stage
                        .maximum_written_pages
                        .saturating_sub(report.stage.written_pages),
                );
            }
            let staged = if let Some(bytes) = stage.staging_cache_bytes {
                let (staged, cache) = maintenance.stage_buffered(
                    fs,
                    GRAPH_PACKED_PROFILE_V1,
                    family,
                    tree.as_ref(),
                    &deltas,
                    batch,
                    bytes,
                )?;
                report.stage.record_cache(cache)?;
                staged
            } else {
                maintenance.stage(
                    fs,
                    GRAPH_PACKED_PROFILE_V1,
                    family,
                    tree.as_ref(),
                    &deltas,
                    batch,
                )?
            };
            let work = staged.report();
            report.entries = checked_sum(report.entries, deltas.len() as u64)?;
            report.logical_bytes = checked_sum(report.logical_bytes, bytes)?;
            report.stage.batches += 1;
            report.stage.read_pages = checked_sum(report.stage.read_pages, work.read_pages)?;
            report.stage.written_pages =
                checked_sum(report.stage.written_pages, work.written_pages)?;
            report.stage.peak_batch_deltas = report.stage.peak_batch_deltas.max(deltas.len());
            tree = Some(staged.tree().clone());
        }
        if entries.next().is_some() {
            return Err(GraphDiskError::IndexCorrupt);
        }
        let tree = tree.ok_or(GraphDiskError::IndexCorrupt)?;
        if tree.family_descriptor().commitment.entries() != count {
            return Err(GraphDiskError::IndexCorrupt);
        }
        trees.push(tree);
    }
    if report.entries != required.entries
        || report.logical_bytes != required.logical_bytes
        || report.stage.batches != required.stage.batches
    {
        return Err(GraphDiskError::IndexCorrupt);
    }
    Ok((
        PackedGraphBase {
            scope: snapshot.scope(),
            anchor: maintenance.anchor(),
            reducer: GraphState::REDUCER_PROFILE,
            trees: trees.try_into().map_err(|_| GraphDiskError::IndexCorrupt)?,
            counts,
            policy: snapshot.namespace_policy().cloned(),
            source_v1_digest: Some(source_digest),
        },
        report,
    ))
}
