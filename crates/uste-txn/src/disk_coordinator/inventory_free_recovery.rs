//! Opt-in per-revision primary metadata staging, with a closed inventory-free wrapper.
use super::*;
use uste_storage::{IndexRunMergeLimits, IndexRunMergeReport, MAX_COMMITTED_BLOBS_PER_JOURNAL};

/// Total suffix and per-family private merge bounds. This path does not retain suffix maps.
#[derive(Clone, Copy, Debug)]
pub struct InventoryFreeMetadataRecoveryLimits {
    pub maximum_revisions: u64,
    pub merge: IndexRunMergeLimits,
}

/// Paired-base metadata recovery bounds. The chosen entry point determines which optional
/// projections are maintained; omitted attached projections must never be silently dropped.
#[derive(Clone, Copy, Debug)]
pub struct PrimaryMetadataRecoveryLimits {
    pub maximum_revisions: u64,
    pub maximum_blob_owners: u64,
    pub maximum_inventory_references: usize,
    pub merge: IndexRunMergeLimits,
}

/// Diagnostics for private primary metadata staging, with the same partial-I/O semantics.
pub type PrimaryMetadataRecoveryReport = InventoryFreeMetadataRecoveryReport;

#[derive(Clone, Copy, Eq, PartialEq)]
enum MetadataMode {
    InventoryFree,
    Primary,
    FirstReferences,
    IndexedUsage(IndexGetLimits),
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

    pub(crate) fn add(&mut self, other: &Self) -> Result<(), TransactionError> {
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
        recovery: AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        base: CoordinatorDiskBase,
        state: S,
        retention: RetentionDays,
        limits: DiskCoordinatorRecoveryLimits,
        metadata_limits: InventoryFreeMetadataRecoveryLimits,
        cache: &mut PageCache,
        domain: &mut D,
    ) -> Result<(Self, InventoryFreeMetadataRecoveryReport), TransactionError> {
        Self::recover_primary_metadata(
            recovery,
            filesystem,
            base,
            state,
            retention,
            limits,
            PrimaryMetadataRecoveryLimits {
                maximum_revisions: metadata_limits.maximum_revisions,
                maximum_blob_owners: 0,
                maximum_inventory_references: 0,
                merge: metadata_limits.merge,
            },
            MetadataMode::InventoryFree,
            cache,
            domain,
        )
    }

    /// Stage retry, transaction-ID and primary first-owner metadata after each certified step.
    /// Only the current bounded inventory's new owners are retained, never cumulative suffix
    /// maps. Repeated exact references preserve the first principal. Optional owner projections
    /// refuse explicitly; publication, authorization and retry rules are unchanged.
    #[allow(clippy::too_many_arguments)]
    pub fn recover_with_primary_metadata_streaming_domain<D: DiskRecoveryDomain<S, F, W, E, I>>(
        recovery: AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        base: CoordinatorDiskBase,
        state: S,
        retention: RetentionDays,
        limits: DiskCoordinatorRecoveryLimits,
        metadata_limits: PrimaryMetadataRecoveryLimits,
        cache: &mut PageCache,
        domain: &mut D,
    ) -> Result<(Self, PrimaryMetadataRecoveryReport), TransactionError> {
        Self::recover_primary_metadata(
            recovery,
            filesystem,
            base,
            state,
            retention,
            limits,
            metadata_limits,
            MetadataMode::Primary,
            cache,
            domain,
        )
    }

    /// Preserve first-reference evidence while privately staging each suffix revision.
    /// An owner-free base can bootstrap evidence; a populated base must already have independently
    /// admitted first references. Quota projections still refuse. Fresh writes require the usual
    /// first-reference-preserving terminal metadata rebase.
    #[allow(clippy::too_many_arguments)]
    pub fn recover_with_first_reference_streaming_domain<D: DiskRecoveryDomain<S, F, W, E, I>>(
        recovery: AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        base: CoordinatorDiskBase,
        state: S,
        retention: RetentionDays,
        limits: DiskCoordinatorRecoveryLimits,
        metadata_limits: PrimaryMetadataRecoveryLimits,
        cache: &mut PageCache,
        domain: &mut D,
    ) -> Result<(Self, PrimaryMetadataRecoveryReport), TransactionError> {
        Self::recover_primary_metadata(
            recovery,
            filesystem,
            base,
            state,
            retention,
            limits,
            metadata_limits,
            MetadataMode::FirstReferences,
            cache,
            domain,
        )
    }

