//! Temporary authenticated journal owner for pre-coordinator derived-root recovery.

use uste_crypto::{EntropySource, KeyAdapter};
use uste_storage::{
    BlobInventory, EntryName, IndexEntry, IndexGetLimits, IndexPredecessor, IndexPredecessorLimits,
    IndexReadStats, IndexRunCursor, IndexRunReadLimits, IndexRunReadReport, IndexRunVisitor,
    IndexScan, OwnershipFileSystem, PageCache, RecoveredIndexRoot,
    journal::{DurableKeyEnvelope, JournalStore, RecoveredGroup, RecoveryReport, StorageError},
};
use uste_types::{CommitRevision, IdempotencyKey, NamespaceRef};

use super::{
    PrincipalDigest, TransactionError, TransactionOutcome, decode_group, map_open_error, sha256,
};

/// Opaque owned copy of the authenticated journal frontier transaction.
///
/// Only one bounded canonical request and inventory are retained. Construction is possible only
/// while the storage journal is fully authenticated.
pub struct RecoveredFrontierTransaction {
    pub(crate) revision: CommitRevision,
    pub(crate) certificate_digest: [u8; 32],
    pub(crate) logical_event_digest: [u8; 32],
    pub(crate) blob_inventory_digest: [u8; 32],
    pub(crate) principal: PrincipalDigest,
    pub(crate) idempotency_key: IdempotencyKey,
    pub(crate) outcome: TransactionOutcome,
    pub(crate) canonical_request: Vec<u8>,
    pub(crate) blob_inventory: Option<BlobInventory>,
}

impl core::fmt::Debug for RecoveredFrontierTransaction {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("RecoveredFrontierTransaction")
            .field("revision", &self.revision)
            .field("certificate_digest", &"[REDACTED]")
            .field("canonical_request", &"[REDACTED]")
            .field("has_blob_inventory", &self.blob_inventory.is_some())
            .finish_non_exhaustive()
    }
}

impl RecoveredFrontierTransaction {
    #[must_use]
    pub const fn revision(&self) -> CommitRevision {
        self.revision
    }

    #[must_use]
    pub const fn certificate_digest(&self) -> &[u8; 32] {
        &self.certificate_digest
    }

    #[must_use]
    pub fn canonical_request(&self) -> &[u8] {
        &self.canonical_request
    }

    #[must_use]
    pub const fn blob_inventory(&self) -> Option<&BlobInventory> {
        self.blob_inventory.as_ref()
    }

    #[must_use]
    pub const fn outcome(&self) -> TransactionOutcome {
        self.outcome
    }

    /// Bind a domain-prepared suffix candidate for independent journal reopen validation.
    #[must_use]
    pub fn bind_prepared<P>(self, prepared: P) -> RecoveredPreparedSuffix<P> {
        RecoveredPreparedSuffix {
            transaction: self,
            prepared,
        }
    }

    pub(crate) fn matches(
        &self,
        group: RecoveredGroup<'_>,
        decoded: &super::DecodedGroup<'_>,
    ) -> bool {
        self.revision == group.revision
            && self.certificate_digest == group.certificate_digest
            && self.logical_event_digest == group.logical_event_digest
            && self.blob_inventory_digest == group.blob_inventory_digest
            && self.principal == decoded.retry_key.principal
            && self.idempotency_key == decoded.retry_key.key
            && self.outcome == decoded.outcome
            && self.canonical_request == decoded.request
            && self.blob_inventory.as_ref() == decoded.blob_inventory
    }
}

/// Frontier transaction plus a domain-prepared change. Neither is authoritative until final open.
pub struct RecoveredPreparedSuffix<P> {
    pub(crate) transaction: RecoveredFrontierTransaction,
    pub(crate) prepared: P,
}

