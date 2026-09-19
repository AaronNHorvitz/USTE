//! Bounded-memory admission, with optional single-pass first-reference evidence.

use super::*;
use uste_storage::{IndexGetLimits, PageCache};

/// Explicit recovery budgets. Compatibility admission rereads the prefix once per owner;
/// first-reference admission needs only the final retry/reference correspondence pass.
/// The total group ceiling covers all passes within this admission, not separate transaction
/// index admission. Every pass also obeys its encoded-byte ceiling.
#[derive(Clone, Copy, Debug)]
pub struct CoordinatorDiskAdmissionLimits {
    pub metadata: CoordinatorMetadataLoadLimits,
    pub lookup: IndexGetLimits,
    pub maximum_total_journal_groups: u64,
    pub maximum_encoded_bytes_per_pass: u64,
}

/// Journal-validated immutable coordinator metadata, without complete coordinator maps.
/// The anchor must still be paired with an independently admitted domain state. Raw recovery
/// lookups here do not grant consumer authorization or apply current expiry/revocation policy.
#[derive(Debug)]
pub struct CoordinatorDiskBase {
    pub(crate) metadata: RecoveredIndexRoot,
    pub(crate) transactions: CoordinatorTransactionIndex,
    pub(crate) first_references: Option<RecoveredIndexRoot>,
    pub(super) usage: Option<usage::BlobUsageIndex>,
}

impl CoordinatorDiskBase {
    pub(crate) fn has_unpublished_roots(&self) -> bool {
        self.metadata.generation() == 0 || self.transactions.root.generation() == 0
    }

    pub(crate) fn owner_count(&self) -> u64 {
        self.metadata
            .runs()
            .find(|run| run.family() == FAMILY_BLOB_OWNER)
            .map_or(0, |run| run.entry_count())
    }

