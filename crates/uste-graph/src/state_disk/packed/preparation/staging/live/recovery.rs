//! Private graph/primary/quota suffix candidates; only a terminal triple becomes live.
use super::*;
use uste_storage::journal::JournalRangeReadReport;
use uste_txn::{
    COORDINATOR_PACKED_PROFILE_V1, COORDINATOR_PACKED_USAGE_PROFILE_V1, CoordinatorRecoveryLimits,
    PackedCoordinatorPrefix, PackedMetadataRebaseLimits, PackedQuotaPrefix, RetentionDays,
    stage_packed_coordinator_prefix, stage_packed_quota_prefix,
};

#[derive(Clone, Copy)]
pub struct PackedGraphSuffixRecoveryLimits {
    pub maximum_revisions: u64,
    /// Per-revision bounds; aggregate work is additionally bounded by admitted revision count.
    pub preparation: PackedGraphPreparationLimits,
    pub deltas: GraphStateDeltaLimits,
    pub graph: PackedGraphStageLimits,
    pub metadata: PackedMetadataRebaseLimits,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackedGraphSuffixRecoveryReport {
    pub journal: JournalRangeReadReport,
    pub graph: PackedGraphStageReport,
    pub proof: PackedGraphReadReport,
    pub metadata_read_pages: u64,
    pub metadata_read_bytes: u64,
    pub metadata_written_pages: u64,
    pub metadata_written_nodes: u64,
    pub maximum_delta_count: u64,
    pub maximum_delta_bytes: u64,
}

/// Raw trusted recovery. On failure no live coordinator escapes and the old roots remain valid.
/// Initial graph and both metadata bases must already be semantically admitted by this owner.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn recover_packed_graph_suffix<F, W, E, I>(
    mut recovery: AuthenticatedIndexRecovery<F, W, E, I>,
    fs: &mut F,
    base: PackedGraphBase,
    graph_root: CertifiedPackedRoot,
    mut primary: PackedCoordinatorPrefix,
    mut quota: PackedQuotaPrefix,
    primary_root: &CertifiedPackedRoot,
    quota_root: &CertifiedPackedRoot,
    retention: RetentionDays,
    overlay: CoordinatorRecoveryLimits,
    limits: PackedGraphSuffixRecoveryLimits,
) -> Result<
    (
        PackedCommitCoordinator<GraphPackedLiveState, F, W, E, I>,
        Option<PackedGraphSuffixRecoveryReport>,
    ),
    GraphDiskError,
