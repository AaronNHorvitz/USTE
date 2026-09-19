//! Opt-in map-free suffix metadata recovery for inventory-free reducers.
use super::*;
use uste_storage::{IndexRunMergeLimits, IndexRunMergeReport};

/// Total suffix and per-family private merge bounds. This path does not retain suffix maps.
#[derive(Clone, Copy, Debug)]
pub struct InventoryFreeMetadataRecoveryLimits {
    pub maximum_revisions: u64,
    pub merge: IndexRunMergeLimits,
}

/// Trusted private-merge diagnostics, not consumer cardinalities or complete physical I/O.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InventoryFreeMetadataRecoveryReport {
    pub revisions: u64,
    pub staged_runs: u64,
    pub output_entries: u64,
    pub output_logical_bytes: u64,
    pub pages_read: u64,
}

impl InventoryFreeMetadataRecoveryReport {
    pub(crate) fn add_run(&mut self, run: &IndexRunMergeReport) -> Result<(), TransactionError> {
        self.add(&Self {
            revisions: 0,
            staged_runs: 1,
            output_entries: run.output_entries,
            output_logical_bytes: run.output_logical_bytes,
            pages_read: run.base.stats.pages_read,
        })
    }

    fn add(&mut self, other: &Self) -> Result<(), TransactionError> {
        let add = |a: u64, b| a.checked_add(b).ok_or(TransactionError::ResourceLimit);
        let next = Self {
            revisions: add(self.revisions, other.revisions)?,
            staged_runs: add(self.staged_runs, other.staged_runs)?,
            output_entries: add(self.output_entries, other.output_entries)?,
            output_logical_bytes: add(self.output_logical_bytes, other.output_logical_bytes)?,
            pages_read: add(self.pages_read, other.pages_read)?,
        };
        *self = next;
        Ok(())
    }
}

