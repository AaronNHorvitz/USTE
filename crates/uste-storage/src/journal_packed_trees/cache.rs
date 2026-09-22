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

    /// Map a complete authenticated range while borrowing each resident key/value. Returned
    /// values cannot borrow from the cache. A hit is admitted against every logical cursor limit
    /// before the mapper observes plaintext; misses retain the same authenticated result first.
    #[allow(clippy::too_many_arguments)]
    pub fn packed_tree_range_cached_with<T>(
        &self,
        filesystem: &mut F,
        tree: &CanonicalPackedTree,
        lower: &[u8],
        upper: Option<&[u8]>,
        limits: TreeCursorLimits,
        reverse: bool,
        cache: &mut PackedPageCache,
        mut map: impl FnMut(&[u8], &[u8]) -> T,
    ) -> Result<MappedPackedTreeRangeResult<T>, StorageError> {
        self.try_packed_tree_range_cached_with(
            filesystem,
            tree,
            lower,
            upper,
            limits,
            reverse,
            cache,
            |key, value| Ok(map(key, value)),
        )
    }

    /// Fallible form of [`Self::packed_tree_range_cached_with`]. Mapper failures are returned only
    /// after every admitted entry is inspected; cached plaintext never escapes the call.
    #[allow(clippy::too_many_arguments)]
    pub fn try_packed_tree_range_cached_with<T, M>(
        &self,
        filesystem: &mut F,
        tree: &CanonicalPackedTree,
        lower: &[u8],
        upper: Option<&[u8]>,
        limits: TreeCursorLimits,
        reverse: bool,
        cache: &mut PackedPageCache,
        mut map: impl FnMut(&[u8], &[u8]) -> Result<T, M>,
    ) -> Result<MappedPackedTreeRangeResult<T>, M>
    where
        M: From<StorageError>,
    {
        if let Err(error) = self.validate_canonical_packed_tree(tree) {
            if self.vault.is_locked() {
                cache.clear();
            }
            return Err(M::from(error));
        }
        PackedTreeCursor::validate_request(
            tree.context,
            tree.commitment,
            tree.root,
            lower,
            upper,
            limits,
        )
        .map_err(M::from)?;
        let session = self
            .vault
            .unlocked_session()
            .map_err(StorageError::from)
            .map_err(M::from)?;
        cache.bind(&tree.proof, session).map_err(M::from)?;
        let identity = tree.root.map(|root| {
            crate::packed_page_cache::LookupIdentity::new(tree.context, root, tree.commitment)
        });
        if let Some(identity) = identity
            && let Some(hit) = cache.range_get_with(
                &self.vault,
                identity,
                lower,
                upper,
                reverse,
                limits,
                &mut map,
            )?
        {
            return Ok(MappedPackedTreeRangeResult {
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
        )
        .map_err(M::from)?;
        let mut entries = Vec::new();
        while let Some(entry) = cursor
            .next_cached(filesystem, &self.database_directory, &self.vault, cache)
            .map_err(M::from)?
        {
            entries
                .try_reserve(1)
                .map_err(|_| M::from(StorageError::ResourceLimit))?;
            entries.push(entry);
        }
        let report = cursor.report();
        if let Some(identity) = identity {
            cache
                .range_insert(
                    &self.vault,
                    identity,
                    lower,
                    upper,
                    reverse,
                    &entries,
                    report,
                )
                .map_err(M::from)?;
        }
        let mut mapped = Vec::new();
        mapped
            .try_reserve_exact(entries.len())
            .map_err(|_| StorageError::ResourceLimit)?;
        let mut map_error = None;
        for entry in &entries {
            match map(entry.key(), entry.value()) {
                Ok(value) if map_error.is_none() => mapped.push(value),
                Ok(_) => {}
                Err(error) if map_error.is_none() => map_error = Some(error),
                Err(_) => {}
            }
        }
        if let Some(error) = map_error {
            return Err(error);
        }
        Ok(MappedPackedTreeRangeResult {
            entries: mapped,
            report,
        })
    }
}
