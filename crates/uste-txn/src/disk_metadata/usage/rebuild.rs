use super::*;

/// Count admission, not an allocator-specific maximum-RSS promise.
pub const MAX_BLOB_USAGE_REBUILD_BATCH_OWNERS: usize = 4096;

#[derive(Clone, Copy, Debug)]
pub struct CoordinatorBlobUsageRebuildLimits {
    pub source: IndexRunReadLimits,
    pub admission: CoordinatorBlobUsageLimits,
    pub merge: CoordinatorMetadataRebaseLimits,
    pub maximum_batch_owners: usize,
    pub maximum_batches: u64,
    /// Sum of every private run's logical key/value bytes, including repeated rewritten data.
    /// This is not encrypted file bytes or physical device I/O.
    pub maximum_merge_output_bytes: u64,
}

/// Privileged cardinality/work data; no consumer authorization capability is implied.
#[derive(Clone, Debug, Default)]
pub struct CoordinatorBlobUsageRebuildReport {
    pub source: IndexRunReadReport,
    pub batches: u64,
    pub merge_output_bytes: u64,
    pub owners: u64,
    pub principals: u64,
}

fn lower_bound(owners: u64, batch: u64) -> Result<u64, TransactionError> {
    if batch == 0 {
        return Err(TransactionError::ResourceLimit);
    }
    if owners == 0 {
        return Ok(KEY.len() as u64 + 24);
    }
    let full = owners / batch;
    let cumulative = full
        .checked_add(1)
        .and_then(|next| full.checked_mul(next))
        .and_then(|twice| (twice / 2).checked_mul(batch))
        .and_then(|sum| {
            sum.checked_add(if owners.is_multiple_of(batch) {
                0
            } else {
                owners
            })
        })
        .ok_or(TransactionError::ResourceLimit)?;
    cumulative
        .checked_mul(128)
        .and_then(|bytes| {
            owners
                .div_ceil(batch)
                .checked_mul(KEY.len() as u64 + 24 + 48)
                .and_then(|metadata_and_one_principal| {
                    bytes.checked_add(metadata_and_one_principal)
                })
        })
        .ok_or(TransactionError::ResourceLimit)
}