impl<S, F, W, E, I> DiskCommitCoordinator<S, F, W, E, I>
where
    S: DiskCoordinatorState + JournalAnchoredTransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Recover a paired domain/metadata base with private metadata staging after every revision.
    /// No suffix outcome/transaction/owner map is accumulated. Inventories and optional owner
    /// projections refuse rather than being dropped; use the existing general path for them.
    /// Only terminal domain publication occurs. Metadata rebase remains required before new
    /// writes when this returns a private terminal metadata base. Existing retry expiry and
    /// current authorization remain the normal coordinator/facade responsibility.
    #[allow(clippy::too_many_arguments)]
    pub fn recover_with_inventory_free_streaming_domain<D: DiskRecoveryDomain<S, F, W, E, I>>(
        mut recovery: AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        mut base: CoordinatorDiskBase,
        mut state: S,
        retention: RetentionDays,
        limits: DiskCoordinatorRecoveryLimits,
        metadata_limits: InventoryFreeMetadataRecoveryLimits,
        cache: &mut PageCache,
        domain: &mut D,
    ) -> Result<(Self, InventoryFreeMetadataRecoveryReport), TransactionError> {
        let (scope, revision, certificate) =
            state.journal_base_anchor().map_err(map_apply_error)?;
        let frontier = recovery
            .journal
            .checkpoint_anchor()
            .ok_or(TransactionError::IntegrityFailure)?;
        let reducer_profile = *base.metadata.reducer_profile();
        let input = domain
            .initial_metadata_input(&state, (revision, certificate))
            .map_err(map_apply_error)?;
        if scope != recovery.scope()
            || input.scope != scope
            || input.revision != revision
            || input.certificate_digest != certificate
            || input.reducer_profile != reducer_profile
            || revision != base.metadata.revision()
            || revision > frontier.0
        {
            return Err(TransactionError::IntegrityFailure);
        }
        if base.owner_count() != 0
            || base.has_first_reference_evidence()
            || base.has_blob_usage_index()
        {
            return Err(TransactionError::InvalidRequest);
        }
        let revisions = frontier.0.get() - revision.get();
        if revisions > metadata_limits.maximum_revisions
            || frontier.0.get() > MAX_OUTCOMES_PER_NAMESPACE as u64
        {
            return Err(TransactionError::ResourceLimit);
        }
        domain.admit(revisions).map_err(map_open_error)?;
        domain
            .validate_initial_metadata_base(&state, &base.metadata)
            .map_err(map_apply_error)?;
        base.revalidate_journal_binding(&recovery.journal, filesystem, limits.lookup, cache)?;
        let mut report = InventoryFreeMetadataRecoveryReport::default();
        if revisions != 0 {
            let first = revision
                .checked_next()
                .map_err(|_| TransactionError::RevisionExhausted)?;
            let mut cursor = recovery.open_transaction_cursor(
                first,
                frontier.0,
                revisions,
                limits.maximum_encoded_bytes,
            )?;
            while let Some(transaction) =
                recovery.next_recovered_transaction(filesystem, &mut cursor)?
            {
                if transaction.blob_inventory.is_some() {
                    return Err(TransactionError::InvalidRequest);
                }
                if base
                    .retry_from_journal(
                        &recovery.journal,
                        filesystem,
                        transaction.principal,
                        transaction.idempotency_key,
                        limits.lookup,
                        cache,
                    )?
                    .is_some()
                    || base
                        .transaction_from_journal(
                            &recovery.journal,
                            filesystem,
                            transaction.outcome.transaction_id,
                            limits.lookup,
                            cache,
                        )?
                        .is_some()
                {
                    return Err(TransactionError::IntegrityFailure);
                }
                let prepared = domain
                    .prepare(&recovery, filesystem, &state, &transaction, cache)
                    .map_err(map_open_error)?;
                state
                    .validate_external_prepared(
                        &transaction.canonical_request,
                        None,
                        transaction.revision,
                        &prepared,
                    )
                    .map_err(streaming::recovery_apply_error)
                    .map_err(map_open_error)?;
                if S::result_digest(&prepared) != transaction.outcome.result_digest {
                    return Err(TransactionError::IntegrityFailure);
                }
                state.publish(prepared);
                domain
                    .advance(&mut recovery, filesystem, &mut state, &transaction, cache)
                    .map_err(map_open_error)?;
                let anchor = (transaction.revision, transaction.certificate_digest);
                if state.journal_base_anchor().map_err(map_apply_error)?
                    != (scope, anchor.0, anchor.1)
                {
                    return Err(TransactionError::IntegrityFailure);
                }
                let input = domain
                    .initial_metadata_input(&state, anchor)
                    .map_err(map_apply_error)?;
                let (next, step) = disk_metadata::stage_inventory_free_metadata_step(
                    &mut recovery,
                    filesystem,
                    &base,
                    &transaction,
                    input,
                    metadata_limits.merge,
                )?;
                report.add(&step)?;
                base = next;
            }
            recovery.finish_transaction_cursor(cursor)?;
        }
        if report.revisions != revisions
            || state.journal_base_anchor().map_err(map_apply_error)?
                != (scope, frontier.0, frontier.1)
        {
            return Err(TransactionError::IntegrityFailure);
        }
        domain
            .finish(&mut recovery, filesystem, &mut state, frontier, cache)
            .map_err(map_open_error)?;
        let terminal = state
            .metadata_publication_input(frontier)
            .map_err(map_apply_error)?;
        if terminal.scope != scope
            || terminal.revision != frontier.0
            || terminal.certificate_digest != frontier.1
            || terminal.reducer_profile != reducer_profile
        {
            return Err(TransactionError::IntegrityFailure);
        }
        state
            .validate_metadata_base(&base.metadata)
            .map_err(map_apply_error)?;
        let mut rebase_required = base.has_unpublished_roots();
        for profile in [
            COORDINATOR_METADATA_PROFILE_V1,
            COORDINATOR_TRANSACTION_PROFILE_V1,
            COORDINATOR_FIRST_REFERENCE_PROFILE_V1,
        ] {
            rebase_required |= recovery
                .load_index_root_manifests(filesystem, profile)?
                .iter()
                .any(|root| root.revision() > base.metadata.revision());
        }
        let inner = CommitCoordinator {
            scope,
            retention,
            journal: recovery.journal,
            state,
            outcomes: BTreeMap::new(),
            transactions: BTreeMap::new(),
            committed_blob_owners: BTreeMap::new(),
            recovered: true,
            uncertain: false,
        };
        Ok((
            Self {
                inner,
                base,
                overlay_limits: limits.overlay,
                rebase_required,
            },
            report,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamed_metadata_report_overflow_is_atomic() {
        let increment = InventoryFreeMetadataRecoveryReport {
            revisions: 1,
            staged_runs: 1,
            output_entries: 1,
            output_logical_bytes: 1,
            pages_read: 1,
        };
        for field in 0..5 {
            let mut report = InventoryFreeMetadataRecoveryReport::default();
            match field {
                0 => report.revisions = u64::MAX,
                1 => report.staged_runs = u64::MAX,
                2 => report.output_entries = u64::MAX,
                3 => report.output_logical_bytes = u64::MAX,
                _ => report.pages_read = u64::MAX,
            }
            let before = report.clone();
            assert_eq!(report.add(&increment), Err(TransactionError::ResourceLimit));
            assert_eq!(report, before);
        }
    }
}
