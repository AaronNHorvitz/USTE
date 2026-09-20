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
}
