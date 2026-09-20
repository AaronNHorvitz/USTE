//! Temporary authenticated journal owner for pre-coordinator derived-root recovery.

use uste_crypto::{EntropySource, KeyAdapter};
use uste_storage::{
    BlobInventory, EntryName, IndexEntry, IndexGetLimits, IndexPredecessor, IndexPredecessorLimits,
    IndexReadStats, IndexRunCursor, IndexRunReadLimits, IndexRunReadReport, IndexRunVisitor,
    IndexScan, OwnershipFileSystem, PageCache, RecoveredIndexRoot,
    journal::{
        DurableKeyEnvelope, JournalRangeReadReport, JournalStore, RecoveredGroup, RecoveryReport,
        StorageError,
    },
};
use uste_types::{CommitRevision, IdempotencyKey, NamespaceRef};

use super::{
    CommitCoordinator, CoordinatorRecoveryLimits, PrincipalDigest, RetentionDays, TransactionError,
    TransactionOutcome, TransactionState, decode_group, map_open_error, sha256,
};

mod maintenance;
mod transaction_reader;
pub use maintenance::RecoveryIndexMaintenance;
pub(crate) use transaction_reader::JournalTransactionReader;

/// Private reconstruction of only the first certified transaction.
/// This is trusted recovery input, not current consumer state or a durable receipt. The caller
/// supplied the genesis reducer; its memory behavior remains that reducer's responsibility.
pub struct RecoveredGenesis<S> {
    state: S,
    transaction: RecoveredFrontierTransaction,
}

impl<S> RecoveredGenesis<S> {
    /// Borrow the reconstructed first-revision reducer for bounded derived-index staging.
    pub fn state(&self) -> &S {
        &self.state
    }

    pub fn transaction(&self) -> &RecoveredFrontierTransaction {
        &self.transaction
    }
}

/// Opaque owned copy of one authenticated journal transaction, including the frontier.
///
/// Only one bounded canonical request and inventory are retained. Construction is possible only
/// while the storage journal is fully authenticated.
pub struct RecoveredFrontierTransaction {
    pub(crate) scope: NamespaceRef,
    pub(crate) revision: CommitRevision,
    pub(crate) certificate_digest: [u8; 32],
    pub(crate) logical_event_digest: [u8; 32],
    pub(crate) blob_inventory_digest: [u8; 32],
    pub(crate) principal: PrincipalDigest,
    pub(crate) idempotency_key: IdempotencyKey,
    pub(crate) outcome: TransactionOutcome,
    pub(crate) canonical_request: Vec<u8>,
    pub(crate) blob_inventory: Option<BlobInventory>,
    // Transient evidence, not transaction content. Never serialized or accepted across owners.
    certificate_proof: Option<uste_storage::journal::CertificateAnchorProof>,
}

impl PartialEq for RecoveredFrontierTransaction {
    fn eq(&self, other: &Self) -> bool {
        self.scope == other.scope
            && self.revision == other.revision
            && self.certificate_digest == other.certificate_digest
            && self.logical_event_digest == other.logical_event_digest
            && self.blob_inventory_digest == other.blob_inventory_digest
            && self.principal == other.principal
            && self.idempotency_key == other.idempotency_key
            && self.outcome == other.outcome
            && self.canonical_request == other.canonical_request
            && self.blob_inventory == other.blob_inventory
    }
}
impl Eq for RecoveredFrontierTransaction {}

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

/// Bounded resumable transaction range pinned to one scope and authenticated journal frontier.
/// The cursor retains no transaction history. Yielded transactions remain provisional until
/// terminal finish; callers must separately validate domain, retry and first-owner semantics.
pub struct TransactionRecoveryCursor {
    scope: NamespaceRef,
    anchor: (CommitRevision, [u8; 32]),
    next: Option<CommitRevision>,
    last: CommitRevision,
    total_groups: u64,
    initial_encoded_bytes: u64,
    remaining_encoded_bytes: u64,
    completed_groups: u64,
    certificate_window_size: u64,
    certificate_proofs: std::vec::IntoIter<uste_storage::journal::CertificateAnchorProof>,
    failed: bool,
}

