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
    pub fn packed_tree_get_cached_with<R>(
        &self,
        filesystem: &mut F,
        tree: &CanonicalPackedTree,
        key: &[u8],
        limits: TreeLookupLimits,
        cache: &mut PackedPageCache,
        map: impl FnOnce(&[u8]) -> R,
    ) -> Result<packed_tree_lookup::MappedTreeLookupResult<R>, StorageError> {
        if let Err(error) = self.validate_canonical_packed_tree(tree) {
            if self.vault.is_locked() {
                cache.clear();
            }
            return Err(error);
        }
        cache.bind(&tree.proof, self.vault.unlocked_session()?)?;
        packed_tree_lookup::lookup_cached_with(
            filesystem,
            &self.database_directory,
            &self.vault,
            tree.context,
            tree.commitment,
            tree.root,
            key,
            limits,
            cache,
            map,
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

    /// Collect one complete authenticated range and optionally retain the successful result.
    /// A cache hit is admitted against every logical cursor limit before any entry is returned.
    #[allow(clippy::too_many_arguments)]
    pub fn packed_tree_range_cached(
        &self,
        filesystem: &mut F,
        tree: &CanonicalPackedTree,
        lower: &[u8],
        upper: Option<&[u8]>,
        limits: TreeCursorLimits,
        reverse: bool,
        cache: &mut PackedPageCache,
    ) -> Result<PackedTreeRangeResult, StorageError> {
        if let Err(error) = self.validate_canonical_packed_tree(tree) {
            if self.vault.is_locked() {
                cache.clear();
            }
            return Err(error);
        }
        PackedTreeCursor::validate_request(
            tree.context,
            tree.commitment,
            tree.root,
            lower,
            upper,
            limits,
        )?;
        cache.bind(&tree.proof, self.vault.unlocked_session()?)?;
        let identity = tree.root.map(|root| {
            crate::packed_page_cache::LookupIdentity::new(tree.context, root, tree.commitment)
        });
        if let Some(identity) = identity
            && let Some(hit) =
                cache.range_get(&self.vault, identity, lower, upper, reverse, limits)?
        {
            return Ok(PackedTreeRangeResult {
                entries: hit.entries,
                report: hit.report,
            });
        }
        let constructor = if reverse {
            PackedTreeCursor::new_reverse
        } else {
            PackedTreeCursor::new
        };
        let mut cursor = constructor(
            tree.context,
            tree.commitment,
            tree.root,
            lower,
            upper,
            limits,
        )?;
        let mut entries = Vec::new();
        while let Some(entry) =
            cursor.next_cached(filesystem, &self.database_directory, &self.vault, cache)?
        {
            entries
                .try_reserve(1)
                .map_err(|_| StorageError::ResourceLimit)?;
            entries.push(entry);
        }
        let report = cursor.report();
        if let Some(identity) = identity {
            cache.range_insert(
                &self.vault,
                identity,
                lower,
                upper,
                reverse,
                &entries,
                report,
            )?;
        }
        Ok(PackedTreeRangeResult { entries, report })
    }
}
