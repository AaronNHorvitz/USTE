//! Streaming overlay publication with retry-safe reuse of a partially published root pair.

use super::*;
use uste_storage::{IndexDelta, IndexRunDescriptor, IndexRunMergeLimits};

/// Per-family merge and root verification limits (reuse and fallback-slot selection). Three data families
/// and one fixed metadata entry are processed; these bounds are not aggregate across families.
#[derive(Clone, Copy, Debug)]
pub struct CoordinatorMetadataRebaseLimits {
    pub merge: IndexRunMergeLimits,
    pub reuse: IndexRunReadLimits,
}

pub(crate) fn publish_overlay_base<S, F, W, E, I>(
    coordinator: &mut CommitCoordinator<S, F, W, E, I>,
    filesystem: &mut F,
    base: &CoordinatorDiskBase,
    limits: CoordinatorMetadataRebaseLimits,
) -> Result<CoordinatorDiskBase, TransactionError>
where
    S: crate::DiskCoordinatorState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let anchor = coordinator
        .checkpoint_anchor()?
        .ok_or(TransactionError::IntegrityFailure)?;
    let input = coordinator
        .state
        .metadata_publication_input(anchor)
        .map_err(crate::map_apply_error)?;
    for profile in [
        COORDINATOR_METADATA_PROFILE_V1,
        COORDINATOR_TRANSACTION_PROFILE_V1,
    ] {
        if coordinator
            .load_index_root_manifests(filesystem, profile)?
            .iter()
            .any(|root| root.revision() > base.metadata.revision() && root.revision() != anchor.0)
        {
            // Do not rotate away the pinned older pair when a different writer path advanced
            // beyond an incomplete rebase. Explicit cache rebuild is required in this case.
            return Err(TransactionError::ResourceLimit);
        }
    }
    if input.scope != coordinator.scope
        || (input.revision, input.certificate_digest) != anchor
        || input.index_profile != COORDINATOR_METADATA_PROFILE_V1
        || input.reducer_profile != *base.metadata.reducer_profile()
        || coordinator.outcomes.len() != coordinator.transactions.len()
        || base
            .metadata
            .revision()
            .get()
            .checked_add(coordinator.outcomes.len() as u64)
            != Some(anchor.0.get())
        || coordinator.outcomes.iter().any(|(key, outcome)| {
            outcome.revision <= base.metadata.revision()
                || outcome.revision > anchor.0
                || coordinator.transactions.get(&outcome.transaction_id)
                    != Some(&(key.principal, *outcome))
        })
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let owner_count = base
        .metadata
        .runs()
        .find(|run| run.family() == FAMILY_BLOB_OWNER)
        .map_or(0, |run| run.entry_count())
        .checked_add(coordinator.committed_blob_owners.len() as u64)
        .ok_or(TransactionError::ResourceLimit)?;
    if owner_count > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64 {
        return Err(TransactionError::ResourceLimit);
    }
    let retry_deltas = coordinator.outcomes.iter().map(|(key, outcome)| {
        let entry = encode_outcome(key.principal, key.key, *outcome)?;
        IndexDelta::new(entry.key, None, Some(entry.value))
    });
    let retry = coordinator
        .journal
        .merge_index_run(
            filesystem,
            coordinator.scope,
            anchor.0,
            COORDINATOR_METADATA_PROFILE_V1,
            FAMILY_OUTCOME,
            Some(&base.metadata),
            limits.merge,
            retry_deltas,
        )
        .map_err(TransactionError::Storage)?;
    let retry = exact_merged_run(retry, anchor.0.get(), coordinator.outcomes.len())?;
    let transaction_deltas = coordinator
        .transactions
        .iter()
        .map(|(id, (principal, outcome))| {
            let entry = encode_outcome(*principal, IdempotencyKey::from_bytes([0; 16]), *outcome)?;
            let mut value = Vec::with_capacity(136);
            value.extend_from_slice(&principal.as_bytes());
            value.extend_from_slice(&entry.value);
            IndexDelta::new(id.as_bytes().to_vec(), None, Some(value))
        });
    let transaction = coordinator
        .journal
        .merge_index_run(
            filesystem,
            coordinator.scope,
            anchor.0,
            COORDINATOR_TRANSACTION_PROFILE_V1,
            1,
            Some(&base.transactions.root),
            limits.merge,
            transaction_deltas,
        )
        .map_err(TransactionError::Storage)?;
    let transaction =
        exact_merged_run(transaction, anchor.0.get(), coordinator.transactions.len())?;
    let mut runs = Vec::with_capacity(3);
    let metadata = coordinator
        .journal
        .publish_index_run(
            filesystem,
            coordinator.scope,
            anchor.0,
            COORDINATOR_METADATA_PROFILE_V1,
            FAMILY_METADATA,
            [IndexEntry {
                key: METADATA_KEY.to_vec(),
                value: metadata_value(anchor.0, anchor.0.get(), owner_count),
            }],
        )
        .map_err(TransactionError::Storage)?;
    runs.push(metadata);
    runs.push(retry);
    if owner_count != 0 {
        let deltas = coordinator
            .committed_blob_owners
            .values()
            .map(|(reference, principal)| {
                if reference.scope() != coordinator.scope {
                    return Err(StorageError::IntegrityFailure);
                }
                let entry = encode_owner(*reference, *principal)?;
                IndexDelta::new(entry.key, None, Some(entry.value))
            });
        let owner_base = base
            .metadata
            .runs()
            .any(|run| run.family() == FAMILY_BLOB_OWNER)
            .then_some(&base.metadata);
        let owner = coordinator
            .journal
            .merge_index_run(
                filesystem,
                coordinator.scope,
                anchor.0,
                COORDINATOR_METADATA_PROFILE_V1,
                FAMILY_BLOB_OWNER,
                owner_base,
                limits.merge,
                deltas,
            )
            .map_err(TransactionError::Storage)?;
        runs.push(exact_merged_run(
            owner,
            owner_count,
            coordinator.committed_blob_owners.len(),
        )?);
    }
    let metadata = publish_or_reuse(coordinator, filesystem, input, &runs, limits.reuse)?;
    let transactions = publish_or_reuse(
        coordinator,
        filesystem,
        IndexRootInput {
            index_profile: COORDINATOR_TRANSACTION_PROFILE_V1,
            ..input
        },
        &[transaction],
        limits.reuse,
    )?;
    Ok(CoordinatorDiskBase {
        metadata,
        transactions: CoordinatorTransactionIndex { root: transactions },
    })
}

