use super::*;

/// Trusted bounded domain recovery hooks. Intermediate work must remain private/unpublished.
/// The coordinator owns canonical binding, exact result-digest, retry/collision and first-owner
/// validation; the domain owns proof preparation, bounded scratch materialization and terminal
/// root publication. Hooks must not expose provisional state or treat this as consumer authority.
pub trait DiskRecoveryDomain<S, F, W, E, I>
where
    S: JournalAnchoredTransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Validate the initial private recovery anchor. The default requires an ordinary ready
    /// state; a domain may admit independently validated unpublished scratch roots here only.
    /// Terminal validation still uses the state's normal publication contract.
    fn initial_metadata_input(
        &self,
        state: &S,
        anchor: (CommitRevision, [u8; 32]),
    ) -> Result<IndexRootInput, ApplyError>
    where
        S: DiskCoordinatorState,
    {
        state.metadata_publication_input(anchor)
    }

    fn validate_initial_metadata_base(
        &self,
        state: &S,
        root: &RecoveredIndexRoot,
    ) -> Result<(), ApplyError>
    where
        S: DiskCoordinatorState,
    {
        state.validate_metadata_base(root)
    }

    /// Admit the total domain suffix before filesystem work. Per-step bounds remain mandatory.
    fn admit(&mut self, revisions: u64) -> Result<(), StorageError>;

    fn prepare(
        &mut self,
        recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        state: &S,
        transaction: &RecoveredFrontierTransaction,
        cache: &mut PageCache,
    ) -> Result<S::Prepared, StorageError>;

    /// Called only after exact prepared binding/digest checks and private reducer publication.
    fn advance(
        &mut self,
        recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        state: &mut S,
        transaction: &RecoveredFrontierTransaction,
        cache: &mut PageCache,
    ) -> Result<(), StorageError>;

    /// Called only after the complete shared-budget suffix and metadata validation succeeded.
    fn finish(
        &mut self,
        recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        state: &mut S,
        frontier: (CommitRevision, [u8; 32]),
        cache: &mut PageCache,
    ) -> Result<(), StorageError>;
}

impl<S, F, W, E, I> DiskCommitCoordinator<S, F, W, E, I>
where
    S: DiskCoordinatorState + JournalAnchoredTransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Replay a bounded multi-revision domain suffix without constructing a full state history.
    /// An admitted domain base may be ahead of the metadata base. No terminal callback occurs on
    /// partial range failure, and no coordinator is exposed on callback/publication failure.
    #[allow(clippy::too_many_arguments)]
    pub fn recover_with_streaming_domain<D: DiskRecoveryDomain<S, F, W, E, I>>(
        recovery: AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        base: CoordinatorDiskBase,
        state: S,
        retention: RetentionDays,
        limits: DiskCoordinatorRecoveryLimits,
        cache: &mut PageCache,
        domain: &mut D,
    ) -> Result<Self, TransactionError> {
        let (scope, revision, certificate) =
            state.journal_base_anchor().map_err(map_apply_error)?;
        let input = domain
            .initial_metadata_input(&state, (revision, certificate))
            .map_err(map_apply_error)?;
        let frontier = recovery
            .journal
            .checkpoint_anchor()
            .ok_or(TransactionError::IntegrityFailure)?;
        let reducer_profile = *base.metadata.reducer_profile();
        if scope != recovery.scope()
            || input.scope != scope
            || input.revision != revision
            || input.certificate_digest != certificate
            || input.reducer_profile != reducer_profile
            || revision < base.metadata.revision()
            || revision > frontier.0
        {
            return Err(TransactionError::IntegrityFailure);
        }
        domain
            .admit(frontier.0.get() - revision.get())
            .map_err(map_open_error)?;
        let mut domain_base_seen = revision == base.metadata.revision();
        if domain_base_seen {
            domain
                .validate_initial_metadata_base(&state, &base.metadata)
                .map_err(map_apply_error)?;
        }
        Self::recover_with_domain_replay(
            recovery,
            filesystem,
            base,
            state,
            retention,
            limits,
            cache,
            |recovery, filesystem, state, transaction, cache| {
                if let Some(transaction) = transaction {
                    if transaction.revision <= revision {
                        if transaction.revision == revision {
                            if transaction.certificate_digest != certificate {
                                return Err(StorageError::IntegrityFailure);
                            }
                            domain_base_seen = true;
                        }
                        return Ok(());
                    }
                    if !domain_base_seen {
                        return Err(StorageError::IntegrityFailure);
                    }
                    let prepared =
                        domain.prepare(recovery, filesystem, state, transaction, cache)?;
                    state
                        .validate_external_prepared(
                            &transaction.canonical_request,
                            transaction.blob_inventory.as_ref(),
                            transaction.revision,
                            &prepared,
                        )
                        .map_err(recovery_apply_error)?;
                    if S::result_digest(&prepared) != transaction.outcome.result_digest {
                        return Err(StorageError::IntegrityFailure);
                    }
                    state.publish(prepared);
                    domain.advance(recovery, filesystem, state, transaction, cache)?;
                    if state.journal_base_anchor().map_err(recovery_apply_error)?
                        != (scope, transaction.revision, transaction.certificate_digest)
                    {
                        return Err(StorageError::IntegrityFailure);
                    }
                } else {
                    if !domain_base_seen
                        || state.journal_base_anchor().map_err(recovery_apply_error)?
                            != (scope, frontier.0, frontier.1)
                    {
                        return Err(StorageError::IntegrityFailure);
                    }
                    domain.finish(recovery, filesystem, state, frontier, cache)?;
                    let terminal = state
                        .metadata_publication_input(frontier)
                        .map_err(recovery_apply_error)?;
                    if terminal.scope != scope
                        || terminal.revision != frontier.0
                        || terminal.certificate_digest != frontier.1
                        || terminal.reducer_profile != reducer_profile
                    {
                        return Err(StorageError::IntegrityFailure);
                    }
                }
                Ok(())
            },
        )
    }
}

fn recovery_apply_error(error: ApplyError) -> StorageError {
    match error {
        ApplyError::ResourceLimit => StorageError::ResourceLimit,
        _ => StorageError::IntegrityFailure,
    }
}
