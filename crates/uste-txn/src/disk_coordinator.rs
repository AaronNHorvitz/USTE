//! Opt-in explicit-I/O coordinator over a validated immutable metadata base and bounded overlays.

use super::*;

/// Trusted domain proof that the supplied live state is exactly the metadata base's state.
/// Implementations must check scope, revision, reducer profile and logical state digest (or an
/// equivalent independently admitted full anchor). Pending/unpublished domain states must fail.
pub trait DiskCoordinatorState: TransactionState {
    fn validate_metadata_base(&self, root: &RecoveredIndexRoot) -> Result<(), ApplyError>;
}

/// Bounds for rebuilding only post-base coordinator metadata and replaying ordinary reducers.
#[derive(Clone, Copy, Debug)]
pub struct DiskCoordinatorRecoveryLimits {
    pub overlay: CoordinatorRecoveryLimits,
    pub lookup: IndexGetLimits,
    pub maximum_encoded_bytes: u64,
}

pub(crate) struct DiskCommitMetadata<'a> {
    pub base: &'a CoordinatorDiskBase,
    pub overlay: CoordinatorRecoveryLimits,
    pub lookup: IndexGetLimits,
    pub cache: &'a mut PageCache,
}

/// Privileged coordinator with explicit disk I/O and bounded post-base metadata overlays.
/// It intentionally does not expose the legacy coordinator's memory-only authorization/read
/// adapter. Consumer authorization must use a separately implemented disk-aware adapter.
pub struct DiskCommitCoordinator<S, F, W, E, I>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    inner: CommitCoordinator<S, F, W, E, I>,
    base: CoordinatorDiskBase,
    overlay_limits: CoordinatorRecoveryLimits,
}

