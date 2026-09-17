//! Temporary authenticated journal owner for pre-coordinator derived-root recovery.

use uste_crypto::{EntropySource, KeyAdapter};
use uste_storage::{
    EntryName, IndexRunReadLimits, IndexRunReadReport, IndexRunVisitor, OwnershipFileSystem,
    RecoveredIndexRoot,
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
}
