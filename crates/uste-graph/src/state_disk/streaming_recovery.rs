use super::*;
use uste_txn::{
    CoordinatorDiskBase, DiskCommitCoordinator, DiskCoordinatorRecoveryLimits, DiskRecoveryDomain,
    RecoveryIndexMaintenance, RetentionDays,
};

/// Total suffix-count admission plus explicit per-revision proof/delta/merge bounds.
/// Maximum total domain work is the admitted revision count times the per-revision bounds;
/// no history-sized state or set of intermediate root handles is retained.
#[derive(Clone, Copy, Debug)]
pub struct GraphDiskSuffixRecoveryLimits {
    pub maximum_revisions: u64,
    pub preparation: GraphDiskPreparationLimits,
    pub deltas: GraphStateDeltaLimits,
    pub merge: GraphStateRootMergeLimits,
}

/// Terminal-only trusted diagnostics, not consumer-authorized cardinality information.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphDiskSuffixRecoveryReport {
    pub revisions: u64,
    pub staged_runs: u64,
    pub base_entries: u64,
    pub output_entries: u64,
    pub output_logical_bytes: u64,
    pub pages_read: u64,
}

/// Recover a private disk-backed graph through every certified suffix revision, then publish
/// only its terminal root. Metadata retry/collision/first-owner checks remain coordinator-owned.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn recover_graph_disk_suffix<F, W, E, I>(
    recovery: AuthenticatedIndexRecovery<F, W, E, I>,
    filesystem: &mut F,
    metadata: CoordinatorDiskBase,
    state: GraphDiskLiveState,
    retention: RetentionDays,
    coordinator_limits: DiskCoordinatorRecoveryLimits,
    limits: GraphDiskSuffixRecoveryLimits,
    cache: &mut PageCache,
) -> Result<
    (
        DiskCommitCoordinator<GraphDiskLiveState, F, W, E, I>,
        GraphDiskSuffixRecoveryReport,
    ),
    GraphDiskError,
>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let mut domain = GraphRecoveryDomain {
        limits,
        admitted: 0,
        report: GraphDiskSuffixRecoveryReport::default(),
    };
    let coordinator = DiskCommitCoordinator::recover_with_streaming_domain(
        recovery,
        filesystem,
        metadata,
        state,
        retention,
        coordinator_limits,
        cache,
        &mut domain,
    )?;
    Ok((coordinator, domain.report))
}

struct GraphRecoveryDomain {
    limits: GraphDiskSuffixRecoveryLimits,
    admitted: u64,
    report: GraphDiskSuffixRecoveryReport,
}

