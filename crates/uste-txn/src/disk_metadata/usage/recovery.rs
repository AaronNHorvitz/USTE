//! Private quota projection staging for one authenticated transaction's new first owners.
use super::*;
use crate::{PrimaryMetadataRecoveryReport, RecoveredFrontierTransaction, RecoveredGenesis};
use uste_storage::IndexRunMergeLimits;

/// Stage a private quota candidate for the reconstructed first transaction, including zero
/// owners. Independent quota-to-primary admission is still mandatory before recovery uses it.
pub fn stage_genesis_blob_usage<S, F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    filesystem: &mut F,
    genesis: &RecoveredGenesis<S>,
    limits: CoordinatorBlobUsageLimits,
    merge: IndexRunMergeLimits,
) -> Result<RecoveredIndexRoot, TransactionError>
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
        || state.current_checkpoint_scope() != recovery.scope()
        || state.current_checkpoint_revision() != Some(CommitRevision::FIRST)
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let references = transaction
        .blob_inventory
        .as_ref()
        .map_or(&[][..], |inventory| inventory.references());
    if references.len() as u64 > limits.maximum_owners
        || limits.maximum_owners > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64
    {
        return Err(TransactionError::ResourceLimit);
    }
    let owners = references
        .iter()
        .map(|reference| (reference.id(), (*reference, transaction.principal)))
        .collect();
    let input = IndexRootInput {
        scope: recovery.scope(),
        revision: CommitRevision::FIRST,
        certificate_digest: *transaction.certificate_digest(),
        reducer_profile: S::REDUCER_PROFILE,
        logical_state_digest: state
            .current_logical_state_digest()
            .map_err(checkpoint_error)?,
        index_profile: COORDINATOR_BLOB_USAGE_PROFILE_V1,
    };
    stage_projection(
        recovery,
        filesystem,
        None,
        transaction,
        input,
        &owners,
        merge,
        limits.lookup,
    )
    .map(|(index, _)| index.root)
}

#[allow(clippy::too_many_arguments)]
pub(in crate::disk_metadata) fn stage_projection<F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    filesystem: &mut F,
    before: Option<&BlobUsageIndex>,
    transaction: &RecoveredFrontierTransaction,
    input: IndexRootInput,
    additions: &BTreeMap<BlobId, (BlobReference, PrincipalDigest)>,
    merge: IndexRunMergeLimits,
    lookup: IndexGetLimits,
) -> Result<(BlobUsageIndex, PrimaryMetadataRecoveryReport), TransactionError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if input.scope != recovery.scope()
        || input.revision != transaction.revision
        || input.certificate_digest != transaction.certificate_digest
        || transaction.scope != input.scope
        || (before.is_none() && input.revision != CommitRevision::FIRST)
        || before.is_some_and(|base| {
            base.root.revision().checked_next().ok() != Some(input.revision)
                || base.root.scope() != input.scope
                || base.root.reducer_profile() != &input.reducer_profile
                || base.root.index_profile() != &COORDINATOR_BLOB_USAGE_PROFILE_V1
        })
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let owners = add(before.map_or(0, |b| b.owners), additions.len() as u64)
        .map_err(TransactionError::Storage)?;
    if owners > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64 {
        return Err(TransactionError::ResourceLimit);
    }
    let charged = additions
        .iter()
        .try_fold(0_u64, |bytes, (id, (reference, principal))| {
            if *id != reference.id()
                || reference.scope() != input.scope
                || *principal != transaction.principal
            {
                return Err(StorageError::IntegrityFailure);
            }
            add(bytes, reference.byte_len())
        })
        .map_err(TransactionError::Storage)?;
    let bytes = add(before.map_or(0, |b| b.bytes), charged).map_err(TransactionError::Storage)?;
    let mut principals = before.map_or(0, |b| b.principals);
    let mut principal_delta = None;
    let mut inserted_principal = 0;
    if !additions.is_empty() {
        let previous = if let Some(base) = before.filter(|base| base.principals != 0) {
            recovery
                .index_get_bounded(
                    filesystem,
                    &base.root,
                    PRINCIPALS,
                    &transaction.principal.as_bytes(),
                    lookup,
                    &mut PageCache::new(64 * 1024).map_err(TransactionError::Storage)?,
                )?
                .0
        } else {
            None
        };
        let (count, total) = previous
            .as_deref()
            .map(decode_pair)
            .transpose()
            .map_err(TransactionError::Storage)?
            .unwrap_or((0, 0));
        if previous.is_none() {
            principals = add(principals, 1).map_err(TransactionError::Storage)?;
            inserted_principal = 1;
        }
        principal_delta = Some(
            IndexDelta::new(
                transaction.principal.as_bytes().to_vec(),
                previous,
                Some(pair(
                    add(count, additions.len() as u64).map_err(TransactionError::Storage)?,
                    add(total, charged).map_err(TransactionError::Storage)?,
                )),
            )
            .map_err(TransactionError::Storage)?,
        );
    }
    let mut report = PrimaryMetadataRecoveryReport::default();
    let mut stage = recovery.stage_indexes_with_io(filesystem, transaction)?;
    let mut metadata = pair(owners, bytes);
    metadata.extend_from_slice(&principals.to_be_bytes());
    let mut runs = Vec::with_capacity(3);
    let head = stage.merge_index_run_visit(
        filesystem,
        input.revision,
        COORDINATOR_BLOB_USAGE_PROFILE_V1,
        META,
        None,
        merge,
        [IndexDelta::new(KEY.to_vec(), None, Some(metadata))],
        &mut |_, _| Ok(()),
    )?;
    report.add_run(&head.report)?;
    runs.push(rebase::exact_merged_run(head, 1, 1)?);
    if owners != 0 {
        let changed_principal = usize::from(principal_delta.is_some());
        let base_root = before.filter(|b| b.owners != 0).map(|b| &b.root);
        let merged = stage.merge_index_run_visit(
            filesystem,
            input.revision,
            COORDINATOR_BLOB_USAGE_PROFILE_V1,
            PRINCIPALS,
            base_root,
            merge,
            principal_delta.into_iter().map(Ok),
            &mut |_, _| Ok(()),
        )?;
        if merged.report.insertions != inserted_principal
            || merged.report.replacements != changed_principal as u64 - inserted_principal
            || merged.report.deletions != 0
            || merged.report.output_entries != principals
        {
            return Err(TransactionError::IntegrityFailure);
        }
        report.add_run(&merged.report)?;
        runs.push(merged.run.ok_or(TransactionError::IntegrityFailure)?);
        let merged = stage.merge_index_run_visit(
            filesystem,
            input.revision,
            COORDINATOR_BLOB_USAGE_PROFILE_V1,
            OWNERS,
            base_root,
            merge,
            additions.values().map(|(reference, principal)| {
                let entry = encode_owner(*reference, *principal)?;
                let mut key = principal.as_bytes().to_vec();
                key.extend_from_slice(&entry.key);
                IndexDelta::new(key, None, Some(entry.value))
            }),
            &mut |_, _| Ok(()),
        )?;
        report.add_run(&merged.report)?;
        runs.push(rebase::exact_merged_run(merged, owners, additions.len())?);
    }
    let root = stage
        .finish(
            IndexRootInput {
                index_profile: COORDINATOR_BLOB_USAGE_PROFILE_V1,
                ..input
            },
            &runs,
        )?
        .read_root()
        .clone();
    Ok((
        BlobUsageIndex {
            root,
            owners,
            bytes,
            principals,
        },
        report,
    ))
}
