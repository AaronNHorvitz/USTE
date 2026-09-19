//! Trusted read access only: no cold admission, publication or caller authorization.
use super::*;

pub struct PackedIndexReader<'a, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    scope: NamespaceRef,
    journal: &'a JournalStore<F, W, E, I>,
    anchor: (CommitRevision, [u8; 32]),
}

impl<'a, F, W, E, I> PackedIndexReader<'a, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    pub(crate) fn new(
        scope: NamespaceRef,
        journal: &'a JournalStore<F, W, E, I>,
        anchor: (CommitRevision, [u8; 32]),
    ) -> Self {
        Self {
            scope,
            journal,
            anchor,
        }
    }
    pub fn anchor(&self) -> (CommitRevision, [u8; 32]) {
        self.anchor
    }
    fn check(&self, context: TreeReadContext) -> Result<(), TransactionError> {
        if context.scope != self.scope || context.revision > self.anchor.0 {
            return Err(TransactionError::InvalidRequest);
        }
        Ok(())
    }
    /// Recheck scope, revision, live owner and available keys, without reading disk.
    pub fn validate_tree_binding(
        &self,
        tree: &CanonicalPackedTree,
    ) -> Result<(), TransactionError> {
        self.check(tree.context())?;
        self.journal
            .validate_packed_tree_binding(tree)
            .map_err(TransactionError::Storage)
    }
    pub fn get(
        &self,
        filesystem: &mut F,
        tree: &CanonicalPackedTree,
        key: &[u8],
        limits: TreeLookupLimits,
    ) -> Result<TreeLookupResult, TransactionError> {
        self.check(tree.context())?;
        self.journal
            .packed_tree_get(filesystem, tree, key, limits)
            .map_err(TransactionError::Storage)
    }
    /// Ascending `[lower, upper)` traversal.
    pub fn cursor(
        &self,
        tree: &CanonicalPackedTree,
        lower: &[u8],
        upper: Option<&[u8]>,
        limits: TreeCursorLimits,
    ) -> Result<ScopedPackedCursor, TransactionError> {
        self.check(tree.context())?;
        let inner = self
            .journal
            .open_packed_tree_cursor(tree, lower, upper, limits)
            .map_err(TransactionError::Storage)?;
        Ok(ScopedPackedCursor {
            inner,
            failed: false,
        })
    }
    /// Descending `(lower, upper]` traversal.
    pub fn reverse_cursor(
        &self,
        tree: &CanonicalPackedTree,
        lower: &[u8],
        upper: Option<&[u8]>,
        limits: TreeCursorLimits,
    ) -> Result<ScopedPackedCursor, TransactionError> {
        self.check(tree.context())?;
        let inner = self
            .journal
            .open_reverse_packed_tree_cursor(tree, lower, upper, limits)
            .map_err(TransactionError::Storage)?;
        Ok(ScopedPackedCursor {
            inner,
            failed: false,
        })
    }
    pub fn next(
        &self,
        filesystem: &mut F,
        cursor: &mut ScopedPackedCursor,
    ) -> Result<Option<PackedCursorEntry>, TransactionError> {
        if cursor.failed {
            return Err(TransactionError::Storage(StorageError::NeedsRecovery));
        }
        let result = self.check(cursor.inner.context()).and_then(|()| {
            self.journal
                .next_packed_tree_entry(filesystem, &mut cursor.inner)
                .map_err(TransactionError::Storage)
        });
        if result.is_err() {
            cursor.failed = true;
        }
        result
    }
}
