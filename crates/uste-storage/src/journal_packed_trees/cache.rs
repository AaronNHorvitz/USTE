use super::*;
use crate::packed_page_cache::PackedPageCache;

impl<F, W, E, I> JournalStore<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    pub fn packed_tree_get_cached(
        &self,
        filesystem: &mut F,
        tree: &CanonicalPackedTree,
        key: &[u8],
        limits: TreeLookupLimits,
        cache: &mut PackedPageCache,
    ) -> Result<TreeLookupResult, StorageError> {
        if let Err(error) = self.validate_canonical_packed_tree(tree) {
            if self.vault.is_locked() {
                cache.clear();
            }
            return Err(error);
        }
        cache.bind(&tree.proof, self.vault.unlocked_session()?)?;
        packed_tree_lookup::lookup_cached(
            filesystem,
            &self.database_directory,
            &self.vault,
            tree.context,
            tree.commitment,
            tree.root,
            key,
            limits,
            cache,
        )
    }
    pub fn next_packed_tree_entry_cached(
        &self,
        filesystem: &mut F,
        cursor: &mut CertifiedPackedTreeCursor,
        cache: &mut PackedPageCache,
    ) -> Result<Option<PackedCursorEntry>, StorageError> {
        if cursor.failed {
            return Err(StorageError::NeedsRecovery);
        }
        let result = self
            .validate_historical_certificate_proof(&cursor.proof)
            .and_then(|()| self.require_packed_tree_key())
            .and_then(|()| {
                cache.bind(&cursor.proof, self.vault.unlocked_session()?)?;
                cursor
                    .cursor
                    .next_cached(filesystem, &self.database_directory, &self.vault, cache)
            });
        if result.is_err() {
            if self.vault.is_locked() {
                cache.clear();
            }
            cursor.failed = true;
        }
        result
    }
}