pub(crate) fn rebuild_usage<S, F, W, E, I>(
    coordinator: &mut CommitCoordinator<S, F, W, E, I>,
    filesystem: &mut F,
    base: &mut CoordinatorDiskBase,
    limits: CoordinatorBlobUsageRebuildLimits,
    cache: &mut PageCache,
) -> Result<CoordinatorBlobUsageRebuildReport, TransactionError>
where
    S: crate::DiskCoordinatorState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let owners = base.owner_count();
    if limits.maximum_batch_owners == 0
        || limits.maximum_batch_owners > MAX_BLOB_USAGE_REBUILD_BATCH_OWNERS
        || owners > limits.admission.maximum_owners
        || limits.admission.maximum_owners > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64
    {
        return Err(TransactionError::ResourceLimit);
    }
    let batch_size = limits.maximum_batch_owners as u64;
    let batches = owners.div_ceil(batch_size).max(1);
    if batches > limits.maximum_batches
        || lower_bound(owners, batch_size)? > limits.maximum_merge_output_bytes
    {
        return Err(TransactionError::ResourceLimit);
    }
    if !coordinator.outcomes.is_empty()
        || !coordinator.committed_blob_owners.is_empty()
        || coordinator.checkpoint_anchor()?
            != Some((
                base.metadata.revision(),
                *base.metadata.certificate_digest(),
            ))
    {
        return Err(TransactionError::InvalidRequest);
    }
    coordinator
        .state
        .validate_metadata_base(&base.metadata)
        .map_err(crate::map_apply_error)?;
    let input = IndexRootInput {
        scope: base.metadata.scope(),
        revision: base.metadata.revision(),
        certificate_digest: *base.metadata.certificate_digest(),
        reducer_profile: *base.metadata.reducer_profile(),
        logical_state_digest: *base.metadata.logical_state_digest(),
        index_profile: COORDINATOR_BLOB_USAGE_PROFILE_V1,
    };
    let journal = &mut coordinator.journal;
    let mut cursor = if owners == 0 {
        None
    } else {
        Some(
            journal
                .open_index_run_cursor(filesystem, &base.metadata, FAMILY_BLOB_OWNER, limits.source)
                .map_err(TransactionError::Storage)?,
        )
    };
    let mut report = CoordinatorBlobUsageRebuildReport::default();
    let mut staged: Option<BlobUsageIndex> = None;
    loop {
        let mut batch = Vec::new();
        batch
            .try_reserve_exact(limits.maximum_batch_owners.min(owners as usize))
            .map_err(|_| TransactionError::ResourceLimit)?;
        let mut finished = cursor.is_none();
        if let Some(cursor) = &mut cursor {
            while batch.len() < limits.maximum_batch_owners {
                let Some(entry) = journal
                    .next_index_run_entry(filesystem, cursor)
                    .map_err(TransactionError::Storage)?
                else {
                    finished = true;
                    break;
                };
                batch.push(
                    decode_owner(input.scope, &entry.key, &entry.value)
                        .map_err(TransactionError::Storage)?,
                );
            }
        }
        if batch.is_empty() && staged.is_some() {
            break;
        }
        report.batches = report
            .batches
            .checked_add(1)
            .filter(|count| *count <= limits.maximum_batches)
            .ok_or(TransactionError::ResourceLimit)?;
        let remaining = limits
            .maximum_merge_output_bytes
            .checked_sub(report.merge_output_bytes)
            .ok_or(TransactionError::ResourceLimit)?;
        let merged = merge_projection(
            journal,
            filesystem,
            staged.as_ref(),
            input,
            limits.merge.merge,
            limits.admission,
            batch.iter(),
            remaining,
        )?;
        report.merge_output_bytes = report
            .merge_output_bytes
            .checked_add(merged.output_bytes)
            .ok_or(TransactionError::ResourceLimit)?;
        // Current-frontier merge accepts the certified same-revision private root. This does
        // not relax historical recovery-stage ordering or rotate any durable root slot.
        let stage = journal
            .open_index_recovery_stage_with_io(
                filesystem,
                input.scope,
                input.revision,
                input.certificate_digest,
            )
            .map_err(TransactionError::Storage)?;
        let root = journal
            .finish_index_recovery_stage(stage, input, &merged.runs)
            .map_err(TransactionError::Storage)?
            .read_root()
            .clone();
        staged = Some(merged.into_index(root));
        if finished {
            break;
        }
    }
    if let Some(cursor) = cursor {
        report.source = journal
            .finish_index_run_cursor(cursor)
            .map_err(TransactionError::Storage)?;
    }
    if report.source.entries != owners || report.batches != batches {
        return Err(TransactionError::IntegrityFailure);
    }
    let candidate = staged.ok_or(TransactionError::IntegrityFailure)?;
    // A separate complete validation compares every secondary owner to the primary ledger.
    let mut admitted = base.validate_blob_usage_index(
        journal,
        filesystem,
        candidate.root,
        limits.admission,
        cache,
    )?;
    let runs = admitted.root.runs().copied().collect::<Vec<_>>();
    // Explicit rebuild publishes fresh runs, including when an old derived run is corrupt.
    admitted.root = journal
        .publish_index_root_recovered_bounded(filesystem, input, &runs, limits.merge.reuse)
        .map_err(TransactionError::Storage)?;
    report.owners = admitted.owners;
    report.principals = admitted.principals;
    base.usage = Some(admitted);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quota_rebuild_lower_bound_matches_a_literal_batch_sum() {
        for owners in 0_u64..=128 {
            for batch in 1..=33 {
                let expected = if owners == 0 {
                    37
                } else {
                    (1..=owners.div_ceil(batch))
                        .map(|step| 37 + 48 + 128 * (step * batch).min(owners))
                        .sum()
                };
                assert_eq!(lower_bound(owners, batch).unwrap(), expected);
            }
        }
        assert!(lower_bound(u64::MAX, 1).is_err());
        assert!(lower_bound(1, 0).is_err());
    }
}
