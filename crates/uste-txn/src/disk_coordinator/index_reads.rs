//! Privileged bounded index reads; never expose the overlay-only legacy coordinator itself.

use super::*;

impl<S, F, W, E, I> DiskCommitCoordinator<S, F, W, E, I>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Trusted fixed-slot discovery. Candidates remain provisional until domain admission.
    pub fn load_index_root_manifests(
        &self,
        filesystem: &mut F,
        profile: [u8; 32],
    ) -> Result<Vec<RecoveredIndexRoot>, TransactionError> {
        self.inner.load_index_root_manifests(filesystem, profile)
    }

    /// Trusted bounded complete-run stream; callbacks are provisional until terminal success.
    pub fn visit_index_run(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        limits: IndexRunReadLimits,
        visitor: &mut IndexRunVisitor<'_>,
    ) -> Result<IndexRunReadReport, TransactionError> {
        self.inner
            .visit_index_run(filesystem, root, family, limits, visitor)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_get_bounded(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        key: &[u8],
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<(Option<Vec<u8>>, IndexReadStats), TransactionError> {
        self.inner
            .index_get_bounded(filesystem, root, family, key, limits, cache)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_get_predecessor(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        prefix: &[u8],
        upper_bound: &[u8],
        limits: IndexPredecessorLimits,
        cache: &mut PageCache,
    ) -> Result<IndexPredecessor, TransactionError> {
        self.inner.index_get_predecessor(
            filesystem,
            root,
            family,
            prefix,
            upper_bound,
            limits,
            cache,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_prefix(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        prefix: &[u8],
        maximum_results: usize,
        maximum_result_bytes: usize,
        cache: &mut PageCache,
    ) -> Result<IndexScan, TransactionError> {
        self.inner.index_scan_prefix(
            filesystem,
            root,
            family,
            prefix,
            maximum_results,
            maximum_result_bytes,
            cache,
        )
    }

    pub fn open_index_run_cursor(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        limits: IndexRunReadLimits,
    ) -> Result<IndexRunCursor<F>, TransactionError> {
        self.inner
            .open_index_run_cursor(filesystem, root, family, limits)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_prefix_bounded(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        prefix: &[u8],
        limits: IndexScanLimits,
        cache: &mut PageCache,
    ) -> Result<IndexScan, TransactionError> {
        self.inner
            .index_scan_prefix_bounded(filesystem, root, family, prefix, limits, cache)
    }

    pub fn next_index_run_entry(
        &self,
        filesystem: &mut F,
        cursor: &mut IndexRunCursor<F>,
    ) -> Result<Option<IndexEntry>, TransactionError> {
        self.inner.next_index_run_entry(filesystem, cursor)
    }

    pub fn finish_index_run_cursor(
        &self,
        cursor: IndexRunCursor<F>,
    ) -> Result<IndexRunReadReport, TransactionError> {
        self.inner.finish_index_run_cursor(cursor)
    }
}
