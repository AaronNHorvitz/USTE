//! Private first-revision roots for bounded origin reconstruction.
use super::*;
use crate::RecoveredGenesis;
use uste_storage::{IndexDelta, IndexRunMergeLimits};

/// Stage retry and transaction-ID roots for a reconstructed inventory-free first revision.
/// Nothing is published to discoverable root slots. Both candidates must undergo the normal
/// independent journal admission before use, and only terminal recovery may publish roots.
pub fn stage_inventory_free_genesis_metadata<S, F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    filesystem: &mut F,
    genesis: &RecoveredGenesis<S>,
    limits: IndexRunMergeLimits,
) -> Result<(CoordinatorMetadataCandidate, RecoveredIndexRoot), TransactionError>
where
    S: CheckpointState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let transaction = genesis.transaction();
    let state = genesis.state();
    if transaction.scope != recovery.scope()
        || transaction.revision != CommitRevision::FIRST
        || transaction.blob_inventory.is_some()
        || state.current_checkpoint_scope() != recovery.scope()
        || state.current_checkpoint_revision() != Some(CommitRevision::FIRST)
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let input = IndexRootInput {
        scope: recovery.scope(),
        revision: CommitRevision::FIRST,
        certificate_digest: *transaction.certificate_digest(),
        reducer_profile: S::REDUCER_PROFILE,
        logical_state_digest: state
            .current_logical_state_digest()
            .map_err(checkpoint_error)?,
        index_profile: COORDINATOR_METADATA_PROFILE_V1,
    };
    let retry = encode_outcome(
        transaction.principal,
        transaction.idempotency_key,
        transaction.outcome,
    )
    .map_err(TransactionError::Storage)?;
    let mut transaction_value = Vec::with_capacity(136);
    transaction_value.extend_from_slice(&transaction.principal.as_bytes());
    transaction_value.extend_from_slice(&retry.value);
    let mut stage = recovery.stage_indexes_with_io(filesystem, transaction)?;
    let mut runs = Vec::with_capacity(2);
    for (family, entry) in [
        (
            FAMILY_METADATA,
            IndexEntry {
                key: METADATA_KEY.to_vec(),
                value: metadata_value(CommitRevision::FIRST, 1, 0),
            },
        ),
        (FAMILY_OUTCOME, retry),
    ] {
        let merged = stage.merge_index_run_visit(
            filesystem,
            CommitRevision::FIRST,
            COORDINATOR_METADATA_PROFILE_V1,
            family,
            None,
            limits,
            [IndexDelta::new(entry.key, None, Some(entry.value))],
            &mut |_, _| Ok(()),
        )?;
        runs.push(merged.run.ok_or(TransactionError::IntegrityFailure)?);
    }
    let metadata = stage.finish(input, &runs)?.read_root().clone();
    let mut stage = recovery.stage_indexes_with_io(filesystem, transaction)?;
    let merged = stage.merge_index_run_visit(
        filesystem,
        CommitRevision::FIRST,
        COORDINATOR_TRANSACTION_PROFILE_V1,
        1,
        None,
        limits,
        [IndexDelta::new(
            transaction.outcome.transaction_id.as_bytes().to_vec(),
            None,
            Some(transaction_value),
        )],
        &mut |_, _| Ok(()),
    )?;
    let transaction_root = stage
        .finish(
            IndexRootInput {
                index_profile: COORDINATOR_TRANSACTION_PROFILE_V1,
                ..input
            },
            &[merged.run.ok_or(TransactionError::IntegrityFailure)?],
        )?
        .read_root()
        .clone();
    Ok((
        CoordinatorMetadataCandidate { root: metadata },
        transaction_root,
    ))
}
