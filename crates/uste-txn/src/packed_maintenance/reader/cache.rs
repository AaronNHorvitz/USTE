use super::*;
use uste_storage::packed_page_cache::PackedPageCache;

impl<F, W, E, I> PackedIndexReader<'_, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Same proof-work admission as uncached lookup, regardless of cache warmth.
    pub fn get_cached(
        &self,
        filesystem: &mut F,
        tree: &CanonicalPackedTree,
        key: &[u8],
        limits: TreeLookupLimits,
        cache: &mut PackedPageCache,
    ) -> Result<TreeLookupResult, TransactionError> {
        self.check(tree.context())?;
        self.journal
            .packed_tree_get_cached(filesystem, tree, key, limits, cache)
            .map_err(TransactionError::Storage)
    }
    /// Convert a successful cached value while borrowing resident plaintext. The result cannot
    /// borrow from the cache; all scope, owner, session and proof-work checks are unchanged.
    pub fn get_cached_with<R>(
        &self,
        filesystem: &mut F,
        tree: &CanonicalPackedTree,
        key: &[u8],
        limits: TreeLookupLimits,
        cache: &mut PackedPageCache,
        map: impl FnOnce(&[u8]) -> R,
    ) -> Result<uste_storage::packed_tree_lookup::MappedTreeLookupResult<R>, TransactionError> {
        self.check(tree.context())?;
        self.journal
            .packed_tree_get_cached_with(filesystem, tree, key, limits, cache, map)
            .map_err(TransactionError::Storage)
    }
    pub fn next_cached(
        &self,
        filesystem: &mut F,
        cursor: &mut ScopedPackedCursor,
        cache: &mut PackedPageCache,
    ) -> Result<Option<PackedCursorEntry>, TransactionError> {
        if cursor.failed {
            return Err(TransactionError::Storage(StorageError::NeedsRecovery));
        }
        let result = self.check(cursor.inner.context()).and_then(|()| {
            self.journal
                .next_packed_tree_entry_cached(filesystem, &mut cursor.inner, cache)
                .map_err(TransactionError::Storage)
        });
        if result.is_err() {
            cursor.failed = true;
        }
        result
    }
    /// Complete ascending `[lower, upper)` or descending `(lower, upper]` traversal with an
    /// optional independently budgeted range-result partition in `cache`.
    #[allow(clippy::too_many_arguments)]
    pub fn range_cached(
        &self,
        filesystem: &mut F,
        tree: &CanonicalPackedTree,
        lower: &[u8],
        upper: Option<&[u8]>,
        limits: TreeCursorLimits,
        reverse: bool,
        cache: &mut PackedPageCache,
    ) -> Result<uste_storage::journal::PackedTreeRangeResult, TransactionError> {
        self.check(tree.context())?;
        self.journal
            .packed_tree_range_cached(filesystem, tree, lower, upper, limits, reverse, cache)
            .map_err(TransactionError::Storage)
    }
    /// Map a complete cached range while borrowing its authenticated resident plaintext. The
    /// mapped values are owned, and all scope, owner, session and proof-work checks are unchanged.
    #[allow(clippy::too_many_arguments)]
    pub fn range_cached_with<T>(
        &self,
        filesystem: &mut F,
        tree: &CanonicalPackedTree,
        lower: &[u8],
        upper: Option<&[u8]>,
        limits: TreeCursorLimits,
        reverse: bool,
        cache: &mut PackedPageCache,
        map: impl FnMut(&[u8], &[u8]) -> T,
    ) -> Result<uste_storage::journal::MappedPackedTreeRangeResult<T>, TransactionError> {
        self.check(tree.context())?;
        self.journal
            .packed_tree_range_cached_with(
                filesystem, tree, lower, upper, limits, reverse, cache, map,
            )
            .map_err(TransactionError::Storage)
    }

    /// Fallible mapped range. The returned values are owned; all binding and proof-work checks
    /// precede mapping exactly as in [`Self::range_cached_with`].
    #[allow(clippy::too_many_arguments)]
    pub fn try_range_cached_with<T, M>(
        &self,
        filesystem: &mut F,
        tree: &CanonicalPackedTree,
        lower: &[u8],
        upper: Option<&[u8]>,
        limits: TreeCursorLimits,
        reverse: bool,
        cache: &mut PackedPageCache,
        map: impl FnMut(&[u8], &[u8]) -> Result<T, M>,
    ) -> Result<uste_storage::journal::MappedPackedTreeRangeResult<T>, M>
    where
        M: From<StorageError> + From<TransactionError>,
    {
        self.check(tree.context()).map_err(M::from)?;
        self.journal.try_packed_tree_range_cached_with(
            filesystem, tree, lower, upper, limits, reverse, cache, map,
        )
    }
}
