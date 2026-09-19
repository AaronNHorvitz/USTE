//! Privileged metadata reads. Consumer authority belongs to the restricted facade.
use super::*;

#[derive(Clone, Copy)]
pub struct PackedBlobAccountingLimits {
    pub lookup: TreeLookupLimits,
    pub maximum_total_owners: u64,
}
fn unexpired(
    outcome: Option<TransactionOutcome>,
    now: UtcInstant,
) -> Result<Option<TransactionOutcome>, TransactionError> {
    match outcome {
        Some(value) if now >= value.expires_at => Err(TransactionError::IdempotencyExpired),
        other => Ok(other),
    }
}
impl<S, F, W, E, I> PackedCommitCoordinator<S, F, W, E, I>
where
    S: PackedCoordinatorState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    pub fn scope(&self) -> NamespaceRef {
        self.inner.scope
    }
    pub fn outcome(
        &self,
        fs: &mut F,
        principal: PrincipalDigest,
        key: IdempotencyKey,
        now: UtcInstant,
        limits: TreeLookupLimits,
    ) -> Result<Option<TransactionOutcome>, TransactionError> {
        if self.inner.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        let outcome = match self.inner.outcomes.get(&RetryKey { principal, key }) {
            Some(outcome) => Some(*outcome),
            None => {
                self.primary
                    .retry_from_journal(&self.inner.journal, fs, principal, key, limits)?
            }
        };
        unexpired(outcome, now)
    }
    pub fn transaction_outcome(
        &self,
        fs: &mut F,
        principal: PrincipalDigest,
        id: TransactionId,
        now: UtcInstant,
        limits: TreeLookupLimits,
    ) -> Result<Option<TransactionOutcome>, TransactionError> {
        if self.inner.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        let outcome = match self.inner.transactions.get(&id) {
            Some(outcome) => Some(*outcome),
            None => self
                .primary
                .transaction_from_journal(&self.inner.journal, fs, id, limits)?,
        }
        .filter(|(owner, _)| *owner == principal)
        .map(|(_, outcome)| outcome);
        unexpired(outcome, now)
    }
    /// Current exact committed charges: admitted base plus bounded disjoint first-owner overlay.
    /// Staged bytes/reservations are not included and must not be inferred to be zero.
    pub fn committed_blob_usage(
        &self,
        fs: &mut F,
        principal: PrincipalDigest,
        limits: PackedBlobAccountingLimits,
    ) -> Result<CommittedBlobUsage, TransactionError> {
        if self.inner.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        let owners = self
            .primary
            .owner_count()
            .checked_add(self.inner.committed_blob_owners.len() as u64)
            .ok_or(TransactionError::ResourceLimit)?;
        if limits.maximum_total_owners > uste_storage::MAX_COMMITTED_BLOBS_PER_JOURNAL as u64
            || owners > limits.maximum_total_owners
        {
            return Err(TransactionError::ResourceLimit);
        }
        let (mut usage, _) = self.quota.usage_from_journal(
            &self.inner.journal,
            fs,
            &self.primary,
            principal,
            limits.lookup,
        )?;
        for (reference, owner) in self.inner.committed_blob_owners.values() {
            usage.namespace_bytes = usage
                .namespace_bytes
                .checked_add(reference.byte_len())
                .ok_or(TransactionError::ResourceLimit)?;
            if *owner == principal {
                usage.principal_bytes = usage
                    .principal_bytes
                    .checked_add(reference.byte_len())
                    .ok_or(TransactionError::ResourceLimit)?;
            }
        }
        usage.owners = owners;
        Ok(usage)
    }
}
