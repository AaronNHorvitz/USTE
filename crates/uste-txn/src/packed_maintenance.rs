//! Privileged derived-cache maintenance, never consumer authorization.
use super::*;
use uste_storage::journal::{
    CanonicalPackedTree, CertificateAnchorProof, CertificateAnchorReadLimits, CertifiedPackedRoot,
    CertifiedPackedTreeCursor, CertifiedPackedTreeStage,
};
use uste_storage::packed_tree_batch::TreeBatchLimits;
use uste_storage::packed_tree_cursor::{PackedCursorEntry, TreeCursorLimits, TreeCursorReport};
use uste_storage::packed_tree_lookup::{TreeLookupLimits, TreeLookupResult, TreeReadContext};
use uste_storage::packed_tree_validation::{TreeValidationLimits, TreeValidationReport};

/// Exclusive maintenance pinned to one namespace and authenticated transaction target.
/// It can build private families, but cannot append transactions or publish a root.
pub struct PackedIndexMaintenance<'a, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    scope: NamespaceRef,
    journal: &'a mut JournalStore<F, W, E, I>,
    target: CertificateAnchorProof,
}

pub struct ScopedPackedCursor {
    inner: CertifiedPackedTreeCursor,
    failed: bool,
}
impl ScopedPackedCursor {
    pub fn report(&self) -> TreeCursorReport {
        self.inner.report()
    }
}

impl<F, W, E, I> DerivedIndexMaintenance<'_, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    pub fn packed_indexes(
        &mut self,
        filesystem: &mut F,
        limits: CertificateAnchorReadLimits,
    ) -> Result<PackedIndexMaintenance<'_, F, W, E, I>, TransactionError> {
        let (revision, digest) = self
            .journal
            .checkpoint_anchor()
            .ok_or(TransactionError::InvalidRequest)?;
        let target = self
            .journal
            .authenticate_certificate_anchor(filesystem, revision, digest, limits)
            .map_err(TransactionError::Storage)?;
        PackedIndexMaintenance::new(self.scope, self.journal, target)
    }
}

impl<'a, F, W, E, I> PackedIndexMaintenance<'a, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    pub(crate) fn new(
        scope: NamespaceRef,
        journal: &'a mut JournalStore<F, W, E, I>,
        target: CertificateAnchorProof,
    ) -> Result<Self, TransactionError> {
        journal
            .validate_certificate_anchor_proof(&target)
            .map_err(TransactionError::Storage)?;
        Ok(Self {
            scope,
            journal,
            target,
        })
    }
    pub fn anchor(&self) -> (CommitRevision, [u8; 32]) {
        self.target.anchor()
    }
    fn check(&self, context: TreeReadContext) -> Result<(), TransactionError> {
        if context.scope != self.scope || context.revision > self.target.anchor().0 {
            return Err(TransactionError::InvalidRequest);
        }
        Ok(())
    }
    pub fn admit(
        &self,
        filesystem: &mut F,
        root: &CertifiedPackedRoot,
        family: u8,
        limits: TreeValidationLimits,
    ) -> Result<(CanonicalPackedTree, TreeValidationReport), TransactionError> {
        let manifest = root.manifest();
        self.check(TreeReadContext {
            scope: manifest.context().scope,
            profile: manifest.context().profile,
            family,
            revision: manifest.claims().revision,
        })?;
        self.journal
            .admit_packed_tree(filesystem, root, family, limits)
            .map_err(TransactionError::Storage)
    }
    /// Check scope, target ordering, live certificate ownership and key availability, without I/O.
    pub fn validate_tree_binding(
        &self,
        tree: &CanonicalPackedTree,
    ) -> Result<(), TransactionError> {
        self.check(tree.context())?;
        self.journal
            .validate_packed_tree_binding(tree)
            .map_err(TransactionError::Storage)
    }
    pub fn stage(
        &mut self,
        filesystem: &mut F,
        profile: [u8; 32],
        family: u8,
        base: Option<&CanonicalPackedTree>,
        deltas: &[IndexDelta],
        limits: TreeBatchLimits,
    ) -> Result<CertifiedPackedTreeStage, TransactionError> {
        if let Some(base) = base {
            self.check(base.context())?;
        }
        self.journal
            .stage_packed_tree_batch_proven(
                filesystem,
                self.scope,
                profile,
                family,
                &self.target,
                base,
                deltas,
                limits,
            )
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
    /// Descending `(lower, upper]` traversal; all owner/scope checks match the forward cursor.
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
