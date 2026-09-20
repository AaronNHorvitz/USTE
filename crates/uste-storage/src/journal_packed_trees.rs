use super::*;
#[path = "journal_packed_trees/cache.rs"]
mod cache;
use crate::{
    ordered_commitment::{self, CommitmentContext, OrderedCommitment},
    packed_index_pack::ImmutablePack,
    packed_index_page::PackedPageContext,
    packed_root_manifest::PackedRootFamily,
    packed_tree_batch::{self, StagedTreeBatch, TreeBatchLimits, TreeBatchReport},
    packed_tree_cursor::{PackedCursorEntry, PackedTreeCursor, TreeCursorLimits, TreeCursorReport},
    packed_tree_lookup::{self, TreeLookupLimits, TreeLookupResult, TreeReadContext},
    packed_tree_record::PackedLocator,
    packed_tree_validation::{self, TreeValidationLimits, TreeValidationReport},
};

/// Canonical content plus live-owner certificate binding, not domain or consumer authority.
#[derive(Clone)]
pub struct CanonicalPackedTree {
    context: TreeReadContext,
    commitment: OrderedCommitment,
    root: Option<PackedLocator>,
    proof: CertificateAnchorProof,
}
impl CanonicalPackedTree {
    pub fn context(&self) -> TreeReadContext {
        self.context
    }
    pub fn family_descriptor(&self) -> PackedRootFamily {
        PackedRootFamily {
            family: self.context.family,
            commitment: self.commitment,
            root: self.root,
        }
    }
}
pub struct CertifiedPackedTreeStage {
    tree: CanonicalPackedTree,
    staged: StagedTreeBatch,
}
impl CertifiedPackedTreeStage {
    pub fn tree(&self) -> &CanonicalPackedTree {
        &self.tree
    }
    pub fn report(&self) -> TreeBatchReport {
        self.staged.report()
    }
    pub fn pack(&self) -> Option<ImmutablePack> {
        self.staged.pack()
    }
}
pub struct CertifiedPackedTreeCursor {
    cursor: PackedTreeCursor,
    proof: CertificateAnchorProof,
    context: TreeReadContext,
    failed: bool,
}
impl CertifiedPackedTreeCursor {
    pub fn context(&self) -> TreeReadContext {
        self.context
    }
    pub fn report(&self) -> TreeCursorReport {
        self.cursor.report()
    }
}

