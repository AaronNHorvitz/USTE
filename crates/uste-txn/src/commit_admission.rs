//! Shared read-only commit admission. No reservation or publication is performed here.
use super::*;

pub(super) enum CommitAdmission {
    Retry(TransactionOutcome),
    Fresh(FreshCommitAdmission),
}

pub(super) struct FreshCommitAdmission {
    pub accepted_at: UtcInstant,
    pub expires_at: UtcInstant,
    pub revision: CommitRevision,
    pub request_digest: [u8; 32],
    pub retry_key: RetryKey,
    pub new_owners: Vec<BlobReference>,
}

impl<S, F, W, E, I> CommitCoordinator<S, F, W, E, I>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    pub(super) fn admit_commit(
        &self,
        filesystem: &mut F,
        request: TransactionRequest<'_>,
        clock: &mut impl Clock,
        cancellation: &impl Cancellation,
        mut disk: Option<disk_coordinator::DiskCommitMetadata<'_>>,
    ) -> Result<CommitAdmission, TransactionError> {
        if self.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        if request.canonical_request.is_empty()
            || request.canonical_request.len() > MAX_REQUEST_BYTES
            || request.blob_inventory.is_some_and(BlobInventory::is_empty)
        {
            return Err(TransactionError::InvalidRequest);
        }
        if request
            .blob_inventory
            .is_some_and(|inventory| inventory.scope() != self.scope)
        {
            return Err(TransactionError::InvalidRequest);
        }
        let blob_inventory_digest = request
            .blob_inventory
            .map_or(EMPTY_BLOB_INVENTORY_DIGEST, BlobInventory::digest);
        let request_digest =
            transaction_request_digest(request.canonical_request, blob_inventory_digest);
        let accepted_at = clock
            .observe()
            .map_err(|_| TransactionError::RetryableUnavailable)?
            .wall_utc;
        let retry_key = RetryKey {
            principal: request.principal,
            key: request.idempotency_key,
        };
        let mut previous = self.outcomes.get(&retry_key).copied();
        if previous.is_none()
            && let Some(disk) = disk.as_mut()
        {
            previous = disk.base.retry_from_journal(
                &self.journal,
                filesystem,
                request.principal,
                request.idempotency_key,
                disk.cache,
            )?;
        }
        if let Some(previous) = previous {
            if accepted_at >= previous.expires_at {
                return Err(TransactionError::IdempotencyExpired);
            }
            return if previous.request_digest == request_digest
                && previous.transaction_id == request.transaction_id
            {
                Ok(CommitAdmission::Retry(previous))
            } else {
                Err(TransactionError::Conflict)
            };
        }
        if self.transactions.contains_key(&request.transaction_id) {
            return Err(TransactionError::Conflict);
        }
        if let Some(disk) = disk.as_mut() {
            if disk
                .base
                .transaction_from_journal(
                    &self.journal,
                    filesystem,
                    request.transaction_id,
                    disk.cache,
                )?
                .is_some()
            {
                return Err(TransactionError::Conflict);
            }
            if self.outcomes.len() >= disk.overlay.maximum_outcomes
                || disk
                    .base
                    .revision()
                    .get()
                    .checked_add(self.outcomes.len() as u64)
                    .is_none_or(|count| count >= MAX_OUTCOMES_PER_NAMESPACE as u64)
            {
                return Err(TransactionError::ResourceLimit);
            }
        }
        if self.outcomes.len() >= MAX_OUTCOMES_PER_NAMESPACE {
            return Err(TransactionError::ResourceLimit);
        }
        if cancellation.is_cancelled() {
            return Err(TransactionError::Cancelled);
        }
        // Resolve first ownership and admit overlay growth before preparation or any journal
        // write. After certification the publication path performs no disk metadata reads.
        let mut new_owners = Vec::new();
        if disk.is_some()
            && let Some(inventory) = request.blob_inventory
        {
            for reference in inventory.references() {
                let mut owner = self
                    .committed_blob_owners
                    .get(&(reference.scope(), reference.id()))
                    .copied();
                if owner.is_none()
                    && let Some(disk) = disk.as_mut()
                {
                    owner = disk.base.owner_from_journal(
                        &self.journal,
                        filesystem,
                        reference.id(),
                        disk.cache,
                    )?;
                }
                if let Some((committed, _)) = owner {
                    if committed != *reference {
                        return Err(TransactionError::IntegrityFailure);
                    }
                } else {
                    if let Some(disk) = disk.as_ref()
                        && self
                            .committed_blob_owners
                            .len()
                            .checked_add(new_owners.len())
                            .is_none_or(|count| count >= disk.overlay.maximum_blob_owners)
                    {
                        return Err(TransactionError::ResourceLimit);
                    }
                    new_owners
                        .try_reserve(1)
                        .map_err(|_| TransactionError::ResourceLimit)?;
                    new_owners.push(*reference);
                }
            }
        }
        if let Some(disk) = disk.as_ref()
            && self
                .committed_blob_owners
                .len()
                .checked_add(new_owners.len())
                .is_none_or(|count| count > disk.overlay.maximum_blob_owners)
        {
            return Err(TransactionError::ResourceLimit);
        }
        let revision = match self.journal.frontier() {
            Some(revision) => revision
                .checked_next()
                .map_err(|_| TransactionError::RevisionExhausted)?,
            None => CommitRevision::FIRST,
        };
        let expires_at = expiration(accepted_at, self.retention)?;
        Ok(CommitAdmission::Fresh(FreshCommitAdmission {
            accepted_at,
            expires_at,
            revision,
            request_digest,
            retry_key,
            new_owners,
        }))
    }
}
