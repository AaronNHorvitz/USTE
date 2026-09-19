//! Restricted packed-metadata facade. Raw coordinator capabilities never escape.
mod read;
mod write;
use crate::{
    AuthorizedDiskPolicyState, AuthorizedError, CommittedBlobUsage, PackedBlobAccountingLimits,
    PackedCommitCoordinator, PackedCoordinatorState, TransactionError, TransactionOutcome,
};
pub use read::{AuthorizedPackedReadState, AuthorizedPackedReader};
use uste_crypto::EntropySource;
use uste_policy::{Action, AuthenticatedPrincipal, PolicyKernel, Target};
use uste_storage::{
    Clock, OwnershipFileSystem, journal::DurableKeyEnvelope, packed_tree_lookup::TreeLookupLimits,
};
use uste_types::{IdempotencyKey, TransactionId};
pub use write::{AuthorizedPackedWriteState, AuthorizedPackedWriter};

pub struct AuthorizedPackedMetadata<'a, S, F, W, E, I>
where
    S: PackedCoordinatorState + AuthorizedDiskPolicyState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    inner: &'a PackedCommitCoordinator<S, F, W, E, I>,
    policy: &'a PolicyKernel,
}
impl<'a, S, F, W, E, I> AuthorizedPackedMetadata<'a, S, F, W, E, I>
where
    S: PackedCoordinatorState + AuthorizedDiskPolicyState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Trusted setup; consumers receive only this restricted borrowed surface.
    pub fn new(
        inner: &'a PackedCommitCoordinator<S, F, W, E, I>,
        policy: &'a PolicyKernel,
    ) -> Result<Self, AuthorizedError> {
        let facade = Self { inner, policy };
        facade.validate_policy()?;
        Ok(facade)
    }
    fn validate_policy(&self) -> Result<(), AuthorizedError> {
        let durable = self
            .inner
            .state()?
            .current_durable_policy()
            .map_err(|_| AuthorizedError::InvalidPolicy)?;
        if durable.scope() != self.inner.scope()
            || self.policy.namespace_policy(self.inner.scope()) != Some(durable)
        {
            return Err(AuthorizedError::InvalidPolicy);
        }
        Ok(())
    }
    fn authorize(
        &self,
        principal: &AuthenticatedPrincipal,
        action: Action,
    ) -> Result<(), AuthorizedError> {
        self.policy
            .authorize(principal, action, Target::Namespace(self.inner.scope()))?;
        self.validate_policy()
    }
    pub fn outcome(
        &self,
        fs: &mut F,
        principal: &AuthenticatedPrincipal,
        key: IdempotencyKey,
        clock: &mut impl Clock,
    ) -> Result<Option<TransactionOutcome>, AuthorizedError> {
        self.authorize(principal, Action::ReadOwnOutcome)?;
        let now = clock
            .observe()
            .map_err(|_| TransactionError::RetryableUnavailable)?
            .wall_utc;
        self.inner
            .outcome(fs, principal.digest(), key, now, lookup_limits())
            .map_err(Into::into)
    }
    pub fn transaction_outcome(
        &self,
        fs: &mut F,
        principal: &AuthenticatedPrincipal,
        id: TransactionId,
        clock: &mut impl Clock,
    ) -> Result<Option<TransactionOutcome>, AuthorizedError> {
        self.authorize(principal, Action::ReadOwnOutcome)?;
        let now = clock
            .observe()
            .map_err(|_| TransactionError::RetryableUnavailable)?
            .wall_utc;
        self.inner
            .transaction_outcome(fs, principal.digest(), id, now, lookup_limits())
            .map_err(Into::into)
    }
    pub fn committed_blob_usage(
        &self,
        fs: &mut F,
        principal: &AuthenticatedPrincipal,
        maximum_total_owners: u64,
    ) -> Result<CommittedBlobUsage, AuthorizedError> {
        self.authorize(principal, Action::InspectQuota)?;
        self.inner
            .committed_blob_usage(
                fs,
                principal.digest(),
                PackedBlobAccountingLimits {
                    lookup: lookup_limits(),
                    maximum_total_owners,
                },
            )
            .map_err(Into::into)
    }
}
// Admitted keys are at most 48 bytes: at most 433 encoded bits, plus leaf and one value chunk.
// Consumer tuning and primitive reports are intentionally absent from this API.
fn lookup_limits() -> TreeLookupLimits {
    TreeLookupLimits {
        maximum_path_branches: 433,
        maximum_pages: 435,
        maximum_encoded_bytes: 435 * 20545,
        maximum_value_bytes: 136,
    }
}