impl<F, W, E, I> JournalStore<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn validate_canonical_packed_tree(
        &self,
        tree: &CanonicalPackedTree,
    ) -> Result<(), StorageError> {
        self.validate_historical_certificate_proof(&tree.proof)?;
        self.require_packed_tree_key()?;
        if tree.context.scope.database() != self.database
            || tree.context.revision != tree.proof.anchor().0
        {
            return Err(StorageError::InvalidState);
        }
        Ok(())
    }
    fn require_packed_tree_key(&self) -> Result<(), StorageError> {
        if self.vault.is_locked() {
            return Err(StorageError::Crypto(CryptoError::Locked));
        }
        Ok(())
    }
    /// Recheck a retained capability's live certificate owner and unlocked key without I/O.
    /// This does not reread ciphertext or establish domain correctness/current consumer access.
    pub fn validate_packed_tree_binding(
        &self,
        tree: &CanonicalPackedTree,
    ) -> Result<(), StorageError> {
        self.validate_canonical_packed_tree(tree)
    }
    /// Full canonical/content admission of one explicit manifest family. Not domain validation.
    pub fn admit_packed_tree(
        &self,
        filesystem: &mut F,
        root: &CertifiedPackedRoot,
        family: u8,
        limits: TreeValidationLimits,
    ) -> Result<(CanonicalPackedTree, TreeValidationReport), StorageError> {
        self.admit_packed_tree_inner(filesystem, root, family, limits, None)
    }
    /// Cold canonical admission with a fresh bounded operation-local plaintext cache.
    /// Never reuses a previous admission's pages. Proof-work limits include cache hits.
    /// The cache is dropped on success or failure; no domain or consumer authority is added.
    pub fn admit_packed_tree_buffered(
        &self,
        filesystem: &mut F,
        root: &CertifiedPackedRoot,
        family: u8,
        limits: TreeValidationLimits,
        cache_bytes: usize,
    ) -> Result<
        (
            CanonicalPackedTree,
            TreeValidationReport,
            crate::packed_page_cache::PackedCacheReport,
        ),
        StorageError,
    > {
        self.validate_packed_root_certificate(root)?;
        self.require_packed_tree_key()?;
        let mut cache = crate::packed_page_cache::PackedPageCache::new(cache_bytes)?;
        cache.bind(root.certificate_proof(), self.vault.unlocked_session()?)?;
        let (tree, report) =
            self.admit_packed_tree_inner(filesystem, root, family, limits, Some(&mut cache))?;
        Ok((tree, report, cache.report()?))
    }
    fn admit_packed_tree_inner(
        &self,
        filesystem: &mut F,
        root: &CertifiedPackedRoot,
        family: u8,
        limits: TreeValidationLimits,
        cache: Option<&mut crate::packed_page_cache::PackedPageCache>,
    ) -> Result<(CanonicalPackedTree, TreeValidationReport), StorageError> {
        self.validate_packed_root_certificate(root)?;
        self.require_packed_tree_key()?;
        let manifest = root.manifest();
        let descriptor = manifest
            .families()
            .iter()
            .find(|entry| entry.family == family)
            .ok_or(StorageError::InvalidState)?;
        let context = TreeReadContext {
            scope: manifest.context().scope,
            profile: manifest.context().profile,
            family,
            revision: manifest.claims().revision,
        };
        let validated = packed_tree_validation::validate_tree_buffered(
            filesystem,
            &self.database_directory,
            &self.vault,
            context,
            descriptor.commitment,
            descriptor.root,
            limits,
            cache,
        )?;
        Ok((
            CanonicalPackedTree {
                context,
                commitment: descriptor.commitment,
                root: descriptor.root,
                proof: root.certificate_proof().clone(),
            },
            validated.report(),
        ))
    }
    pub fn packed_tree_get(
        &self,
        filesystem: &mut F,
        tree: &CanonicalPackedTree,
        key: &[u8],
        limits: TreeLookupLimits,
    ) -> Result<TreeLookupResult, StorageError> {
        self.validate_canonical_packed_tree(tree)?;
        packed_tree_lookup::lookup(
            filesystem,
            &self.database_directory,
            &self.vault,
            tree.context,
            tree.commitment,
            tree.root,
            key,
            limits,
        )
    }
    pub fn open_packed_tree_cursor(
        &self,
        tree: &CanonicalPackedTree,
        lower: &[u8],
        upper: Option<&[u8]>,
        limits: TreeCursorLimits,
    ) -> Result<CertifiedPackedTreeCursor, StorageError> {
        self.open_directional_packed_tree_cursor(tree, lower, upper, limits, false)
    }
    /// Owner-bound descending `(lower, upper]` traversal of an admitted canonical tree.
    pub fn open_reverse_packed_tree_cursor(
        &self,
        tree: &CanonicalPackedTree,
        lower: &[u8],
        upper: Option<&[u8]>,
        limits: TreeCursorLimits,
    ) -> Result<CertifiedPackedTreeCursor, StorageError> {
        self.open_directional_packed_tree_cursor(tree, lower, upper, limits, true)
    }
    fn open_directional_packed_tree_cursor(
        &self,
        tree: &CanonicalPackedTree,
        lower: &[u8],
        upper: Option<&[u8]>,
        limits: TreeCursorLimits,
        reverse: bool,
    ) -> Result<CertifiedPackedTreeCursor, StorageError> {
        self.validate_canonical_packed_tree(tree)?;
        let constructor = if reverse {
            PackedTreeCursor::new_reverse
        } else {
            PackedTreeCursor::new
        };
        let cursor = constructor(
            tree.context,
            tree.commitment,
            tree.root,
            lower,
            upper,
            limits,
        )?;
        Ok(CertifiedPackedTreeCursor {
            cursor,
            proof: tree.proof.clone(),
            context: tree.context,
            failed: false,
        })
    }
    pub fn next_packed_tree_entry(
        &self,
        filesystem: &mut F,
        cursor: &mut CertifiedPackedTreeCursor,
    ) -> Result<Option<PackedCursorEntry>, StorageError> {
        if cursor.failed {
            return Err(StorageError::NeedsRecovery);
        }
        let result = self
            .validate_historical_certificate_proof(&cursor.proof)
            .and_then(|()| self.require_packed_tree_key())
            .and_then(|()| {
                cursor
                    .cursor
                    .next(filesystem, &self.database_directory, &self.vault)
            });
        if result.is_err() {
            cursor.failed = true;
        }
        result
    }
    /// Private, bounded cache construction at an authenticated target; never publishes a root.
    /// The base's canonical capability may come from a prior private stage under this owner.
    #[allow(clippy::too_many_arguments)]
    pub fn stage_packed_tree_batch_proven(
        &mut self,
        filesystem: &mut F,
        scope: NamespaceRef,
        profile: [u8; 32],
        family: u8,
        target: &CertificateAnchorProof,
        base: Option<&CanonicalPackedTree>,
        deltas: &[IndexDelta],
        limits: TreeBatchLimits,
    ) -> Result<CertifiedPackedTreeStage, StorageError> {
        self.validate_certificate_anchor_proof(target)?;
        self.require_packed_tree_key()?;
        if scope.database() != self.database || family == 0 {
            return Err(StorageError::InvalidState);
        }
        let revision = target.anchor().0;
        let (context, commitment, root) = if let Some(base) = base {
            self.validate_canonical_packed_tree(base)?;
            if base.context.scope != scope
                || base.context.profile != profile
                || base.context.family != family
                || base.context.revision > revision
            {
                return Err(StorageError::InvalidState);
            }
            (base.context, base.commitment, base.root)
        } else {
            let context = TreeReadContext {
                scope,
                profile,
                family,
                revision,
            };
            let logical = CommitmentContext::new(scope, profile, family)
                .map_err(|_| StorageError::InvalidState)?;
            (context, ordered_commitment::empty_commitment(logical), None)
        };
        let staged = packed_tree_batch::stage_batch(
            filesystem,
            &self.database_directory,
            &mut self.vault,
            &mut self.identity_entropy,
            context,
            commitment,
            root,
            deltas,
            PackedPageContext {
                scope,
                profile,
                family,
                creation_revision: revision,
                epoch: self.epoch,
                writer: self.writer,
                object: [0; 16],
                page: 0,
            },
            limits,
        )?;
        let tree = CanonicalPackedTree {
            context: staged.context(),
            commitment: staged.logical_root(),
            root: staged.root().map(|entry| entry.location),
            proof: target.clone(),
        };
        Ok(CertifiedPackedTreeStage { tree, staged })
    }
}
