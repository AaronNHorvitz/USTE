//! One private primary-metadata advancement; no cumulative recovery maps.
use super::*;
use crate::{InventoryFreeMetadataRecoveryReport, RecoveredFrontierTransaction};
use uste_storage::{IndexDelta, IndexRunMergeLimits};

#[allow(clippy::too_many_arguments)]
pub(crate) fn stage_primary_metadata_step<F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    filesystem: &mut F,
    base: &CoordinatorDiskBase,
    transaction: &RecoveredFrontierTransaction,
    input: IndexRootInput,
    limits: IndexRunMergeLimits,
    owners: &BTreeMap<BlobId, (BlobReference, PrincipalDigest)>,
    preserve_first_references: bool,
) -> Result<(CoordinatorDiskBase, InventoryFreeMetadataRecoveryReport), TransactionError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if (!preserve_first_references && base.first_references.is_some())
        || (preserve_first_references && base.owner_count() != 0 && base.first_references.is_none())
        || base.usage.is_some()
        || transaction.scope != recovery.scope()
        || input.scope != recovery.scope()
        || input.revision != transaction.revision
        || input.certificate_digest != transaction.certificate_digest
        || input.reducer_profile != *base.metadata.reducer_profile()
        || input.index_profile != COORDINATOR_METADATA_PROFILE_V1
        || base.metadata.revision().checked_next().ok() != Some(transaction.revision)
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let owner_count = base
        .owner_count()
        .checked_add(owners.len() as u64)
        .ok_or(TransactionError::ResourceLimit)?;
    if owner_count > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64 {
        return Err(TransactionError::ResourceLimit);
    }
    let mut report = InventoryFreeMetadataRecoveryReport::default();
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
    let mut runs = Vec::with_capacity(3);
    for (family, entry, source, count) in [
        (
            FAMILY_METADATA,
            IndexEntry {
                key: METADATA_KEY.to_vec(),
                value: metadata_value(input.revision, input.revision.get(), owner_count),
            },
            None,
            1,
        ),
        (
            FAMILY_OUTCOME,
            retry,
            Some(&base.metadata),
            input.revision.get(),
        ),
    ] {
        let merged = stage.merge_index_run_visit(
            filesystem,
            input.revision,
            COORDINATOR_METADATA_PROFILE_V1,
            family,
            source,
            limits,
            [IndexDelta::new(entry.key, None, Some(entry.value))],
            &mut |_, _| Ok(()),
        )?;
        report.add_run(&merged.report)?;
        runs.push(rebase::exact_merged_run(merged, count, 1)?);
    }
    if owner_count != 0 {
        let merged = stage.merge_index_run_visit(
            filesystem,
            input.revision,
            COORDINATOR_METADATA_PROFILE_V1,
            FAMILY_BLOB_OWNER,
            (base.owner_count() != 0).then_some(&base.metadata),
            limits,
            owners.values().map(|(reference, principal)| {
                let entry = encode_owner(*reference, *principal)?;
                IndexDelta::new(entry.key, None, Some(entry.value))
            }),
            &mut |_, _| Ok(()),
        )?;
        report.add_run(&merged.report)?;
        runs.push(rebase::exact_merged_run(merged, owner_count, owners.len())?);
    }
    let metadata = stage.finish(input, &runs)?.read_root().clone();
    let mut stage = recovery.stage_indexes_with_io(filesystem, transaction)?;
    let merged = stage.merge_index_run_visit(
        filesystem,
        input.revision,
        COORDINATOR_TRANSACTION_PROFILE_V1,
        1,
        Some(&base.transactions.root),
        limits,
        [IndexDelta::new(
            transaction.outcome.transaction_id.as_bytes().to_vec(),
            None,
            Some(transaction_value),
        )],
        &mut |_, _| Ok(()),
    )?;
    report.add_run(&merged.report)?;
    let run = rebase::exact_merged_run(merged, input.revision.get(), 1)?;
    let transactions = stage
        .finish(
            IndexRootInput {
                index_profile: COORDINATOR_TRANSACTION_PROFILE_V1,
                ..input
            },
            &[run],
        )?
        .read_root()
        .clone();
    report.revisions = 1;
    let first_references = if preserve_first_references && owner_count != 0 {
        let mut stage = recovery.stage_indexes_with_io(filesystem, transaction)?;
        let merged = stage.merge_index_run_visit(
            filesystem,
            input.revision,
            COORDINATOR_FIRST_REFERENCE_PROFILE_V1,
            1,
            base.first_references.as_ref(),
            limits,
            owners.keys().map(|id| {
                IndexDelta::new(
                    id.as_bytes().to_vec(),
                    None,
                    Some(input.revision.get().to_be_bytes().to_vec()),
                )
            }),
            &mut |_, _| Ok(()),
        )?;
        report.add_run(&merged.report)?;
        let run = rebase::exact_merged_run(merged, owner_count, owners.len())?;
        Some(
            stage
                .finish(
                    IndexRootInput {
                        index_profile: COORDINATOR_FIRST_REFERENCE_PROFILE_V1,
                        ..input
                    },
                    &[run],
                )?
                .read_root()
                .clone(),
        )
    } else {
        None
    };
    Ok((
        CoordinatorDiskBase {
            metadata,
            transactions: CoordinatorTransactionIndex { root: transactions },
            first_references,
            usage: None,
        },
        report,
    ))
}
