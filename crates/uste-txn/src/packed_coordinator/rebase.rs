//! Private paired suffix staging and terminal publication; no authoritative transaction writes.
use super::*;
use crate::index_recovery::JournalTransactionReader;
use uste_storage::{journal::JournalRangeReadReport, packed_tree_batch::TreeBatchReport};

/// Trusted ready-domain binding. No full snapshot may be constructed just to answer this call.
pub trait PackedCoordinatorPublicationState: PackedCoordinatorState {
    fn packed_publication_claims(
        &self,
        scope: NamespaceRef,
        anchor: (CommitRevision, [u8; 32]),
    ) -> Result<PackedRootClaims, ApplyError>;
}
#[derive(Clone, Copy)]
pub struct PackedMetadataRebaseLimits {
    pub staging: PackedCoordinatorLimits,
    pub maximum_groups: u64,
    pub maximum_encoded_bytes: u64,
    pub certificate_window: u64,
    pub maximum_publication_attempts: u8,
}
pub struct PackedMetadataRebaseReport {
    pub journal: JournalRangeReadReport,
    pub primary_root: CertifiedPackedRoot,
    pub quota_root: CertifiedPackedRoot,
    pub lookup_pages: u64,
    pub lookup_bytes: u64,
    pub batch_read_pages: u64,
    pub batch_read_bytes: u64,
    pub written_pages: u64,
    pub written_nodes: u64,
}
#[derive(Default)]
struct Work {
    lookup_pages: u64,
    lookup_bytes: u64,
    read_pages: u64,
    read_bytes: u64,
    pages: u64,
    nodes: u64,
}
fn add(value: &mut u64, amount: u64) -> Result<(), TransactionError> {
    *value = value
        .checked_add(amount)
        .ok_or(TransactionError::ResourceLimit)?;
    Ok(())
}
impl Work {
    fn batch(&mut self, batch: TreeBatchReport) -> Result<(), TransactionError> {
        add(&mut self.read_pages, batch.read_pages)?;
        add(&mut self.read_bytes, batch.read_bytes)?;
        add(&mut self.pages, batch.written_pages)?;
        add(&mut self.nodes, batch.written_nodes)
    }
}
impl<S, F, W, E, I> PackedCommitCoordinator<S, F, W, E, I>
where
    S: PackedCoordinatorPublicationState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// No-op returns None. Once an attempted nonempty rebase fails, fresh writes wait for retry;
    /// exact outcomes remain available. Neither private stages nor a primary-only root install.
    pub fn rebase_metadata(
        &mut self,
        fs: &mut F,
        limits: PackedMetadataRebaseLimits,
    ) -> Result<Option<PackedMetadataRebaseReport>, TransactionError> {
        let anchor = self
            .inner
            .checkpoint_anchor()?
            .ok_or(TransactionError::IntegrityFailure)?;
        if self.inner.outcomes.is_empty() {
            if anchor != self.primary.anchor() || self.rebase_required {
                return Err(TransactionError::IntegrityFailure);
            }
            return Ok(None);
        }
        self.rebase_required = true;
        if limits.maximum_publication_attempts == 0 || limits.maximum_publication_attempts > 64 {
            return Err(TransactionError::ResourceLimit);
        }
        let count = anchor
            .0
            .get()
            .checked_sub(self.primary.anchor().0.get())
            .ok_or(TransactionError::IntegrityFailure)?;
        if count != self.inner.outcomes.len() as u64
            || self.inner.transactions.len() != self.inner.outcomes.len()
        {
            return Err(TransactionError::IntegrityFailure);
        }
        self.quota
            .validate_live_pair(&self.inner.journal, &self.primary)?;
        let scope = self.inner.scope;
        let mut cursor = JournalTransactionReader {
            scope,
            journal: &self.inner.journal,
        }
        .open_transaction_cursor_with_certificate_window(
            self.primary
                .anchor()
                .0
                .checked_next()
                .map_err(|_| TransactionError::RevisionExhausted)?,
            anchor.0,
            limits.maximum_groups,
            limits.maximum_encoded_bytes,
            limits.certificate_window,
        )?;
        let claims = self
            .inner
            .state
            .packed_publication_claims(scope, anchor)
            .map_err(map_apply_error)?;
        if (claims.revision, claims.certificate_digest) != anchor
            || (claims.reducer_profile, claims.state_commitment_profile) != self.profiles
        {
            return Err(TransactionError::IntegrityFailure);
        }
        let mut primary = None;
        let mut quota = None;
        let mut work = Work::default();
        while let Some(transaction) = (JournalTransactionReader {
            scope,
            journal: &self.inner.journal,
        })
        .next_recovered_transaction(fs, &mut cursor)?
        {
            let retry = RetryKey {
                principal: transaction.principal,
                key: transaction.idempotency_key,
            };
            if self.inner.outcomes.get(&retry) != Some(&transaction.outcome)
                || self
                    .inner
                    .transactions
                    .get(&transaction.outcome.transaction_id)
                    != Some(&(transaction.principal, transaction.outcome))
            {
                return Err(TransactionError::IntegrityFailure);
            }
            let (next_primary, p) = disk_metadata::stage_packed_coordinator_prefix_on_journal(
                scope,
                &mut self.inner.journal,
                fs,
                Some(primary.as_ref().unwrap_or(&self.primary)),
                &transaction,
                limits.staging,
            )?;
            let (next_quota, q) = disk_metadata::stage_packed_quota_prefix_on_journal(
                scope,
                &mut self.inner.journal,
                fs,
                Some(quota.as_ref().unwrap_or(&self.quota)),
                &next_primary,
                &transaction,
                limits.staging,
            )?;
            add(&mut work.lookup_pages, p.owner_lookup_pages)?;
            add(&mut work.lookup_bytes, p.owner_lookup_bytes)?;
            add(&mut work.lookup_pages, q.primary_lookup_pages)?;
            add(&mut work.lookup_bytes, q.primary_lookup_bytes)?;
            if let Some(lookup) = q.principal_lookup {
                add(&mut work.lookup_pages, lookup.pages)?;
                add(&mut work.lookup_bytes, lookup.encoded_bytes)?;
            }
            for batch in p.batches.into_iter().chain(q.batches) {
                work.batch(batch)?;
            }
            primary = Some(next_primary);
            quota = Some(next_quota);
        }
        let journal = JournalTransactionReader {
            scope,
            journal: &self.inner.journal,
        }
        .finish_transaction_cursor(cursor)?;
        let primary = primary.ok_or(TransactionError::IntegrityFailure)?;
        let quota = quota.ok_or(TransactionError::IntegrityFailure)?;
        if primary.anchor() != anchor
            || journal.groups != count
            || primary
                .owner_count()
                .checked_sub(self.primary.owner_count())
                != Some(self.inner.committed_blob_owners.len() as u64)
        {
            return Err(TransactionError::IntegrityFailure);
        }
        // Independent bounded overlay correspondence before either terminal manifest can publish.
        for ((_, id), expected) in &self.inner.committed_blob_owners {
            let (actual, lookup) = primary.owner_from_journal_report(
                &self.inner.journal,
                fs,
                *id,
                limits.staging.lookup,
            )?;
            add(&mut work.lookup_pages, lookup.pages)?;
            add(&mut work.lookup_bytes, lookup.encoded_bytes)?;
            if actual != Some(*expected) {
                return Err(TransactionError::IntegrityFailure);
            }
        }
        quota.validate_live_pair(&self.inner.journal, &primary)?;
        let primary_root = self
            .inner
            .journal
            .publish_packed_root(
                fs,
                scope,
                COORDINATOR_PACKED_PROFILE_V1,
                claims,
                &primary.families(),
                limits.maximum_publication_attempts,
            )
            .map_err(TransactionError::Storage)?;
        let quota_root = self
            .inner
            .journal
            .publish_packed_root(
                fs,
                scope,
                COORDINATOR_PACKED_USAGE_PROFILE_V1,
                claims,
                &quota.families(),
                limits.maximum_publication_attempts,
            )
            .map_err(TransactionError::Storage)?;
        self.primary = primary;
        self.quota = quota;
        self.inner.outcomes.clear();
        self.inner.transactions.clear();
        self.inner.committed_blob_owners.clear();
        self.rebase_required = false;
        Ok(Some(PackedMetadataRebaseReport {
            journal,
            primary_root,
            quota_root,
            lookup_pages: work.lookup_pages,
            lookup_bytes: work.lookup_bytes,
            batch_read_pages: work.read_pages,
            batch_read_bytes: work.read_bytes,
            written_pages: work.pages,
            written_nodes: work.nodes,
        }))
    }
}