    pub(crate) fn visit_owners_from_journal<F, W, E, I>(
        &self,
        journal: &uste_storage::journal::JournalStore<F, W, E, I>,
        filesystem: &mut F,
        limits: IndexRunReadLimits,
        visitor: &mut dyn FnMut(BlobReference, PrincipalDigest) -> Result<(), StorageError>,
    ) -> Result<(), TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        if self.owner_count() == 0 {
            return Ok(());
        }
        journal
            .visit_index_run(
                filesystem,
                &self.metadata,
                FAMILY_BLOB_OWNER,
                limits,
                &mut |key, value| {
                    let (reference, principal) = decode_owner(self.metadata.scope(), key, value)?;
                    visitor(reference, principal)
                },
            )
            .map_err(TransactionError::Storage)?;
        Ok(())
    }

    pub(crate) fn revalidate_journal_binding<F, W, E, I>(
        &self,
        journal: &uste_storage::journal::JournalStore<F, W, E, I>,
        filesystem: &mut F,
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<(), TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        // The journal lookup verifies this root's historical certificate on its own chain,
        // including the zero-suffix case where no later group would otherwise be inspected.
        let (value, _) = journal
            .index_get_bounded(
                filesystem,
                &self.metadata,
                FAMILY_METADATA,
                METADATA_KEY,
                limits,
                cache,
            )
            .map_err(TransactionError::Storage)?;
        let value = value.ok_or(TransactionError::IntegrityFailure)?;
        parse_metadata(&value, self.metadata.revision()).map_err(TransactionError::Storage)?;
        Ok(())
    }

    pub fn anchor(&self) -> IndexRootAnchor {
        self.metadata.anchor()
    }

    pub fn transaction_index(&self) -> &CoordinatorTransactionIndex {
        &self.transactions
    }

    /// Whether this base retains admitted single-pass first-owner evidence for bounded rebase.
    pub fn has_first_reference_evidence(&self) -> bool {
        self.first_references.is_some()
    }

    /// Trusted recovery-only raw retry lookup at this immutable base, including expired entries.
    #[allow(clippy::too_many_arguments)]
    pub fn retry_at_base<F, W, E, I>(
        &self,
        recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        principal: PrincipalDigest,
        key: IdempotencyKey,
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<Option<TransactionOutcome>, TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        if recovery.scope() != self.metadata.scope() {
            return Err(TransactionError::IntegrityFailure);
        }
        self.retry_from_journal(&recovery.journal, filesystem, principal, key, limits, cache)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn retry_from_journal<F, W, E, I>(
        &self,
        journal: &uste_storage::journal::JournalStore<F, W, E, I>,
        filesystem: &mut F,
        principal: PrincipalDigest,
        key: IdempotencyKey,
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<Option<TransactionOutcome>, TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        let mut encoded_key = [0; OUTCOME_KEY_BYTES];
        encoded_key[..32].copy_from_slice(&principal.as_bytes());
        encoded_key[32..].copy_from_slice(key.as_bytes());
        let (value, _) = journal
            .index_get_bounded(
                filesystem,
                &self.metadata,
                FAMILY_OUTCOME,
                &encoded_key,
                limits,
                cache,
            )
            .map_err(TransactionError::Storage)?;
        value
            .map(|value| {
                decode_outcome(&encoded_key, &value, self.metadata.revision())
                    .map(|(_, _, outcome)| outcome)
                    .map_err(TransactionError::Storage)
            })
            .transpose()
    }

    /// Privileged exact revision from an independently admitted first-reference root.
    pub(crate) fn first_reference_from_journal<F, W, E, I>(
        &self,
        journal: &uste_storage::journal::JournalStore<F, W, E, I>,
        filesystem: &mut F,
        id: BlobId,
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<Option<CommitRevision>, TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        let Some(root) = self.first_references.as_ref() else {
            return Ok(None);
        };
        let (value, _) = journal
            .index_get_bounded(filesystem, root, 1, &id.as_bytes(), limits, cache)
            .map_err(TransactionError::Storage)?;
        value
            .map(|value| {
                first_reference::decode_first_reference(
                    &id.as_bytes(),
                    &value,
                    self.metadata.revision(),
                )
                .map_err(TransactionError::Storage)
            })
            .transpose()
    }

    /// Trusted recovery-only raw first-owner lookup; it is not a blob-read capability.
    pub fn owner_at_base<F, W, E, I>(
        &self,
        recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        id: BlobId,
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<Option<(BlobReference, PrincipalDigest)>, TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        if self.metadata.scope() != recovery.scope() {
            return Err(TransactionError::IntegrityFailure);
        }
        self.owner_from_journal(&recovery.journal, filesystem, id, limits, cache)
    }

    pub(crate) fn owner_from_journal<F, W, E, I>(
        &self,
        journal: &uste_storage::journal::JournalStore<F, W, E, I>,
        filesystem: &mut F,
        id: BlobId,
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<Option<(BlobReference, PrincipalDigest)>, TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        if !self
            .metadata
            .runs()
            .any(|run| run.family() == FAMILY_BLOB_OWNER)
        {
            return Ok(None);
        }
        let (value, _) = journal
            .index_get_bounded(
                filesystem,
                &self.metadata,
                FAMILY_BLOB_OWNER,
                &id.as_bytes(),
                limits,
                cache,
            )
            .map_err(TransactionError::Storage)?;
        value
            .map(|value| {
                decode_owner(self.metadata.scope(), &id.as_bytes(), &value)
                    .map_err(TransactionError::Storage)
            })
            .transpose()
    }

    pub(crate) fn transaction_from_journal<F, W, E, I>(
        &self,
        journal: &uste_storage::journal::JournalStore<F, W, E, I>,
        filesystem: &mut F,
        id: TransactionId,
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<Option<(PrincipalDigest, TransactionOutcome)>, TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        let (value, _) = journal
            .index_get_bounded(
                filesystem,
                &self.transactions.root,
                1,
                id.as_bytes(),
                limits,
                cache,
            )
            .map_err(TransactionError::Storage)?;
        value
            .map(|value| {
                if value.len() != 136 {
                    return Err(TransactionError::IntegrityFailure);
                }
                let mut key = [0; OUTCOME_KEY_BYTES];
                key[..32].copy_from_slice(&value[..32]);
                let (principal, _, outcome) =
                    decode_outcome(&key, &value[32..], self.metadata.revision())
                        .map_err(TransactionError::Storage)?;
                if outcome.transaction_id != id {
                    return Err(TransactionError::IntegrityFailure);
                }
                Ok((principal, outcome))
            })
            .transpose()
    }
}

/// Prove exact retries, transaction collisions and earliest blob ownership using disk roots
/// plus the authoritative journal. This bounded-memory compatibility path is O(revisions *
/// owners), not a qualifying large-scale recovery algorithm. Admission rejects its aggregate
/// work requirement before owner replay. No complete retry, transaction or owner map is built.
pub fn admit_coordinator_disk_base<F, W, E, I>(
    recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
    filesystem: &mut F,
    candidate: CoordinatorMetadataCandidate,
    transactions: CoordinatorTransactionIndex,
    limits: CoordinatorDiskAdmissionLimits,
    cache: &mut PageCache,
) -> Result<CoordinatorDiskBase, TransactionError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    admit_base(
        recovery,
        filesystem,
        candidate,
        transactions,
        limits,
        cache,
        None,
    )
}

