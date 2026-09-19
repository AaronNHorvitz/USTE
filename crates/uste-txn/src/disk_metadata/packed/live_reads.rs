//! Internal live-coordinator reads retain the journal's historical owner/key checks.
use super::*;
use uste_storage::journal::JournalStore;

impl PackedCoordinatorPrefix {
    pub(crate) fn retry_from_journal<F, W, E, I>(
        &self,
        journal: &JournalStore<F, W, E, I>,
        fs: &mut F,
        principal: PrincipalDigest,
        key: IdempotencyKey,
        limits: TreeLookupLimits,
    ) -> Result<Option<TransactionOutcome>, TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        let mut encoded = [0; 48];
        encoded[..32].copy_from_slice(&principal.as_bytes());
        encoded[32..].copy_from_slice(key.as_bytes());
        let result = journal
            .packed_tree_get(fs, &self.trees[0], &encoded, limits)
            .map_err(TransactionError::Storage)?;
        result
            .value
            .map(|v| decode_outcome(&encoded, v.as_slice(), self.anchor.0).map(|v| v.2))
            .transpose()
            .map_err(TransactionError::Storage)
    }

    pub(crate) fn transaction_from_journal<F, W, E, I>(
        &self,
        journal: &JournalStore<F, W, E, I>,
        fs: &mut F,
        id: TransactionId,
        limits: TreeLookupLimits,
    ) -> Result<Option<(PrincipalDigest, TransactionOutcome)>, TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        let result = journal
            .packed_tree_get(fs, &self.trees[1], id.as_bytes(), limits)
            .map_err(TransactionError::Storage)?;
        result
            .value
            .map(|v| {
                let value = v.as_slice();
                if value.len() != 136 {
                    return Err(StorageError::IntegrityFailure);
                }
                let mut key = [0; 48];
                key[..32].copy_from_slice(&value[..32]);
                let (principal, _, outcome) = decode_outcome(&key, &value[32..], self.anchor.0)?;
                if outcome.transaction_id != id {
                    return Err(StorageError::IntegrityFailure);
                }
                Ok((principal, outcome))
            })
            .transpose()
            .map_err(TransactionError::Storage)
    }

    pub(crate) fn owner_from_journal<F, W, E, I>(
        &self,
        journal: &JournalStore<F, W, E, I>,
        fs: &mut F,
        id: BlobId,
        limits: TreeLookupLimits,
    ) -> Result<Option<(BlobReference, PrincipalDigest)>, TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        self.owner_from_journal_report(journal, fs, id, limits)
            .map(|v| v.0)
    }

    pub(crate) fn owner_from_journal_report<F, W, E, I>(
        &self,
        journal: &JournalStore<F, W, E, I>,
        fs: &mut F,
        id: BlobId,
        limits: TreeLookupLimits,
    ) -> Result<(Option<(BlobReference, PrincipalDigest)>, TreeLookupReport), TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        let result = journal
            .packed_tree_get(fs, &self.trees[2], &id.as_bytes(), limits)
            .map_err(TransactionError::Storage)?;
        let owner = result
            .value
            .map(|v| decode_owner(self.scope, &id.as_bytes(), v.as_slice()))
            .transpose()
            .map_err(TransactionError::Storage)?;
        Ok((owner, result.report))
    }
}