fn exact_merged_run(
    merged: uste_storage::MergedIndexRun,
    entries: u64,
    insertions: usize,
) -> Result<IndexRunDescriptor, TransactionError> {
    if merged.report.output_entries != entries
        || merged.report.insertions != insertions as u64
        || merged.report.replacements != 0
        || merged.report.deletions != 0
    {
        return Err(TransactionError::IntegrityFailure);
    }
    merged.run.ok_or(TransactionError::IntegrityFailure)
}

fn publish_or_reuse<S, F, W, E, I>(
    coordinator: &mut CommitCoordinator<S, F, W, E, I>,
    filesystem: &mut F,
    input: IndexRootInput,
    runs: &[IndexRunDescriptor],
    limits: IndexRunReadLimits,
) -> Result<RecoveredIndexRoot, TransactionError>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    for root in coordinator.load_index_root_manifests(filesystem, input.index_profile)? {
        if root.revision() != input.revision {
            continue;
        }
        // Repeated partial-pair publication must not rotate away the old matching root. Reuse
        // only a fully authenticated current candidate matching freshly merged logical content.
        if root.scope() != input.scope
            || root.certificate_digest() != &input.certificate_digest
            || root.reducer_profile() != &input.reducer_profile
            || root.logical_state_digest() != &input.logical_state_digest
            || root.runs().len() != runs.len()
            || root.runs().zip(runs).any(|(old, new)| {
                old.family() != new.family()
                    || old.entry_count() != new.entry_count()
                    || old.logical_digest() != new.logical_digest()
            })
        {
            return Err(TransactionError::IntegrityFailure);
        }
        coordinator
            .journal
            .resync_index_root_bounded(filesystem, &root, limits)
            .map_err(TransactionError::Storage)?;
        return Ok(root);
    }
    coordinator
        .journal
        .publish_index_root_recovered_bounded(filesystem, input, runs, limits)
        .map_err(TransactionError::Storage)
}
