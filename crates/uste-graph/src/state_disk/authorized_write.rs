use super::*;
use uste_policy::AuthorizationRequirements;
use uste_txn::{AuthorizedDiskWriteState, AuthorizedTransactionState, DiskCommitCoordinator};

#[derive(Clone, Copy, Debug)]
pub struct GraphDiskWritePreparationLimits {
    pub proof: GraphDiskPreparationLimits,
    pub delta: GraphStateDeltaLimits,
}

impl<F, W, E, I> AuthorizedDiskWriteState<F, W, E, I> for GraphDiskLiveState
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    type PrepareLimits = GraphDiskWritePreparationLimits;
    type PublishLimits = GraphStateRootMergeLimits;
    type WriteError = TransactionError;

    fn committed_write_policy(&self) -> Result<&NamespacePolicy, ApplyError> {
        match &self.pending {
            Some(pending) => pending.target_policy.as_ref(),
            None => self.base.namespace_policy(),
        }
        .ok_or(ApplyError::InvalidRequest)
    }

    fn write_requirements(request: &[u8]) -> Result<AuthorizationRequirements, ApplyError> {
        <GraphState as AuthorizedTransactionState>::authorization_requirements(request, None)
    }

    fn prepare_disk_write(
        coordinator: &DiskCommitCoordinator<Self, F, W, E, I>,
        filesystem: &mut F,
        request: &[u8],
        limits: &GraphDiskWritePreparationLimits,
        cache: &mut PageCache,
    ) -> Result<GraphDiskCommit, TransactionError> {
        (|| {
            let transaction = crate::decode_transaction(request)?;
            let proof = load_graph_disk_coordinator_preparation_view(
                coordinator,
                filesystem,
                transaction,
                limits.proof,
                cache,
            )?
            .prepare()?;
            prepare_graph_disk_commit(proof, limits.delta)
        })()
        .map_err(content_free_error)
    }

    fn publish_disk_write(
        coordinator: &mut DiskCommitCoordinator<Self, F, W, E, I>,
        filesystem: &mut F,
        outcome: TransactionOutcome,
        limits: &GraphStateRootMergeLimits,
    ) -> Result<(), TransactionError> {
        let state = coordinator.state()?;
        // An older exact retry must not pretend to acknowledge or repair a different pending write.
        if !state.is_pending() || state.revision() != outcome.revision {
            return Ok(());
        }
        publish_graph_disk_coordinator_base(coordinator, filesystem, outcome, *limits)
            .map_err(content_free_error)?;
        Ok(())
    }
}

// Graph validation errors can contain hidden dependency counts or record identities. Match the
// existing authorized reducer's content-free error boundary, never export raw proof errors.
fn content_free_error(error: GraphDiskError) -> TransactionError {
    match error {
        GraphDiskError::Storage(error) => TransactionError::Storage(error),
        GraphDiskError::Transaction(error) => error,
        GraphDiskError::Graph(error) => match error.into_apply_error() {
            ApplyError::Conflict => TransactionError::Conflict,
            ApplyError::SourceChanged => TransactionError::SourceChanged,
            ApplyError::InvalidRequest => TransactionError::InvalidRequest,
            ApplyError::ResourceLimit => TransactionError::ResourceLimit,
            ApplyError::UnsupportedPredicate => TransactionError::UnsupportedPredicate,
        },
        GraphDiskError::Codec(GraphCodecError::ResourceLimit) => TransactionError::ResourceLimit,
        GraphDiskError::Codec(_) | GraphDiskError::SnapshotHasNoRevision => {
            TransactionError::InvalidRequest
        }
        GraphDiskError::RootStateMismatch => TransactionError::Conflict,
        GraphDiskError::IndexCorrupt => TransactionError::IntegrityFailure,
        GraphDiskError::UnsupportedRequest => TransactionError::UnsupportedPredicate,
    }
}
