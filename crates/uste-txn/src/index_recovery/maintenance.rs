use super::*;
use uste_storage::journal::IndexRecoveryStage;
use uste_storage::{
    IndexDelta, IndexRootInput, IndexRunDescriptor, IndexRunMergeLimits, MergedIndexRun,
    StagedIndexRoot,
};

impl RecoveredFrontierTransaction {
    pub(crate) fn packed_maintenance<'a, F, W, E, I>(
        &self,
        scope: NamespaceRef,
        journal: &'a mut JournalStore<F, W, E, I>,
        filesystem: &mut F,
        limits: uste_storage::journal::CertificateAnchorReadLimits,
    ) -> Result<crate::PackedIndexMaintenance<'a, F, W, E, I>, TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        if self.scope != scope {
            return Err(TransactionError::IntegrityFailure);
        }
        let target = if let Some(proof) = &self.certificate_proof {
            if proof.anchor() != (self.revision, self.certificate_digest) {
                return Err(TransactionError::IntegrityFailure);
            }
            proof.clone()
        } else {
            journal
                .authenticate_certificate_anchor(
                    filesystem,
                    self.revision,
                    self.certificate_digest,
                    limits,
                )
                .map_err(map_open_error)?
        };
        crate::PackedIndexMaintenance::new(scope, journal, target)
    }
}

/// Exclusive, scope-bound scratch index maintenance for one authenticated transaction.
/// This cannot append a transaction or publish an intermediate root slot.
pub struct RecoveryIndexMaintenance<'a, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    journal: &'a mut JournalStore<F, W, E, I>,
    stage: Option<IndexRecoveryStage>,
    anchor: (CommitRevision, [u8; 32]),
}

impl<F, W, E, I> AuthenticatedIndexRecovery<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Pin private packed-family construction to one authenticated recovery transaction.
    /// Bound receipts never fall back to I/O after owner/frontier rejection.
    pub fn packed_indexes_with_io(
        &mut self,
        filesystem: &mut F,
        transaction: &RecoveredFrontierTransaction,
        limits: uste_storage::journal::CertificateAnchorReadLimits,
    ) -> Result<crate::PackedIndexMaintenance<'_, F, W, E, I>, TransactionError> {
        transaction.packed_maintenance(self.scope, &mut self.journal, filesystem, limits)
    }

    /// Reuse a cursor's exact live-owner certificate evidence when present; otherwise use the
    /// explicit-I/O proof path. A bound transaction never falls back after proof rejection.
    pub fn stage_indexes_with_io(
        &mut self,
        filesystem: &mut F,
        transaction: &RecoveredFrontierTransaction,
    ) -> Result<RecoveryIndexMaintenance<'_, F, W, E, I>, TransactionError> {
        if transaction.scope != self.scope {
            return Err(TransactionError::IntegrityFailure);
        }
        let stage = if let Some(proof) = transaction.certificate_proof.as_ref() {
            if proof.anchor() != (transaction.revision, transaction.certificate_digest) {
                return Err(TransactionError::IntegrityFailure);
            }
            self.journal
                .open_proven_index_recovery_stage(self.scope, proof)
        } else {
            self.journal.open_index_recovery_stage_with_io(
                filesystem,
                self.scope,
                transaction.revision,
                transaction.certificate_digest,
            )
        }
        .map_err(map_open_error)?;
        Ok(RecoveryIndexMaintenance {
            journal: &mut self.journal,
            stage: Some(stage),
            anchor: (transaction.revision, transaction.certificate_digest),
        })
    }

    pub fn stage_indexes(
        &mut self,
        transaction: &RecoveredFrontierTransaction,
    ) -> Result<RecoveryIndexMaintenance<'_, F, W, E, I>, TransactionError> {
        if transaction.scope != self.scope {
            return Err(TransactionError::IntegrityFailure);
        }
        let stage = if let Some(proof) = transaction.certificate_proof.as_ref() {
            if proof.anchor() != (transaction.revision, transaction.certificate_digest) {
                return Err(TransactionError::IntegrityFailure);
            }
            self.journal
                .open_proven_index_recovery_stage(self.scope, proof)
        } else {
            self.journal.open_index_recovery_stage(
                self.scope,
                transaction.revision,
                transaction.certificate_digest,
            )
        }
        .map_err(map_open_error)?;
        Ok(RecoveryIndexMaintenance {
            journal: &mut self.journal,
            stage: Some(stage),
            anchor: (transaction.revision, transaction.certificate_digest),
        })
    }

    /// Trusted terminal recovery publication; storage still requires the exact current frontier.
    pub fn publish_recovered_index_root(
        &mut self,
        filesystem: &mut F,
        input: IndexRootInput,
        runs: &[IndexRunDescriptor],
        fallback_limits: IndexRunReadLimits,
    ) -> Result<RecoveredIndexRoot, TransactionError> {
        if input.scope != self.scope {
            return Err(TransactionError::IntegrityFailure);
        }
        self.journal
            .publish_index_root_recovered_bounded(filesystem, input, runs, fallback_limits)
            .map_err(map_open_error)
    }

    /// Finish durability of a visible terminal candidate after an earlier uncertain publication,
    /// preserving both slots and using explicit per-run authentication limits.
    pub fn resynchronize_recovered_index_root(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        limits: IndexRunReadLimits,
    ) -> Result<(), TransactionError> {
        if root.scope() != self.scope {
            return Err(TransactionError::IntegrityFailure);
        }
        self.journal
            .resync_index_root_bounded(filesystem, root, limits)
            .map_err(map_open_error)
    }
}

impl<F, W, E, I> RecoveryIndexMaintenance<'_, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    pub fn anchor(&self) -> Result<(CommitRevision, [u8; 32]), TransactionError> {
        self.stage
            .as_ref()
            .ok_or(TransactionError::InvalidRequest)?;
        Ok(self.anchor)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn merge_index_run_visit<T>(
        &mut self,
        filesystem: &mut F,
        revision: CommitRevision,
        index_profile: [u8; 32],
        family: u8,
        base: Option<&RecoveredIndexRoot>,
        limits: IndexRunMergeLimits,
        deltas: T,
        visitor: &mut IndexRunVisitor<'_>,
    ) -> Result<MergedIndexRun, TransactionError>
    where
        T: IntoIterator<Item = Result<IndexDelta, StorageError>>,
    {
        if revision != self.anchor.0 {
            return Err(TransactionError::IntegrityFailure);
        }
        let stage = self
            .stage
            .as_ref()
            .ok_or(TransactionError::InvalidRequest)?;
        self.journal
            .stage_index_merge_visit(
                filesystem,
                stage,
                index_profile,
                family,
                base,
                limits,
                deltas,
                visitor,
            )
            .map_err(map_open_error)
    }

    pub fn finish(
        &mut self,
        input: IndexRootInput,
        runs: &[IndexRunDescriptor],
    ) -> Result<StagedIndexRoot, TransactionError> {
        let stage = self.stage.take().ok_or(TransactionError::InvalidRequest)?;
        self.journal
            .finish_index_recovery_stage(stage, input, runs)
            .map_err(map_open_error)
    }
}