    /// Preserve admitted first-reference and quota projections with private per-revision staging.
    /// An independently admitted quota root is mandatory even for an empty owner base; absence
    /// is never interpreted as zero usage. Only new first owners add charges to their principal.
    #[allow(clippy::too_many_arguments)]
    pub fn recover_with_indexed_usage_streaming_domain<D: DiskRecoveryDomain<S, F, W, E, I>>(
        recovery: AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        base: CoordinatorDiskBase,
        state: S,
        retention: RetentionDays,
        limits: DiskCoordinatorRecoveryLimits,
        metadata_limits: PrimaryMetadataRecoveryLimits,
        usage_lookup: IndexGetLimits,
        cache: &mut PageCache,
        domain: &mut D,
    ) -> Result<(Self, PrimaryMetadataRecoveryReport), TransactionError> {
        Self::recover_primary_metadata(
            recovery,
            filesystem,
            base,
            state,
            retention,
            limits,
            metadata_limits,
            MetadataMode::IndexedUsage(usage_lookup),
            cache,
            domain,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn recover_primary_metadata<D: DiskRecoveryDomain<S, F, W, E, I>>(
        mut recovery: AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        mut base: CoordinatorDiskBase,
        mut state: S,
        retention: RetentionDays,
        limits: DiskCoordinatorRecoveryLimits,
        metadata_limits: PrimaryMetadataRecoveryLimits,
        mode: MetadataMode,
        cache: &mut PageCache,
        domain: &mut D,
    ) -> Result<(Self, PrimaryMetadataRecoveryReport), TransactionError> {
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
        let first_references = matches!(
            mode,
            MetadataMode::FirstReferences | MetadataMode::IndexedUsage(_)
        );
        let usage_lookup = match mode {
            MetadataMode::IndexedUsage(lookup) => Some(lookup),
            _ => None,
        };
        if (mode == MetadataMode::InventoryFree && base.owner_count() != 0)
            || (!first_references && base.has_first_reference_evidence())
            || (first_references && base.owner_count() != 0 && !base.has_first_reference_evidence())
            || base.has_blob_usage_index() != usage_lookup.is_some()
        {
            return Err(TransactionError::InvalidRequest);
        }
        let revisions = frontier.0.get() - revision.get();
        if revisions > metadata_limits.maximum_revisions
            || frontier.0.get() > MAX_OUTCOMES_PER_NAMESPACE as u64
            || base.owner_count() > metadata_limits.maximum_blob_owners
            || metadata_limits.maximum_blob_owners > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64
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
                if mode == MetadataMode::InventoryFree && transaction.blob_inventory.is_some() {
                    return Err(TransactionError::InvalidRequest);
                }
                let mut owners = BTreeMap::new();
                if let Some(inventory) = transaction.blob_inventory.as_ref() {
                    if inventory.references().len() > metadata_limits.maximum_inventory_references {
                        return Err(TransactionError::ResourceLimit);
                    }
                    for reference in inventory.references() {
                        if reference.scope() != scope {
                            return Err(TransactionError::IntegrityFailure);
                        }
                        match base.owner_from_journal(
                            &recovery.journal,
                            filesystem,
                            reference.id(),
                            limits.lookup,
                            cache,
                        )? {
                            Some((stored, _)) if stored != *reference => {
                                return Err(TransactionError::IntegrityFailure);
                            }
                            Some(_) => {}
                            None => {
                                let count = base
                                    .owner_count()
                                    .checked_add(owners.len() as u64)
                                    .and_then(|n| n.checked_add(1))
                                    .ok_or(TransactionError::ResourceLimit)?;
                                if count > metadata_limits.maximum_blob_owners {
                                    return Err(TransactionError::ResourceLimit);
                                }
                                owners.insert(reference.id(), (*reference, transaction.principal));
                            }
                        }
                    }
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
                        transaction.blob_inventory.as_ref(),
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
                let (next, step) = disk_metadata::stage_primary_metadata_step(
                    &mut recovery,
                    filesystem,
                    &base,
                    &transaction,
                    input,
                    metadata_limits.merge,
                    &owners,
                    first_references,
                    usage_lookup,
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
