//! Explicit journal-origin reconstruction; ordinary opens never select this path implicitly.
use super::*;
use crate::{PackedGraphGenesisLimits, PackedGraphGenesisReport, stage_packed_graph_genesis};
use uste_txn::{PackedCoordinatorReport, PackedQuotaReport};

#[derive(Clone, Copy)]
pub struct PackedGraphOriginRecoveryLimits {
    /// First-transaction encrypted range/proof allowance, separate from the suffix allowance.
    pub maximum_genesis_encoded_bytes: u64,
    pub genesis: PackedGraphGenesisLimits,
    pub suffix: PackedGraphSuffixRecoveryLimits,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackedGraphOriginRecoveryReport {
    /// Staging work only; first-transaction source authentication is not included here.
    pub genesis: PackedGraphGenesisReport,
    pub primary: PackedCoordinatorReport,
    pub quota: PackedQuotaReport,
    pub suffix: PackedGraphSuffixRecoveryReport,
}

/// Rebuild derived packed state from the retained authoritative journal, never roll it back.
/// No live state escapes a failed reconstruction or partial terminal publication.
/// The caller selects storage recovery mode: this method does not remove certificate/blob maps
/// retained by a memory-resident opener. It reconstructs graph/coordinator state without such
/// history-wide maps of its own. Graph-only requests must not contain a blob inventory.
#[allow(clippy::type_complexity)]
pub fn recover_packed_graph_origin<F, W, E, I>(
    mut recovery: AuthenticatedIndexRecovery<F, W, E, I>,
    fs: &mut F,
    retention: RetentionDays,
    overlay: CoordinatorRecoveryLimits,
    limits: PackedGraphOriginRecoveryLimits,
) -> Result<
    (
        PackedCommitCoordinator<GraphPackedLiveState, F, W, E, I>,
        PackedGraphOriginRecoveryReport,
    ),
    GraphDiskError,
>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let terminal = recovery
        .authenticated_frontier_anchor()
        .ok_or(GraphDiskError::RootStateMismatch)?;
    let count = terminal.0.get() - 1;
    let suffix = limits.suffix;
    if count > suffix.maximum_revisions
        || count > suffix.metadata.maximum_groups
        || suffix.metadata.maximum_publication_attempts == 0
        || suffix.metadata.maximum_publication_attempts > 64
    {
        return Err(StorageError::ResourceLimit.into());
    }
    let genesis = recovery.recover_inventory_free_genesis(
        fs,
        GraphState::new(recovery.scope()),
        limits.maximum_genesis_encoded_bytes,
    )?;
    let (base, graph) = stage_packed_graph_genesis(&mut recovery, fs, &genesis, limits.genesis)?;
    let (primary, p) = stage_packed_coordinator_prefix(
        &mut recovery,
        fs,
        None,
        genesis.transaction(),
        suffix.metadata.staging,
    )?;
    let (quota, q) = stage_packed_quota_prefix(
        &mut recovery,
        fs,
        None,
        &primary,
        genesis.transaction(),
        suffix.metadata.staging,
    )?;
    // No complete reducer or first-transaction payload is kept while streaming later history.
    drop(genesis);
    let (candidates, suffix_report) = advance(
        &mut recovery,
        fs,
        Candidates {
            base,
            primary,
            quota,
        },
        terminal,
        suffix,
    )?;
    let coordinator = publish_terminal(
        recovery,
        fs,
        candidates,
        retention,
        overlay,
        suffix.metadata.maximum_publication_attempts,
    )?;
    Ok((
        coordinator,
        PackedGraphOriginRecoveryReport {
            genesis: graph,
            primary: p,
            quota: q,
            suffix: suffix_report,
        },
    ))
}
