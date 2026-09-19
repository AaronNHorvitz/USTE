//! Shared admission dispatch; packed and run-backed metadata obey the same commit state machine.
use super::*;
use uste_storage::packed_tree_lookup::TreeLookupLimits;

#[derive(Clone, Copy)]
pub(crate) enum DiskMetadataBase<'a> {
    Runs(&'a CoordinatorDiskBase, IndexGetLimits),
    Packed(&'a PackedCoordinatorPrefix, TreeLookupLimits),
}
impl DiskMetadataBase<'_> {
    pub(crate) fn revision(&self) -> CommitRevision {
        match self {
            Self::Runs(base, _) => base.metadata.revision(),
            Self::Packed(base, _) => base.anchor().0,
        }
    }
    pub(crate) fn retry_from_journal<F, W, E, I>(
        &self,
        journal: &JournalStore<F, W, E, I>,
        fs: &mut F,
        principal: PrincipalDigest,
        key: IdempotencyKey,
        cache: &mut PageCache,
    ) -> Result<Option<TransactionOutcome>, TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        match self {
            Self::Runs(base, limits) => {
                base.retry_from_journal(journal, fs, principal, key, *limits, cache)
            }
            Self::Packed(base, limits) => {
                base.retry_from_journal(journal, fs, principal, key, *limits)
            }
        }
    }
    pub(crate) fn transaction_from_journal<F, W, E, I>(
        &self,
        journal: &JournalStore<F, W, E, I>,
        fs: &mut F,
        id: TransactionId,
        cache: &mut PageCache,
    ) -> Result<Option<(PrincipalDigest, TransactionOutcome)>, TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        match self {
            Self::Runs(base, limits) => {
                base.transaction_from_journal(journal, fs, id, *limits, cache)
            }
            Self::Packed(base, limits) => base.transaction_from_journal(journal, fs, id, *limits),
        }
    }
    pub(crate) fn owner_from_journal<F, W, E, I>(
        &self,
        journal: &JournalStore<F, W, E, I>,
        fs: &mut F,
        id: BlobId,
        cache: &mut PageCache,
    ) -> Result<Option<(BlobReference, PrincipalDigest)>, TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        match self {
            Self::Runs(base, limits) => base.owner_from_journal(journal, fs, id, *limits, cache),
            Self::Packed(base, limits) => base.owner_from_journal(journal, fs, id, *limits),
        }
    }
}