/// Single-journal-pass owner admission using an independently authenticated first-reference run.
/// The optional cache remains derived evidence: every claim is checked against the journal.
#[allow(clippy::too_many_arguments)]
pub fn admit_coordinator_disk_base_with_first_references<F, W, E, I>(
    recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
    filesystem: &mut F,
    candidate: CoordinatorMetadataCandidate,
    transactions: CoordinatorTransactionIndex,
    first_references: RecoveredIndexRoot,
    first_reference_limits: IndexRunReadLimits,
    limits: CoordinatorDiskAdmissionLimits,
    cache: &mut PageCache,
) -> Result<CoordinatorDiskBase, TransactionError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    admit_base(
        recovery,
        filesystem,
        candidate,
        transactions,
        limits,
        cache,
        Some((first_references, first_reference_limits)),
    )
}

fn admit_base<F, W, E, I>(
    recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
    filesystem: &mut F,
    candidate: CoordinatorMetadataCandidate,
    transactions: CoordinatorTransactionIndex,
    limits: CoordinatorDiskAdmissionLimits,
    cache: &mut PageCache,
    first_references: Option<(RecoveredIndexRoot, IndexRunReadLimits)>,
) -> Result<CoordinatorDiskBase, TransactionError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if candidate.root.scope() != recovery.scope()
        || candidate.root.index_profile() != &COORDINATOR_METADATA_PROFILE_V1
        || candidate.anchor() != transactions.anchor()
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let mut budget = LoadBudget::new(limits.metadata);
    let mut metadata = None;
    visit_family(
        recovery,
        filesystem,
        &candidate,
        FAMILY_METADATA,
        1,
        &mut budget,
        &mut |key, value| {
            if key != METADATA_KEY || metadata.is_some() {
                return Err(StorageError::IntegrityFailure);
            }
            metadata = Some(parse_metadata(value, candidate.revision())?);
            Ok(())
        },
    )?;
    let metadata = metadata.ok_or(TransactionError::IntegrityFailure)?;
    validate_shape(&candidate, metadata, limits.metadata)?;
    if metadata.outcomes != candidate.revision().get() {
        return Err(TransactionError::IntegrityFailure);
    }
    let total_groups = first_references
        .as_ref()
        .map_or(metadata.owners, |_| 0)
        .checked_add(1)
        .and_then(|passes| passes.checked_mul(candidate.revision().get()))
        .ok_or(TransactionError::ResourceLimit)?;
    if total_groups > limits.maximum_total_journal_groups {
        return Err(TransactionError::ResourceLimit);
    }
    if let Some((root, run_limits)) = &first_references {
        if root.anchor() != candidate.anchor()
            || root.index_profile() != &COORDINATOR_FIRST_REFERENCE_PROFILE_V1
            || root.runs().len() != 1
            || root
                .runs()
                .next()
                .is_none_or(|run| run.family() != 1 || run.entry_count() != metadata.owners)
            || metadata.owners == 0
        {
            return Err(TransactionError::IntegrityFailure);
        }
        recovery.visit_index_run(filesystem, root, 1, *run_limits, &mut |key, value| {
            first_reference::decode_first_reference(key, value, candidate.revision())?;
            Ok(())
        })?;
    }
    visit_family(
        recovery,
        filesystem,
        &candidate,
        FAMILY_OUTCOME,
        metadata.outcomes,
        &mut budget,
        &mut |key, value| {
            decode_outcome(key, value, candidate.revision())?;
            Ok(())
        },
    )?;
    if metadata.owners != 0 {
        let read_limits = IndexRunReadLimits::new(
            budget.remaining_pages()?.min(MAX_INDEX_PAGES_PER_RUN),
            metadata.owners,
            budget
                .remaining_logical_bytes()?
                .min(MAX_INDEX_RUN_LOGICAL_BYTES),
        )
        .map_err(TransactionError::Storage)?;
        let mut cursor = recovery.open_index_run_cursor(
            filesystem,
            &candidate.root,
            FAMILY_BLOB_OWNER,
            read_limits,
        )?;
        while let Some(entry) = recovery.next_index_run_entry(filesystem, &mut cursor)? {
            let (reference, principal) = decode_owner(recovery.scope(), &entry.key, &entry.value)
                .map_err(TransactionError::Storage)?;
            if let Some((root, _)) = &first_references {
                let (value, _) = recovery.index_get_bounded(
                    filesystem,
                    root,
                    1,
                    &entry.key,
                    limits.lookup,
                    cache,
                )?;
                first_reference::decode_first_reference(
                    &entry.key,
                    &value.ok_or(TransactionError::IntegrityFailure)?,
                    candidate.revision(),
                )
                .map_err(TransactionError::Storage)?;
                continue;
            }
            let mut found = false;
            recovery.visit_transactions(
                filesystem,
                CommitRevision::FIRST,
                candidate.revision(),
                candidate.revision().get(),
                limits.maximum_encoded_bytes_per_pass,
                |_, transaction| {
                    if !found && let Some(inventory) = transaction.blob_inventory.as_ref() {
                        for actual in inventory.references() {
                            if actual.id() == reference.id() {
                                if *actual != reference || transaction.principal != principal {
                                    return Err(StorageError::IntegrityFailure);
                                }
                                found = true;
                                break;
                            }
                        }
                    }
                    Ok(())
                },
            )?;
            if !found {
                return Err(TransactionError::IntegrityFailure);
            }
        }
        let report = recovery.finish_index_run_cursor(cursor)?;
        if report.entries != metadata.owners {
            return Err(TransactionError::IntegrityFailure);
        }
        budget.add(&report)?;
    }
    let mut first_matches = 0_u64;
    // Exact retry/reference correspondence and first-revision counts are order-independent.
    // The compatibility first-owner discovery above deliberately remains forward ordered.
    recovery.visit_transactions_reverse(
        filesystem,
        CommitRevision::FIRST,
        candidate.revision(),
        candidate.revision().get(),
        limits.maximum_encoded_bytes_per_pass,
        |filesystem, transaction| {
            let expected = encode_outcome(
                transaction.principal,
                transaction.idempotency_key,
                transaction.outcome,
            )?;
            let (actual, _) = recovery
                .index_get_bounded(
                    filesystem,
                    &candidate.root,
                    FAMILY_OUTCOME,
                    &expected.key,
                    limits.lookup,
                    cache,
                )
                .map_err(storage_error)?;
            if actual.as_deref() != Some(expected.value.as_slice()) {
                return Err(StorageError::IntegrityFailure);
            }
            if let Some(inventory) = transaction.blob_inventory.as_ref() {
                for reference in inventory.references() {
                    let (actual, _) = recovery
                        .index_get_bounded(
                            filesystem,
                            &candidate.root,
                            FAMILY_BLOB_OWNER,
                            &reference.id().as_bytes(),
                            limits.lookup,
                            cache,
                        )
                        .map_err(storage_error)?;
                    let actual = actual.ok_or(StorageError::IntegrityFailure)?;
                    let (stored, principal) =
                        decode_owner(recovery.scope(), &reference.id().as_bytes(), &actual)?;
                    if stored != *reference {
                        return Err(StorageError::IntegrityFailure);
                    }
                    if let Some((root, _)) = &first_references {
                        let key = reference.id().as_bytes();
                        let (value, _) = recovery
                            .index_get_bounded(filesystem, root, 1, &key, limits.lookup, cache)
                            .map_err(storage_error)?;
                        let first = first_reference::decode_first_reference(
                            &key,
                            &value.ok_or(StorageError::IntegrityFailure)?,
                            candidate.revision(),
                        )?;
                        if first > transaction.revision {
                            return Err(StorageError::IntegrityFailure);
                        }
                        if first == transaction.revision {
                            if principal != transaction.principal {
                                return Err(StorageError::IntegrityFailure);
                            }
                            first_matches = first_matches
                                .checked_add(1)
                                .ok_or(StorageError::ResourceLimit)?;
                        }
                    }
                }
            }
            Ok(())
        },
    )?;
    if first_references.is_some() && first_matches != metadata.owners {
        return Err(TransactionError::IntegrityFailure);
    }
    Ok(CoordinatorDiskBase {
        metadata: candidate.root,
        transactions,
        first_references: first_references.map(|(root, _)| root),
        usage: None,
    })
}

fn storage_error(error: TransactionError) -> StorageError {
    match error {
        TransactionError::Storage(error) => error,
        TransactionError::ResourceLimit => StorageError::ResourceLimit,
        _ => StorageError::IntegrityFailure,
    }
}