impl<F, W, E, I> DiskRecoveryDomain<GraphDiskLiveState, F, W, E, I> for GraphRecoveryDomain
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn admit(&mut self, revisions: u64) -> Result<(), StorageError> {
        if revisions > self.limits.maximum_revisions {
            return Err(StorageError::ResourceLimit);
        }
        self.admitted = revisions;
        Ok(())
    }

    fn prepare(
        &mut self,
        recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        state: &GraphDiskLiveState,
        transaction: &RecoveredFrontierTransaction,
        cache: &mut PageCache,
    ) -> Result<GraphDiskCommit, StorageError> {
        let base = state.current_base().ok_or(StorageError::IntegrityFailure)?;
        let proof = load_graph_disk_recovery_preparation_view(
            recovery,
            filesystem,
            base,
            transaction,
            self.limits.preparation,
            cache,
        )
        .map_err(recovery_error)?
        .prepare()
        .map_err(recovery_error)?;
        prepare_graph_disk_commit(proof, self.limits.deltas).map_err(recovery_error)
    }

    fn advance(
        &mut self,
        recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        state: &mut GraphDiskLiveState,
        transaction: &RecoveredFrontierTransaction,
        _cache: &mut PageCache,
    ) -> Result<(), StorageError> {
        let (published, counts, policy, report) = {
            let (base, pending) = state
                .pending_publication(transaction.outcome())
                .map_err(recovery_error)?;
            let mut maintenance = recovery
                .stage_indexes(transaction)
                .map_err(transaction_error)?;
            let (published, report) = publish_graph_state_root_delta_with(
                &mut maintenance,
                filesystem,
                base.admitted_root(),
                &pending.plan,
                transaction.outcome(),
                self.limits.merge,
            )
            .map_err(recovery_error)?;
            (
                published,
                pending.plan.target_counts,
                pending.target_policy.clone(),
                report,
            )
        };
        self.report.revisions = add(self.report.revisions, 1)?;
        self.report.staged_runs = add(self.report.staged_runs, report.runs)?;
        self.report.base_entries = add(self.report.base_entries, report.base_entries)?;
        self.report.output_entries = add(self.report.output_entries, report.output_entries)?;
        self.report.output_logical_bytes = add(
            self.report.output_logical_bytes,
            report.output_logical_bytes,
        )?;
        self.report.pages_read = add(self.report.pages_read, report.pages_read)?;
        state
            .install_publication(
                recovery.scope(),
                Some((transaction.revision(), *transaction.certificate_digest())),
                GraphDiskLivePublication {
                    base: GraphDiskBase {
                        root: published,
                        counts,
                        current_policy: policy,
                    },
                },
            )
            .map_err(apply_error)
    }

    fn finish(
        &mut self,
        recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        state: &mut GraphDiskLiveState,
        frontier: (CommitRevision, [u8; 32]),
        _cache: &mut PageCache,
    ) -> Result<(), StorageError> {
        if self.report.revisions != self.admitted || state.is_pending() {
            return Err(StorageError::IntegrityFailure);
        }
        let root = &state.base.root.root;
        if (root.revision(), *root.certificate_digest()) != frontier {
            return Err(StorageError::IntegrityFailure);
        }
        if self.admitted == 0 {
            if root.generation() == 0 {
                return Err(StorageError::IntegrityFailure);
            }
            return recovery
                .resynchronize_recovered_index_root(
                    filesystem,
                    root,
                    self.limits
                        .merge
                        .fallback_read_limits()
                        .map_err(recovery_error)?,
                )
                .map_err(transaction_error);
        }
        let input = IndexRootInput {
            scope: root.scope(),
            revision: root.revision(),
            certificate_digest: *root.certificate_digest(),
            reducer_profile: *root.reducer_profile(),
            logical_state_digest: *root.logical_state_digest(),
            index_profile: GRAPH_STATE_PROFILE_V1,
        };
        let runs = root.runs().copied().collect::<Vec<_>>();
        let published = recovery
            .publish_recovered_index_root(
                filesystem,
                input,
                &runs,
                self.limits
                    .merge
                    .fallback_read_limits()
                    .map_err(recovery_error)?,
            )
            .map_err(transaction_error)?;
        state.base.root.root = published;
        Ok(())
    }
}

impl<F, W, E, I> GraphStateIndexPublisher<F> for RecoveryIndexMaintenance<'_, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn graph_checkpoint_anchor(
        &self,
    ) -> Result<Option<(CommitRevision, [u8; 32])>, TransactionError> {
        self.anchor().map(Some)
    }

    fn graph_merge_index_run_visit<T>(
        &mut self,
        filesystem: &mut F,
        revision: CommitRevision,
        family: u8,
        base_root: Option<&RecoveredIndexRoot>,
        limits: IndexRunMergeLimits,
        deltas: T,
        visitor: &mut IndexRunVisitor<'_>,
    ) -> Result<uste_storage::MergedIndexRun, TransactionError>
    where
        T: IntoIterator<Item = Result<IndexDelta, StorageError>>,
    {
        self.merge_index_run_visit(
            filesystem,
            revision,
            GRAPH_STATE_PROFILE_V1,
            family,
            base_root,
            limits,
            deltas,
            visitor,
        )
    }

    fn graph_publish_index_root_recovered(
        &mut self,
        _filesystem: &mut F,
        input: IndexRootInput,
        runs: &[IndexRunDescriptor],
        _fallback_limits: IndexRunReadLimits,
    ) -> Result<RecoveredIndexRoot, TransactionError> {
        self.finish(input, runs)
            .map(|staged| staged.read_root().clone())
    }
}

fn add(left: u64, right: u64) -> Result<u64, StorageError> {
    left.checked_add(right).ok_or(StorageError::ResourceLimit)
}

fn apply_error(error: ApplyError) -> StorageError {
    match error {
        ApplyError::ResourceLimit => StorageError::ResourceLimit,
        _ => StorageError::IntegrityFailure,
    }
}

fn transaction_error(error: TransactionError) -> StorageError {
    match error {
        TransactionError::Storage(error) => error,
        TransactionError::ResourceLimit => StorageError::ResourceLimit,
        _ => StorageError::IntegrityFailure,
    }
}

fn recovery_error(error: GraphDiskError) -> StorageError {
    match error {
        GraphDiskError::Storage(error) => error,
        GraphDiskError::Transaction(error) => transaction_error(error),
        GraphDiskError::Graph(error) => apply_error(error.into_apply_error()),
        GraphDiskError::Codec(GraphCodecError::ResourceLimit) => StorageError::ResourceLimit,
        _ => StorageError::IntegrityFailure,
    }
}
