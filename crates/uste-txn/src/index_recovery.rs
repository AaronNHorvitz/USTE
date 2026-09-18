//! Temporary authenticated journal owner for pre-coordinator derived-root recovery.

use uste_crypto::{EntropySource, KeyAdapter};
use uste_storage::{
    EntryName, IndexEntry, IndexGetLimits, IndexPredecessor, IndexPredecessorLimits,
    IndexReadStats, IndexRunCursor, IndexRunReadLimits, IndexRunReadReport, IndexRunVisitor,
    OwnershipFileSystem, PageCache, RecoveredIndexRoot,
    journal::{DurableKeyEnvelope, JournalStore, RecoveryReport},
};
use uste_types::NamespaceRef;

use super::{TransactionError, map_open_error};

/// Authenticates the complete storage journal and keeps its exclusive owner/key context alive while
/// optional derived roots are inspected. It does not decode transaction groups or create commit
/// authority. Drop it before `CommitCoordinator::open_seeded`, which reopens and independently
/// validates the transaction prefix and exact seed anchor.
pub struct AuthenticatedIndexRecovery<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    scope: NamespaceRef,
    journal: JournalStore<F, W, E, I>,
}

impl<F, W, E, I> core::fmt::Debug for AuthenticatedIndexRecovery<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AuthenticatedIndexRecovery")
            .field("scope", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl<F, W, E, I> AuthenticatedIndexRecovery<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    #[allow(clippy::too_many_arguments)]
    pub fn open<A>(
        filesystem: &mut F,
        final_name: &EntryName,
        scope: NamespaceRef,
        vault_entropy: E,
        identity_entropy: I,
        key_adapter: &mut A,
    ) -> Result<(Self, RecoveryReport), TransactionError>
    where
        A: KeyAdapter<Envelope = W>,
    {
        let (journal, report) = JournalStore::open(
            filesystem,
            final_name,
            scope.database(),
            vault_entropy,
            identity_entropy,
            key_adapter,
            |_| Ok(()),
        )
        .map_err(map_open_error)?;
        Ok((Self { scope, journal }, report))
    }

    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.scope
    }

    /// Load storage-authenticated derived roots for trusted recovery code.
    pub fn load_index_roots(
        &self,
        filesystem: &mut F,
        index_profile: [u8; 32],
    ) -> Result<Vec<RecoveredIndexRoot>, TransactionError> {
        self.journal
            .load_index_roots(filesystem, self.scope, index_profile)
            .map_err(TransactionError::Storage)
    }

    /// Authenticated exact lookup for trusted recovery semantic proofs.
    pub fn index_get(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        key: &[u8],
        cache: &mut PageCache,
    ) -> Result<(Option<Vec<u8>>, IndexReadStats), TransactionError> {
        if root.scope() != self.scope {
            return Err(TransactionError::InvalidRequest);
        }
        self.journal
            .index_get(filesystem, root, family, key, cache)
            .map_err(TransactionError::Storage)
    }

    /// Authenticated exact lookup with caller-selected page and result bounds.
    #[allow(clippy::too_many_arguments)]
    pub fn index_get_bounded(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        key: &[u8],
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<(Option<Vec<u8>>, IndexReadStats), TransactionError> {
        if root.scope() != self.scope {
            return Err(TransactionError::InvalidRequest);
        }
        self.journal
            .index_get_bounded(filesystem, root, family, key, limits, cache)
            .map_err(TransactionError::Storage)
    }

    /// Authenticated predecessor lookup for trusted recovery semantic proofs.
    #[allow(clippy::too_many_arguments)]
    pub fn index_get_predecessor(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        prefix: &[u8],
        upper_bound: &[u8],
        limits: IndexPredecessorLimits,
        cache: &mut PageCache,
    ) -> Result<IndexPredecessor, TransactionError> {
        if root.scope() != self.scope {
            return Err(TransactionError::InvalidRequest);
        }
        self.journal
            .index_get_predecessor(filesystem, root, family, prefix, upper_bound, limits, cache)
            .map_err(TransactionError::Storage)
    }

    /// Revalidate and visit one complete run. Visitor effects remain provisional until success.
    pub fn visit_index_run(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        limits: IndexRunReadLimits,
        visitor: &mut IndexRunVisitor<'_>,
    ) -> Result<IndexRunReadReport, TransactionError> {
        if root.scope() != self.scope {
            return Err(TransactionError::InvalidRequest);
        }
        self.journal
            .visit_index_run(filesystem, root, family, limits, visitor)
            .map_err(TransactionError::Storage)
    }

    /// Open a resumable authenticated complete-run cursor for trusted recovery code.
    pub fn open_index_run_cursor(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        limits: IndexRunReadLimits,
    ) -> Result<IndexRunCursor<F>, TransactionError> {
        if root.scope() != self.scope {
            return Err(TransactionError::InvalidRequest);
        }
        self.journal
            .open_index_run_cursor(filesystem, root, family, limits)
            .map_err(TransactionError::Storage)
    }

    /// Advance a resumable recovery cursor by one complete entry.
    pub fn next_index_run_entry(
        &self,
        filesystem: &mut F,
        cursor: &mut IndexRunCursor<F>,
    ) -> Result<Option<IndexEntry>, TransactionError> {
        if cursor.scope() != self.scope {
            return Err(TransactionError::InvalidRequest);
        }
        self.journal
            .next_index_run_entry(filesystem, cursor)
            .map_err(TransactionError::Storage)
    }

    /// Consume an exhausted recovery cursor and release its terminal report.
    pub fn finish_index_run_cursor(
        &self,
        cursor: IndexRunCursor<F>,
    ) -> Result<IndexRunReadReport, TransactionError> {
        if cursor.scope() != self.scope {
            return Err(TransactionError::InvalidRequest);
        }
        self.journal
            .finish_index_run_cursor(cursor)
            .map_err(TransactionError::Storage)
    }
}
