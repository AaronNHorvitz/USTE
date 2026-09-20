//! Private graph/primary/quota suffix candidates; only a terminal triple becomes live.
use super::*;
mod origin;
pub use origin::{
    PackedGraphOriginRecoveryLimits, PackedGraphOriginRecoveryReport, recover_packed_graph_origin,
};
mod rebuild;
use rebuild::{Candidates, advance, publish_terminal};
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
    primary: PackedCoordinatorPrefix,
    quota: PackedQuotaPrefix,
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
    let (candidates, report) = advance(
        &mut recovery,
        fs,
        Candidates {
            base: state.base,
            primary,
            quota,
        },
        terminal,
        limits,
    )?;
    let coordinator = publish_terminal(
        recovery,
        fs,
        candidates,
        retention,
        overlay,
        limits.metadata.maximum_publication_attempts,
    )?;
    Ok((coordinator, Some(report)))
}
