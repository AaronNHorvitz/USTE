//! Shared admission for explicit-I/O domain recovery orchestrators.
use super::*;

impl<F, W, E, I> AuthenticatedIndexRecovery<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Validate the exact admitted historical pair and trusted domain under this owner, without
    /// reads or writes. Return the actual terminal anchor, not the selected historical anchor.
    pub fn validate_packed_recovery_base<S: PackedCoordinatorState>(
        &self,
        primary: &PackedCoordinatorPrefix,
        quota: &PackedQuotaPrefix,
        primary_root: &CertifiedPackedRoot,
        quota_root: &CertifiedPackedRoot,
        state: &S,
    ) -> Result<(CommitRevision, [u8; 32]), TransactionError> {
        validate_packed_base(self, primary, quota, primary_root, quota_root, state)?;
        self.journal
            .checkpoint_anchor()
            .ok_or(TransactionError::IntegrityFailure)
    }
}
