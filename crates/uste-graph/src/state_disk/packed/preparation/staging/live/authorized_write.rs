use super::*;
use uste_policy::AuthorizationRequirements;
use uste_txn::{AuthorizedPackedWriteState, AuthorizedTransactionState};

#[derive(Clone, Copy)]
pub struct PackedGraphWritePreparationLimits {
    /// Fresh per-preparation cache; `None` preserves uncached proof reads.
    pub proof_cache_bytes: Option<usize>,
    pub proof: PackedGraphPreparationLimits,
    pub delta: GraphStateDeltaLimits,
    pub certificates: CertificateAnchorReadLimits,
}
#[derive(Clone, Copy)]
pub struct PackedGraphWritePublicationLimits {
    pub stage: PackedGraphStageLimits,
    pub maximum_attempts: u8,
}
impl<F, W, E, I> AuthorizedPackedWriteState<F, W, E, I> for GraphPackedLiveState
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    type PrepareLimits = PackedGraphWritePreparationLimits;
    type PublishLimits = PackedGraphWritePublicationLimits;
    fn committed_write_policy(&self) -> Result<&NamespacePolicy, ApplyError> {
        self.pending
            .as_ref()
            .and_then(|plan| plan.prepared.prepared.policy_change.as_ref())
            .or(self.base.policy.as_ref())
            .ok_or(ApplyError::InvalidRequest)
    }
    fn write_requirements(request: &[u8]) -> Result<AuthorizationRequirements, ApplyError> {
        <GraphState as AuthorizedTransactionState>::authorization_requirements(request, None)
    }
    fn prepare_packed_write(
        coordinator: &mut PackedCommitCoordinator<Self, F, W, E, I>,
        fs: &mut F,
        request: &[u8],
        limits: &Self::PrepareLimits,
    ) -> Result<Self::Prepared, TransactionError> {
        (|| {
            let mut borrow = coordinator.reducer_and_index_maintenance()?;
            let base = borrow
                .reducer
                .current_base()
                .ok_or(GraphDiskError::RootStateMismatch)?;
            let maintenance = borrow.indexes.packed_indexes(fs, limits.certificates)?;
            let transaction = crate::decode_transaction(request)?;
            let prepared = match limits.proof_cache_bytes {
                Some(bytes) => {
                    prepare_packed_graph_transaction_buffered(
                        &maintenance,
                        fs,
                        base,
                        transaction,
                        limits.proof,
                        bytes,
                    )?
                    .0
                }
                None => {
                    prepare_packed_graph_transaction(
                        &maintenance,
                        fs,
                        base,
                        transaction,
                        limits.proof,
                    )?
                    .0
                }
            };
            prepare_packed_graph_delta(prepared, limits.delta)
        })()
        .map_err(crate::state_disk::authorized_write::content_free_error)
    }
    fn publish_packed_write(
        coordinator: &mut PackedCommitCoordinator<Self, F, W, E, I>,
        fs: &mut F,
        outcome: TransactionOutcome,
        limits: &Self::PublishLimits,
    ) -> Result<(), TransactionError> {
        let state = coordinator.state()?;
        if !state.needs_repair() || state.revision() != outcome.revision {
            return Ok(());
        }
        publish_packed_graph_live_base(
            coordinator,
            fs,
            outcome,
            limits.stage,
            limits.maximum_attempts,
        )
        .map_err(crate::state_disk::authorized_write::content_free_error)?;
        Ok(())
    }
}