impl core::fmt::Debug for TransactionRecoveryCursor {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("TransactionRecoveryCursor")
            .field("completed_groups", &self.completed_groups)
            .field("failed", &self.failed)
            .finish_non_exhaustive()
    }
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
/// optional derived roots are inspected or rebuilt privately. It creates no append authority;
/// consuming it into a coordinator requires the selected transaction/domain/metadata validation.
/// The legacy `CommitCoordinator::open_seeded` instead requires dropping this owner before reopen.
pub struct AuthenticatedIndexRecovery<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    scope: NamespaceRef,
    pub(crate) journal: JournalStore<F, W, E, I>,
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

    /// Actual already-authenticated frontier, not a new disk observation or consumer authority.
    pub fn authenticated_frontier_anchor(&self) -> Option<(CommitRevision, [u8; 32])> {
        self.journal.checkpoint_anchor()
    }

    /// Authenticate the full journal and retain its final transaction without a certificate map.
    /// Other storage blob/inventory metadata remains resident and independently bounded.
    #[allow(clippy::too_many_arguments)]
    pub fn open_with_disk_certificate_anchors<A>(
        filesystem: &mut F,
        final_name: &EntryName,
        scope: NamespaceRef,
        vault_entropy: E,
        identity_entropy: I,
        key_adapter: &mut A,
        limits: uste_storage::journal::CertificateAnchorReadLimits,
    ) -> Result<(Self, RecoveryReport, Option<RecoveredFrontierTransaction>), TransactionError>
    where
        A: KeyAdapter<Envelope = W>,
    {
        let mut frontier = None;
        let (journal, report) = JournalStore::open_with_disk_certificate_anchors(
            filesystem,
            final_name,
            scope.database(),
            vault_entropy,
            identity_entropy,
            key_adapter,
            limits,
            |group| {
                frontier = Some(capture_group(scope, group)?);
                Ok(())
            },
        )
        .map_err(map_open_error)?;
        Ok((Self { scope, journal }, report, frontier))
    }

    /// Authenticate storage without resident certificate/blob/inventory/namespace history.
    /// Retains only the final decoded transaction; domain/coordinator admission remains required.
    /// Nonempty inventory append still requires a separate disk-aware storage capability.
    #[allow(clippy::too_many_arguments)]
    pub fn open_with_disk_blob_metadata<A>(
        filesystem: &mut F,
        final_name: &EntryName,
        scope: NamespaceRef,
        vault_entropy: E,
        identity_entropy: I,
        key_adapter: &mut A,
        limits: uste_storage::journal::BlobRecoveryLimits,
        cache: &mut PageCache,
    ) -> Result<(Self, RecoveryReport, Option<RecoveredFrontierTransaction>), TransactionError>
    where
        A: KeyAdapter<Envelope = W>,
    {
        let mut frontier = None;
        let (journal, report) = JournalStore::open_with_disk_blob_metadata(
            filesystem,
            final_name,
            scope.database(),
            vault_entropy,
            identity_entropy,
            key_adapter,
            limits,
            cache,
            |group| {
                frontier = Some(capture_group(scope, group)?);
                Ok(())
            },
        )
        .map_err(map_open_error)?;
        Ok((Self { scope, journal }, report, frontier))
    }

    /// Consume this exclusive owner into a deliberately bounded full-replay coordinator.
    /// Intended for small bootstrap prefixes before any derived roots exist. The caller supplies
    /// trusted genesis state; this is not the disk-backed large-history recovery path.
    /// Counts are checked before reducer preparation/metadata insertion, and no provisional
    /// coordinator escapes late authentication, resource or reducer failure. Storage metadata
    /// and the supplied reducer's own memory behavior are not bounded by these count limits.
    pub fn into_bounded_coordinator<S: TransactionState>(
        self,
        filesystem: &mut F,
        mut state: S,
        retention: RetentionDays,
        limits: CoordinatorRecoveryLimits,
        maximum_encoded_bytes: u64,
    ) -> Result<CommitCoordinator<S, F, W, E, I>, TransactionError> {
        let mut outcomes = std::collections::BTreeMap::new();
        let mut transactions = std::collections::BTreeMap::new();
        let mut owners = std::collections::BTreeMap::new();
        if let Some(frontier) = self.journal.frontier() {
            self.journal
                .visit_committed_range(
                    filesystem,
                    CommitRevision::FIRST,
                    frontier,
                    limits.maximum_outcomes as u64,
                    maximum_encoded_bytes,
                    |_, group| {
                        let decoded = super::decode_recovered_group(self.scope, group)?;
                        super::admit_recovered_metadata(&outcomes, &owners, &decoded, limits)?;
                        let prepared = state
                            .prepare(decoded.request, decoded.blob_inventory, group.revision)
                            .map_err(|error| match error {
                                super::ApplyError::ResourceLimit => StorageError::ResourceLimit,
                                _ => StorageError::IntegrityFailure,
                            })?;
                        if S::result_digest(&prepared) != decoded.outcome.result_digest {
                            return Err(StorageError::IntegrityFailure);
                        }
                        super::record_decoded_group_metadata(
                            &mut outcomes,
                            &mut transactions,
                            &mut owners,
                            &decoded,
                        )?;
                        state.publish(prepared);
                        Ok(())
                    },
                )
                .map_err(map_open_error)?;
        }
        Ok(CommitCoordinator {
            scope: self.scope,
            retention,
            journal: self.journal,
            state,
            outcomes,
            transactions,
            committed_blob_owners: owners,
            recovered: true,
            uncertain: false,
        })
    }

    /// Reconstruct exactly revision one while retaining this owner's authenticated terminal
    /// journal frontier. No transaction is appended and no historical root is published.
    ///
    /// This inventory-free bootstrap primitive is for origin index reconstruction, not a
    /// fallback to full-history replay. The trusted caller supplies the exact genesis reducer.
    /// One request is admitted by an encrypted-range byte limit before reducer preparation;
    /// inventories are rejected rather than silently dropping first-owner metadata. The exact
    /// reducer result digest and terminal cursor consumption must match before a value escapes.
    pub fn recover_inventory_free_genesis<S: TransactionState>(
        &self,
        filesystem: &mut F,
        state: S,
        maximum_encoded_bytes: u64,
    ) -> Result<RecoveredGenesis<S>, TransactionError> {
        self.recover_genesis_bounded(filesystem, state, maximum_encoded_bytes, None)
    }

    /// Reconstruct exactly the first certified transaction, including a bounded inventory.
    /// The storage decoder's independent inventory caps still apply before the narrower
    /// reference admission. No owner maps or root slots are created. Primary metadata staging
    /// and independent journal admission must preserve every first owner before live recovery.
    pub fn recover_primary_genesis<S: TransactionState>(
        &self,
        filesystem: &mut F,
        state: S,
        maximum_encoded_bytes: u64,
        maximum_inventory_references: usize,
    ) -> Result<RecoveredGenesis<S>, TransactionError> {
        self.recover_genesis_bounded(
            filesystem,
            state,
            maximum_encoded_bytes,
            Some(maximum_inventory_references),
        )
    }

    fn recover_genesis_bounded<S: TransactionState>(
        &self,
        filesystem: &mut F,
        mut state: S,
        maximum_encoded_bytes: u64,
        inventory_limit: Option<usize>,
    ) -> Result<RecoveredGenesis<S>, TransactionError> {
        let mut cursor = self.open_transaction_cursor(
            CommitRevision::FIRST,
            CommitRevision::FIRST,
            1,
            maximum_encoded_bytes,
        )?;
        let transaction = self
            .next_recovered_transaction(filesystem, &mut cursor)?
            .ok_or(TransactionError::IntegrityFailure)?;
        self.finish_transaction_cursor(cursor)?;
        if let Some(inventory) = transaction.blob_inventory.as_ref() {
            let maximum = inventory_limit.ok_or(TransactionError::InvalidRequest)?;
            if inventory.references().len() > maximum {
                return Err(TransactionError::ResourceLimit);
            }
        }
        let prepared = state
            .prepare(
                &transaction.canonical_request,
                transaction.blob_inventory.as_ref(),
                CommitRevision::FIRST,
            )
            .map_err(super::map_apply_error)?;
        if S::result_digest(&prepared) != transaction.outcome.result_digest {
            return Err(TransactionError::IntegrityFailure);
        }
        state.publish(prepared);
        Ok(RecoveredGenesis { state, transaction })
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
        let mut cursor =
            self.open_transaction_cursor(first, last, maximum_groups, maximum_encoded_bytes)?;
        while let Some(transaction) = self.next_recovered_transaction(filesystem, &mut cursor)? {
            visitor(filesystem, transaction).map_err(map_open_error)?;
        }
        self.finish_transaction_cursor(cursor).map(|_| ())
    }

    /// Order-independent validation in descending revision order. Every transaction is bound
    /// to the authenticated frontier before exposure, without per-revision suffix proof scans.
    /// Never use this for ordered reducer replay. Results remain provisional until success.
    pub fn visit_transactions_reverse<V>(
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
            .visit_committed_range_reverse_report(
                filesystem,
                first,
                last,
                maximum_groups,
                maximum_encoded_bytes,
                |filesystem, group| visitor(filesystem, capture_group(self.scope, group)?),
            )
            .map(|_| ())
            .map_err(map_open_error)
    }

    pub(crate) fn transaction_reader(&self) -> JournalTransactionReader<'_, F, W, E, I> {
        JournalTransactionReader {
            scope: self.scope,
            journal: &self.journal,
        }
    }
    /// Admit an inclusive authenticated range before I/O; no consumer authority is granted.
    pub fn open_transaction_cursor(
        &self,
        first: CommitRevision,
        last: CommitRevision,
        maximum_groups: u64,
        maximum_encoded_bytes: u64,
    ) -> Result<TransactionRecoveryCursor, TransactionError> {
        self.transaction_reader().open_transaction_cursor(
            first,
            last,
            maximum_groups,
            maximum_encoded_bytes,
        )
    }
    /// Bounded certificate lookahead with one shared range-byte budget.
    pub fn open_transaction_cursor_with_certificate_window(
        &self,
        first: CommitRevision,
        last: CommitRevision,
        maximum_groups: u64,
        maximum_encoded_bytes: u64,
        window_size: u64,
    ) -> Result<TransactionRecoveryCursor, TransactionError> {
        self.transaction_reader()
            .open_transaction_cursor_with_certificate_window(
                first,
                last,
                maximum_groups,
                maximum_encoded_bytes,
                window_size,
            )
    }
    /// A failed cursor cannot resume and yields no partial transaction.
    pub fn next_recovered_transaction(
        &self,
        filesystem: &mut F,
        cursor: &mut TransactionRecoveryCursor,
    ) -> Result<Option<RecoveredFrontierTransaction>, TransactionError> {
        self.transaction_reader()
            .next_recovered_transaction(filesystem, cursor)
    }
    /// Release terminal work only after exact range exhaustion.
    pub fn finish_transaction_cursor(
        &self,
        cursor: TransactionRecoveryCursor,
    ) -> Result<JournalRangeReadReport, TransactionError> {
        self.transaction_reader().finish_transaction_cursor(cursor)
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
        scope,
        revision: group.revision,
        certificate_digest: group.certificate_digest,
        logical_event_digest: group.logical_event_digest,
        blob_inventory_digest: group.blob_inventory_digest,
        principal: decoded.retry_key.principal,
        idempotency_key: decoded.retry_key.key,
        outcome: decoded.outcome,
        canonical_request,
        blob_inventory,
        certificate_proof: None,
    })
}
