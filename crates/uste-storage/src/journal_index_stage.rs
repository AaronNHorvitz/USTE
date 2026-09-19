use super::*;
use crate::StagedIndexRoot;

/// Trusted recovery staging at an already certified revision, pinned to the open frontier.
/// This capability cannot publish a root slot, append a transaction or authorize a consumer.
pub struct IndexRecoveryStage {
    scope: NamespaceRef,
    revision: CommitRevision,
    certificate_digest: [u8; 32],
    frontier: (CommitRevision, [u8; 32]),
}

impl fmt::Debug for IndexRecoveryStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IndexRecoveryStage")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

impl<F, W, E, I> JournalStore<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Admit recovery scratch work without I/O. The caller still must authenticate selected
    /// transactions and validate exact domain outcomes before exposing recovered state.
    pub fn open_index_recovery_stage(
        &self,
        scope: NamespaceRef,
        revision: CommitRevision,
        certificate_digest: [u8; 32],
    ) -> Result<IndexRecoveryStage, StorageError> {
        let frontier = self.checkpoint_anchor().ok_or(StorageError::InvalidState)?;
        if self.poisoned
            || scope.database() != self.database
            || revision > frontier.0
            || self.certificate_anchors.get(&revision) != Some(&certificate_digest)
        {
            return Err(StorageError::InvalidState);
        }
        Ok(IndexRecoveryStage {
            scope,
            revision,
            certificate_digest,
            frontier,
        })
    }

    fn validate_recovery_stage(&self, stage: &IndexRecoveryStage) -> Result<(), StorageError> {
        if self.poisoned
            || stage.scope.database() != self.database
            || self.checkpoint_anchor() != Some(stage.frontier)
            || self.certificate_anchors.get(&stage.revision) != Some(&stage.certificate_digest)
        {
            return Err(StorageError::InvalidState);
        }
        Ok(())
    }

    /// Build an invisible run at the stage's certified revision. Intermediate historical roots
    /// never replace durable root slots. All visitor effects remain provisional on failure.
    #[allow(clippy::too_many_arguments)]
    pub fn stage_index_merge_visit<T>(
        &mut self,
        filesystem: &mut F,
        stage: &IndexRecoveryStage,
        index_profile: [u8; 32],
        family: u8,
        base_root: Option<&RecoveredIndexRoot>,
        limits: IndexRunMergeLimits,
        deltas: T,
        visitor: &mut IndexRunVisitor<'_>,
    ) -> Result<MergedIndexRun, StorageError>
    where
        T: IntoIterator<Item = Result<IndexDelta, StorageError>>,
    {
        self.validate_recovery_stage(stage)?;
        if let Some(root) = base_root {
            self.validate_index_cursor_root(root)?;
            if root.scope() != stage.scope || root.revision() >= stage.revision {
                return Err(StorageError::InvalidState);
            }
        }
        index::merge_run_visit(
            filesystem,
            &IndexContext {
                database: self.database,
                epoch: self.epoch,
                writer: self.writer,
                directory: &self.database_directory,
            },
            &mut self.vault,
            &mut self.identity_entropy,
            stage.scope,
            stage.revision,
            index_profile,
            family,
            base_root,
            limits,
            deltas,
            visitor,
        )
    }

    /// Assemble a bounded private read handle after the caller's terminal semantic validation.
    /// No filesystem access or root publication occurs. This is not a durable receipt.
    pub fn finish_index_recovery_stage(
        &self,
        stage: IndexRecoveryStage,
        input: IndexRootInput,
        runs: &[IndexRunDescriptor],
    ) -> Result<StagedIndexRoot, StorageError> {
        self.validate_recovery_stage(&stage)?;
        if input.scope != stage.scope
            || input.revision != stage.revision
            || input.certificate_digest != stage.certificate_digest
        {
            return Err(StorageError::InvalidState);
        }
        index::stage_root(
            &IndexContext {
                database: self.database,
                epoch: self.epoch,
                writer: self.writer,
                directory: &self.database_directory,
            },
            input,
            runs,
        )
    }
}