impl<P> core::fmt::Debug for RecoveredPreparedSuffix<P> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("RecoveredPreparedSuffix")
            .field("transaction", &self.transaction)
            .field("prepared", &"[REDACTED]")
            .finish()
    }
}

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

    /// Authenticate the complete journal while retaining only its final decoded transaction.
    #[allow(clippy::too_many_arguments)]
    pub fn open_with_frontier_transaction<A>(
        filesystem: &mut F,
        final_name: &EntryName,
        scope: NamespaceRef,
        vault_entropy: E,
        identity_entropy: I,
        key_adapter: &mut A,
    ) -> Result<(Self, RecoveryReport, Option<RecoveredFrontierTransaction>), TransactionError>
    where
        A: KeyAdapter<Envelope = W>,
    {
        let mut frontier = None;
        let (journal, report) = JournalStore::open(
            filesystem,
            final_name,
            scope.database(),
            vault_entropy,
            identity_entropy,
            key_adapter,
            |group| {
                frontier = Some(capture_group(scope, group)?);
                Ok(())
            },
        )
        .map_err(map_open_error)?;
        Ok((Self { scope, journal }, report, frontier))
    }

    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.scope
    }

    /// Stream canonical transactions from an authenticated inclusive journal range, retaining
    /// one request/inventory at a time. Callbacks may use this recovery owner's explicit-I/O
    /// index APIs. Their results remain provisional until the entire range succeeds.
    ///
    /// The byte budget covers encrypted certificates/groups; inventory sizes have independent
    /// format caps. This does not yet remove storage's certificate and blob metadata maps.
    pub fn visit_transactions<V>(
        &self,
        filesystem: &mut F,
        first: CommitRevision,
        last: CommitRevision,
        maximum_groups: u64,
        maximum_encoded_bytes: u64,
        mut visitor: V,
    ) -> Result<(), TransactionError>
    where
        V: FnMut(&mut F, RecoveredFrontierTransaction) -> Result<(), StorageError>,
    {
        self.journal
            .visit_committed_range(
                filesystem,
                first,
                last,
                maximum_groups,
                maximum_encoded_bytes,
                |filesystem, group| visitor(filesystem, capture_group(self.scope, group)?),
            )
            .map_err(map_open_error)
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

    /// Load certificate-bound root manifests without an implicit absolute-maximum run scrub.
    ///
    /// Returned handles remain provisional until trusted domain code exhausts complete
    /// authenticated cursors with explicit limits and completes semantic admission.
    pub fn load_index_root_manifests(
        &self,
        filesystem: &mut F,
        index_profile: [u8; 32],
    ) -> Result<Vec<RecoveredIndexRoot>, TransactionError> {
        self.journal
            .load_index_root_manifests(filesystem, self.scope, index_profile)
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

    /// Bounded authenticated prefix scan for recovery-domain proof loading.
    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_prefix(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        prefix: &[u8],
        maximum_results: usize,
        maximum_result_bytes: usize,
        cache: &mut PageCache,
    ) -> Result<IndexScan, TransactionError> {
        if root.scope() != self.scope {
            return Err(TransactionError::InvalidRequest);
        }
        self.journal
            .index_scan_prefix(
                filesystem,
                root,
                family,
                prefix,
                maximum_results,
                maximum_result_bytes,
                cache,
            )
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

fn capture_group(
    scope: NamespaceRef,
    group: RecoveredGroup<'_>,
) -> Result<RecoveredFrontierTransaction, StorageError> {
    if sha256(group.encoded_group) != group.logical_event_digest {
        return Err(StorageError::IntegrityFailure);
    }
    let decoded = decode_group(
        scope,
        group.encoded_group,
        group.revision,
        group.blob_inventory_digest,
        group.blob_inventory,
    )
    .map_err(|error| match error {
        TransactionError::ResourceLimit => StorageError::ResourceLimit,
        _ => StorageError::IntegrityFailure,
    })?;
    let mut canonical_request = Vec::new();
    canonical_request
        .try_reserve_exact(decoded.request.len())
        .map_err(|_| StorageError::ResourceLimit)?;
    canonical_request.extend_from_slice(decoded.request);
    let blob_inventory = decoded
        .blob_inventory
        .map(|inventory| BlobInventory::new(scope, inventory.references().iter().copied()))
        .transpose()?;
    Ok(RecoveredFrontierTransaction {
        revision: group.revision,
        certificate_digest: group.certificate_digest,
        logical_event_digest: group.logical_event_digest,
        blob_inventory_digest: group.blob_inventory_digest,
        principal: decoded.retry_key.principal,
        idempotency_key: decoded.retry_key.key,
        outcome: decoded.outcome,
        canonical_request,
        blob_inventory,
    })
}
