//! Trusted disk storage publication. Consumer authorization and staging ownership are separate.
use super::*;
use uste_storage::journal::{
    BlobMetadataRebuildLimits, BlobMetadataRebuildReport, DiskBlobAppendLimits,
};

impl<S, F, W, E, I> DiskCommitCoordinator<S, F, W, E, I>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Commit using explicit disk storage bounds for nonempty inventories. Exact retries still
    /// precede cancellation and new-write admission. Fresh inventory writes require disk blob
    /// recovery mode; no fallback reconstructs history maps or supplies implicit storage limits.
    #[allow(clippy::too_many_arguments)]
    pub fn commit_with_disk_inventory(
        &mut self,
        filesystem: &mut F,
        request: TransactionRequest<'_>,
        clock: &mut impl Clock,
        cancellation: &impl Cancellation,
        lookup: IndexGetLimits,
        storage: DiskBlobAppendLimits,
        cache: &mut PageCache,
    ) -> Result<TransactionOutcome, TransactionError> {
        let overlay = self.admitted_overlay_limits();
        self.inner.commit_with_preparation(
            filesystem,
            request,
            clock,
            cancellation,
            Some(DiskCommitMetadata {
                base: &self.base,
                overlay,
                lookup,
                storage: Some(storage),
                cache,
            }),
            |state, revision| {
                state.prepare(request.canonical_request, request.blob_inventory, revision)
            },
        )
    }

    /// External preparation must bind the exact inventory as well as request/base/revision.
    /// This privileged method does not broaden the inventory-free authorized graph writer.
    #[allow(clippy::too_many_arguments)]
    pub fn commit_prepared_with_disk_inventory(
        &mut self,
        filesystem: &mut F,
        request: TransactionRequest<'_>,
        prepared: S::Prepared,
        clock: &mut impl Clock,
        cancellation: &impl Cancellation,
        lookup: IndexGetLimits,
        storage: DiskBlobAppendLimits,
        cache: &mut PageCache,
    ) -> Result<TransactionOutcome, TransactionError>
    where
        S: ExternallyPreparedTransactionState,
    {
        let overlay = self.admitted_overlay_limits();
        self.inner.commit_with_preparation(
            filesystem,
            request,
            clock,
            cancellation,
            Some(DiskCommitMetadata {
                base: &self.base,
                overlay,
                lookup,
                storage: Some(storage),
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

    /// Trusted storage maintenance only. Coordinator metadata rebase is independent; neither
    /// operation can replace the other's base, outcome, owner or pending-domain requirements.
    pub fn refresh_disk_blob_metadata(
        &mut self,
        filesystem: &mut F,
        limits: BlobMetadataRebuildLimits,
        cache: &mut PageCache,
    ) -> Result<BlobMetadataRebuildReport, TransactionError> {
        if self.inner.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        self.inner
            .journal
            .refresh_disk_blob_metadata(filesystem, limits, cache)
            .map_err(map_open_error)
    }

    /// Privileged bounded pending-entry counts; not consumer quota or full-history residency.
    pub fn disk_blob_pending_residency(&self) -> Option<(usize, usize, usize)> {
        self.inner.journal.disk_blob_pending_residency()
    }
}
