//! Paired current-owner reads without granting maintenance or consumer authority.
use super::*;
impl PackedQuotaPrefix {
    pub(crate) fn usage_from_journal<F, W, E, I>(
        &self,
        journal: &JournalStore<F, W, E, I>,
        fs: &mut F,
        primary: &PackedCoordinatorPrefix,
        principal: PrincipalDigest,
        limits: TreeLookupLimits,
    ) -> Result<(crate::CommittedBlobUsage, [TreeLookupReport; 2]), TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        self.validate_live_pair(journal, primary)?;
        self.read_usage(principal, |tree, key| {
            journal
                .packed_tree_get(fs, tree, key, limits)
                .map_err(TransactionError::Storage)
        })
    }
}