>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if !matches_root(&base, &graph_root) {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let state = GraphPackedLiveState {
        base,
        root: graph_root,
        pending: None,
    };
    let terminal = recovery.validate_packed_recovery_base(
        &primary,
        &quota,
        primary_root,
        quota_root,
        &state,
    )?;
    if state.base.anchor == terminal {
        return Ok((
            PackedCommitCoordinator::from_admitted_prefixes(
                recovery,
                primary,
                quota,
                primary_root,
                quota_root,
                state,
                retention,
                overlay,
            )?,
            None,
        ));
    }
    let count = terminal
        .0
        .get()
        .checked_sub(state.base.anchor.0.get())
        .ok_or(GraphDiskError::RootStateMismatch)?;
    if count > limits.maximum_revisions
        || count > limits.metadata.maximum_groups
        || limits.metadata.maximum_publication_attempts == 0
        || limits.metadata.maximum_publication_attempts > 64
    {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    let mut base = state.base;
    let mut cursor = recovery.open_transaction_cursor_with_certificate_window(
        base.anchor
            .0
            .checked_next()
            .map_err(|_| GraphDiskError::RootStateMismatch)?,
        terminal.0,
        limits.maximum_revisions.min(limits.metadata.maximum_groups),
        limits.metadata.maximum_encoded_bytes,
        limits.metadata.certificate_window,
    )?;
    let mut report = PackedGraphSuffixRecoveryReport::default();
    while let Some(transaction) = recovery.next_recovered_transaction(fs, &mut cursor)? {
        let prepared = {
            let maintenance =
                recovery.packed_indexes_with_io(fs, &transaction, limits.graph.certificates)?;
            prepare_packed_graph_transaction(
                &maintenance,
                fs,
                &base,
                crate::decode_transaction(transaction.canonical_request())?,
                limits.preparation,
            )?
        };
        let proof = prepared.2;
        report.proof.point_lookups = checked_sum(report.proof.point_lookups, proof.point_lookups)?;
        report.proof.pages = checked_sum(report.proof.pages, proof.pages)?;
        report.proof.encoded_bytes = checked_sum(report.proof.encoded_bytes, proof.encoded_bytes)?;
        report.proof.scan_candidates =
            checked_sum(report.proof.scan_candidates, proof.scan_candidates)?;
        let plan = prepare_packed_graph_delta(prepared.0, limits.deltas)?;
        report.maximum_delta_count = report.maximum_delta_count.max(plan.delta_count());
        report.maximum_delta_bytes = report.maximum_delta_bytes.max(plan.logical_bytes());
        // Exact canonical request/inventory/result checks precede all private graph output.
        let (next_base, graph) =
            stage_packed_graph_delta(&mut recovery, fs, &base, &transaction, &plan, limits.graph)?;
        let (next_primary, p) = stage_packed_coordinator_prefix(
            &mut recovery,
            fs,
            Some(&primary),
            &transaction,
            limits.metadata.staging,
        )?;
        let (next_quota, q) = stage_packed_quota_prefix(
            &mut recovery,
            fs,
            Some(&quota),
            &next_primary,
            &transaction,
            limits.metadata.staging,
        )?;
        report.graph.batches = checked_sum(report.graph.batches, graph.batches)?;
        report.graph.read_pages = checked_sum(report.graph.read_pages, graph.read_pages)?;
        report.graph.written_pages = checked_sum(report.graph.written_pages, graph.written_pages)?;
        report.graph.peak_batch_deltas =
            report.graph.peak_batch_deltas.max(graph.peak_batch_deltas);
        for (pages, bytes) in [
            (p.owner_lookup_pages, p.owner_lookup_bytes),
            (q.primary_lookup_pages, q.primary_lookup_bytes),
        ] {
            report.metadata_read_pages = checked_sum(report.metadata_read_pages, pages)?;
            report.metadata_read_bytes = checked_sum(report.metadata_read_bytes, bytes)?;
        }
        if let Some(work) = q.principal_lookup {
            report.metadata_read_pages = checked_sum(report.metadata_read_pages, work.pages)?;
            report.metadata_read_bytes =
                checked_sum(report.metadata_read_bytes, work.encoded_bytes)?;
        }
        for batch in p.batches.into_iter().chain(q.batches) {
            report.metadata_read_pages = checked_sum(report.metadata_read_pages, batch.read_pages)?;
            report.metadata_read_bytes = checked_sum(report.metadata_read_bytes, batch.read_bytes)?;
            report.metadata_written_pages =
                checked_sum(report.metadata_written_pages, batch.written_pages)?;
            report.metadata_written_nodes =
                checked_sum(report.metadata_written_nodes, batch.written_nodes)?;
        }
        base = next_base;
        primary = next_primary;
        quota = next_quota;
    }
    report.journal = recovery.finish_transaction_cursor(cursor)?;
    if report.journal.groups != count
        || base.anchor != terminal
        || primary.anchor() != terminal
        || quota.anchor() != terminal
    {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let claims = base.publication_claims();
    let graph_root = recovery.publish_recovered_packed_root(
        fs,
        GRAPH_PACKED_PROFILE_V1,
        claims,
        &base.families(),
        limits.metadata.maximum_publication_attempts,
    )?;
    let primary_root = recovery.publish_recovered_packed_root(
        fs,
        COORDINATOR_PACKED_PROFILE_V1,
        claims,
        &primary.families(),
        limits.metadata.maximum_publication_attempts,
    )?;
    let quota_root = recovery.publish_recovered_packed_root(
        fs,
        COORDINATOR_PACKED_USAGE_PROFILE_V1,
        claims,
        &quota.families(),
        limits.metadata.maximum_publication_attempts,
    )?;
    let state = GraphPackedLiveState {
        base,
        root: graph_root,
        pending: None,
    };
    let coordinator = PackedCommitCoordinator::from_admitted_prefixes(
        recovery,
        primary,
        quota,
        &primary_root,
        &quota_root,
        state,
        retention,
        overlay,
    )?;
    Ok((coordinator, Some(report)))
}
