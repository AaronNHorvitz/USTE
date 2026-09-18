//! Bounded-memory, deliberately read-amplified admission of the existing metadata profile.

use super::*;
use uste_storage::{IndexGetLimits, PageCache};

/// Explicit recovery budgets. Owner validation rereads the prefix once per owner; the total
/// group ceiling includes those passes and the final retry/reference correspondence pass.
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
    metadata: RecoveredIndexRoot,
    transactions: CoordinatorTransactionIndex,
}

impl CoordinatorDiskBase {
    pub fn anchor(&self) -> IndexRootAnchor {
        self.metadata.anchor()
    }

    pub fn transaction_index(&self) -> &CoordinatorTransactionIndex {
        &self.transactions
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
        let mut encoded_key = [0; OUTCOME_KEY_BYTES];
        encoded_key[..32].copy_from_slice(&principal.as_bytes());
        encoded_key[32..].copy_from_slice(key.as_bytes());
        let (value, _) = recovery.index_get_bounded(
            filesystem,
            &self.metadata,
            FAMILY_OUTCOME,
            &encoded_key,
            limits,
            cache,
        )?;
        value
            .map(|value| {
                decode_outcome(&encoded_key, &value, self.metadata.revision())
                    .map(|(_, _, outcome)| outcome)
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
        if !self
            .metadata
            .runs()
            .any(|run| run.family() == FAMILY_BLOB_OWNER)
        {
            return Ok(None);
        }
        let (value, _) = recovery.index_get_bounded(
            filesystem,
            &self.metadata,
            FAMILY_BLOB_OWNER,
            &id.as_bytes(),
            limits,
            cache,
        )?;
        value
            .map(|value| {
                decode_owner(recovery.scope(), &id.as_bytes(), &value)
                    .map_err(TransactionError::Storage)
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
    let total_groups = metadata
        .owners
        .checked_add(1)
        .and_then(|passes| passes.checked_mul(candidate.revision().get()))
        .ok_or(TransactionError::ResourceLimit)?;
    if total_groups > limits.maximum_total_journal_groups {
        return Err(TransactionError::ResourceLimit);
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
    recovery.visit_transactions(
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
                    let (stored, _) =
                        decode_owner(recovery.scope(), &reference.id().as_bytes(), &actual)?;
                    if stored != *reference {
                        return Err(StorageError::IntegrityFailure);
                    }
                }
            }
            Ok(())
        },
    )?;
    Ok(CoordinatorDiskBase {
        metadata: candidate.root,
        transactions,
    })
}

fn storage_error(error: TransactionError) -> StorageError {
    match error {
        TransactionError::Storage(error) => error,
        TransactionError::ResourceLimit => StorageError::ResourceLimit,
        _ => StorageError::IntegrityFailure,
    }
}