impl<S, F, W, E, I> DiskCommitCoordinator<S, F, W, E, I>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Consume the exclusive recovery owner at the exact admitted base frontier. No reopen,
    /// journal-origin coordinator map reconstruction, or ownership-release race occurs.
    pub fn from_admitted_base(
        recovery: AuthenticatedIndexRecovery<F, W, E, I>,
        base: CoordinatorDiskBase,
        state: S,
        retention: RetentionDays,
        overlay_limits: CoordinatorRecoveryLimits,
    ) -> Result<Self, TransactionError>
    where
        S: DiskCoordinatorState,
    {
        if recovery.scope() != base.metadata.scope()
            || recovery.journal.checkpoint_anchor()
                != Some((
                    base.metadata.revision(),
                    *base.metadata.certificate_digest(),
                ))
        {
            return Err(TransactionError::IntegrityFailure);
        }
        state
            .validate_metadata_base(&base.metadata)
            .map_err(map_apply_error)?;
        let inner = CommitCoordinator {
            scope: recovery.scope(),
            retention,
            journal: recovery.journal,
            state,
            outcomes: BTreeMap::new(),
            transactions: BTreeMap::new(),
            committed_blob_owners: BTreeMap::new(),
            recovered: true,
            uncertain: false,
        };
        Ok(Self {
            inner,
            base,
            overlay_limits,
        })
    }

    pub fn state(&self) -> Result<&S, TransactionError> {
        if self.inner.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        Ok(&self.inner.state)
    }

    /// Replay an authenticated suffix into bounded private overlays and an ordinary reducer.
    /// No state is returned until the terminal group succeeds. Reducers requiring external I/O
    /// preparation (including pending disk-graph states) are not silently reconstructed in RAM;
    /// their `prepare` refusal propagates and their separate streaming path remains required.
    #[allow(clippy::too_many_arguments)]
    pub fn recover_from_admitted_base(
        recovery: AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        base: CoordinatorDiskBase,
        mut state: S,
        retention: RetentionDays,
        limits: DiskCoordinatorRecoveryLimits,
        cache: &mut PageCache,
    ) -> Result<Self, TransactionError>
    where
        S: DiskCoordinatorState,
    {
        let frontier = recovery
            .journal
            .frontier()
            .ok_or(TransactionError::IntegrityFailure)?;
        if recovery.scope() != base.metadata.scope() || frontier < base.metadata.revision() {
            return Err(TransactionError::IntegrityFailure);
        }
        state
            .validate_metadata_base(&base.metadata)
            .map_err(map_apply_error)?;
        let suffix_count = frontier.get() - base.metadata.revision().get();
        if suffix_count > limits.overlay.maximum_outcomes as u64
            || frontier.get() > MAX_OUTCOMES_PER_NAMESPACE as u64
        {
            return Err(TransactionError::ResourceLimit);
        }
        base.revalidate_journal_binding(&recovery.journal, filesystem, limits.lookup, cache)?;
        let mut outcomes = BTreeMap::new();
        let mut transactions = BTreeMap::new();
        let mut owners = BTreeMap::new();
        if suffix_count != 0 {
            let first = base
                .metadata
                .revision()
                .checked_next()
                .map_err(|_| TransactionError::RevisionExhausted)?;
            recovery.visit_transactions(
                filesystem,
                first,
                frontier,
                suffix_count,
                limits.maximum_encoded_bytes,
                |filesystem, transaction| {
                    let key = RetryKey {
                        principal: transaction.principal,
                        key: transaction.idempotency_key,
                    };
                    if outcomes.contains_key(&key)
                        || transactions.contains_key(&transaction.outcome.transaction_id)
                        || base
                            .retry_from_journal(
                                &recovery.journal,
                                filesystem,
                                key.principal,
                                key.key,
                                limits.lookup,
                                cache,
                            )
                            .map_err(recovery_storage_error)?
                            .is_some()
                        || base
                            .transaction_from_journal(
                                &recovery.journal,
                                filesystem,
                                transaction.outcome.transaction_id,
                                limits.lookup,
                                cache,
                            )
                            .map_err(recovery_storage_error)?
                            .is_some()
                    {
                        return Err(StorageError::IntegrityFailure);
                    }
                    if let Some(inventory) = transaction.blob_inventory.as_ref() {
                        for reference in inventory.references() {
                            let identity = (reference.scope(), reference.id());
                            let owner = match owners.get(&identity) {
                                Some(value) => Some(*value),
                                None => base
                                    .owner_from_journal(
                                        &recovery.journal,
                                        filesystem,
                                        reference.id(),
                                        limits.lookup,
                                        cache,
                                    )
                                    .map_err(recovery_storage_error)?,
                            };
                            if let Some((stored, _)) = owner {
                                if stored != *reference {
                                    return Err(StorageError::IntegrityFailure);
                                }
                            } else {
                                if owners.len() >= limits.overlay.maximum_blob_owners {
                                    return Err(StorageError::ResourceLimit);
                                }
                                owners.insert(identity, (*reference, transaction.principal));
                            }
                        }
                    }
                    let prepared = state
                        .prepare(
                            &transaction.canonical_request,
                            transaction.blob_inventory.as_ref(),
                            transaction.revision,
                        )
                        .map_err(|error| match error {
                            ApplyError::ResourceLimit => StorageError::ResourceLimit,
                            _ => StorageError::IntegrityFailure,
                        })?;
                    if S::result_digest(&prepared) != transaction.outcome.result_digest {
                        return Err(StorageError::IntegrityFailure);
                    }
                    state.publish(prepared);
                    outcomes.insert(key, transaction.outcome);
                    transactions.insert(
                        transaction.outcome.transaction_id,
                        (transaction.principal, transaction.outcome),
                    );
                    Ok(())
                },
            )?;
        }
        let inner = CommitCoordinator {
            scope: recovery.scope(),
            retention,
            journal: recovery.journal,
            state,
            outcomes,
            transactions,
            committed_blob_owners: owners,
            recovered: true,
            uncertain: false,
        };
        Ok(Self {
            inner,
            base,
            overlay_limits: limits.overlay,
        })
    }

    pub fn checkpoint_anchor(
        &self,
    ) -> Result<Option<(CommitRevision, [u8; 32])>, TransactionError> {
        self.inner.checkpoint_anchor()
    }

    /// Number of post-base outcomes and first owners retained in memory, excluding storage maps.
    pub fn overlay_counts(&self) -> (usize, usize) {
        (
            self.inner.outcomes.len(),
            self.inner.committed_blob_owners.len(),
        )
    }

    pub fn start_blob_upload(
        &mut self,
        scope: NamespaceRef,
    ) -> Result<BlobUpload, TransactionError> {
        self.inner.start_blob_upload(scope)
    }

    pub fn resume_blob_upload(
        &mut self,
        filesystem: &mut F,
        token: BlobUploadToken,
    ) -> Result<BlobUpload, TransactionError> {
        self.inner.resume_blob_upload(filesystem, token)
    }

    pub fn write_blob_upload(
        &mut self,
        filesystem: &mut F,
        upload: &mut BlobUpload,
        input: &[u8],
    ) -> Result<(), TransactionError> {
        self.inner.write_blob_upload(filesystem, upload, input)
    }

    pub fn finish_blob_upload(
        &mut self,
        filesystem: &mut F,
        upload: &mut BlobUpload,
    ) -> Result<BlobReference, TransactionError> {
        self.inner.finish_blob_upload(filesystem, upload)
    }

    pub fn abort_blob_upload(
        &mut self,
        filesystem: &mut F,
        upload: &mut BlobUpload,
    ) -> Result<(), TransactionError> {
        self.inner.abort_blob_upload(filesystem, upload)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit(
        &mut self,
        filesystem: &mut F,
        request: TransactionRequest<'_>,
        clock: &mut impl Clock,
        cancellation: &impl Cancellation,
        lookup: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<TransactionOutcome, TransactionError> {
        self.inner.commit_with_preparation(
            filesystem,
            request,
            clock,
            cancellation,
            Some(DiskCommitMetadata {
                base: &self.base,
                overlay: self.overlay_limits,
                lookup,
                cache,
            }),
            |state, revision| {
                state.prepare(request.canonical_request, request.blob_inventory, revision)
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_prepared(
        &mut self,
        filesystem: &mut F,
        request: TransactionRequest<'_>,
        prepared: S::Prepared,
        clock: &mut impl Clock,
        cancellation: &impl Cancellation,
        lookup: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<TransactionOutcome, TransactionError>
    where
        S: ExternallyPreparedTransactionState,
    {
        self.inner.commit_with_preparation(
            filesystem,
            request,
            clock,
            cancellation,
            Some(DiskCommitMetadata {
                base: &self.base,
                overlay: self.overlay_limits,
                lookup,
                cache,
            }),
            move |state, revision| {
                state.validate_external_prepared(
                    request.canonical_request,
                    request.blob_inventory,
                    revision,
                    &prepared,
                )?;
                Ok(prepared)
            },
        )
    }

    /// Privileged exact retry outcome, preserving principal isolation and expiry semantics.
    #[allow(clippy::too_many_arguments)]
    pub fn outcome(
        &self,
        filesystem: &mut F,
        principal: PrincipalDigest,
        key: IdempotencyKey,
        now: UtcInstant,
        lookup: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<Option<TransactionOutcome>, TransactionError> {
        if self.inner.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        let outcome = match self.inner.outcomes.get(&RetryKey { principal, key }) {
            Some(value) => Some(*value),
            None => self.base.retry_from_journal(
                &self.inner.journal,
                filesystem,
                principal,
                key,
                lookup,
                cache,
            )?,
        };
        match outcome {
            Some(outcome) if now >= outcome.expires_at => Err(TransactionError::IdempotencyExpired),
            other => Ok(other),
        }
    }

    /// Privileged transaction-ID lookup with principal isolation and expiry checking.
    #[allow(clippy::too_many_arguments)]
    pub fn transaction_outcome(
        &self,
        filesystem: &mut F,
        principal: PrincipalDigest,
        id: TransactionId,
        now: UtcInstant,
        lookup: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<Option<TransactionOutcome>, TransactionError> {
        if self.inner.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        let outcome = match self.inner.transactions.get(&id) {
            Some(value) => Some(*value),
            None => self.base.transaction_from_journal(
                &self.inner.journal,
                filesystem,
                id,
                lookup,
                cache,
            )?,
        }
        .filter(|(owner, _)| *owner == principal)
        .map(|(_, outcome)| outcome);
        match outcome {
            Some(outcome) if now >= outcome.expires_at => Err(TransactionError::IdempotencyExpired),
            other => Ok(other),
        }
    }

    /// Trusted metadata lookup, not a blob-read capability.
    pub fn committed_blob_owner(
        &self,
        filesystem: &mut F,
        reference: BlobReference,
        lookup: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<Option<PrincipalDigest>, TransactionError> {
        if self.inner.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        if reference.scope() != self.inner.scope {
            return Err(TransactionError::InvalidRequest);
        }
        let owner = match self
            .inner
            .committed_blob_owners
            .get(&(reference.scope(), reference.id()))
        {
            Some(value) => Some(*value),
            None => self.base.owner_from_journal(
                &self.inner.journal,
                filesystem,
                reference.id(),
                lookup,
                cache,
            )?,
        };
        Ok(owner
            .filter(|(stored, _)| *stored == reference)
            .map(|(_, principal)| principal))
    }

    pub fn reducer_and_index_maintenance(
        &mut self,
    ) -> Result<ReducerIndexMaintenance<'_, S, F, W, E, I>, TransactionError> {
        self.inner.reducer_and_index_maintenance()
    }

    pub fn install_postcommit_publication(
        &mut self,
        publication: S::Publication,
    ) -> Result<(), TransactionError>
    where
        S: PostCommitStateMaintenance,
    {
        self.inner.install_postcommit_publication(publication)
    }
}

fn recovery_storage_error(error: TransactionError) -> StorageError {
    match error {
        TransactionError::Storage(error) => error,
        TransactionError::ResourceLimit => StorageError::ResourceLimit,
        _ => StorageError::IntegrityFailure,
    }
}
