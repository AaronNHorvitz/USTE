//! Read-only consumer metadata capabilities over the privileged disk coordinator.

use std::sync::Mutex;

mod read;
mod write;
pub use read::{AuthorizedDiskCacheReport, AuthorizedDiskReadState, AuthorizedDiskReader};
pub use write::{AuthorizedDiskWriteError, AuthorizedDiskWriteState, AuthorizedDiskWriter};

use uste_crypto::EntropySource;
use uste_policy::{Action, AuthenticatedPrincipal, NamespacePolicy, PolicyKernel, Target};
use uste_storage::{
    Clock, IndexGetLimits, OwnershipFileSystem, PageCache, journal::DurableKeyEnvelope,
};
use uste_types::{IdempotencyKey, TransactionId};

use crate::{
    ApplyError, AuthorizedError, CommittedBlobUsage, DiskBlobAccountingLimits,
    DiskCommitCoordinator, TransactionError, TransactionOutcome, TransactionState,
};

/// Trusted domain contract: expose the current durable policy without a complete snapshot.
/// Pending/stale domain roots and absent policies must fail closed.
pub trait AuthorizedDiskPolicyState: TransactionState {
    fn current_durable_policy(&self) -> Result<&NamespacePolicy, ApplyError>;
}

/// Borrowed read-only metadata facade. No raw state, owner lookup, maintenance, writes or upload
/// capability escapes. Each operation authorizes against the current exact durable policy.
/// The immutable borrows prevent policy/state changes during a call; create a fresh facade after
/// durable policy publication. This is not the full disk-aware database consumer interface.
pub struct AuthorizedDiskMetadata<'a, S, F, W, E, I>
where
    S: AuthorizedDiskPolicyState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    inner: &'a DiskCommitCoordinator<S, F, W, E, I>,
    policy: &'a PolicyKernel,
    // Consumer-visible cache statistics would disclose privileged query work.
    cache: Mutex<PageCache>,
}

impl<'a, S, F, W, E, I> AuthorizedDiskMetadata<'a, S, F, W, E, I>
where
    S: AuthorizedDiskPolicyState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Trusted adapter setup. Consumers receive this restricted facade, not its raw inputs.
    pub fn new(
        inner: &'a DiskCommitCoordinator<S, F, W, E, I>,
        policy: &'a PolicyKernel,
    ) -> Result<Self, AuthorizedError> {
        Self::new_with_cache_budget(inner, policy, 64 * 1024)
    }

    fn new_with_cache_budget(
        inner: &'a DiskCommitCoordinator<S, F, W, E, I>,
        policy: &'a PolicyKernel,
        cache_bytes: usize,
    ) -> Result<Self, AuthorizedError> {
        let facade = Self {
            inner,
            policy,
            cache: Mutex::new(PageCache::new(cache_bytes).map_err(TransactionError::Storage)?),
        };
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
        // Denial precedes storage, clock and cardinality-sensitive admission work.
        self.policy
            .authorize(principal, action, Target::Namespace(self.inner.scope()))?;
        self.validate_policy()
    }

    pub fn outcome(
        &self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
        key: IdempotencyKey,
        clock: &mut impl Clock,
    ) -> Result<Option<TransactionOutcome>, AuthorizedError> {
        self.authorize(principal, Action::ReadOwnOutcome)?;
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| AuthorizedError::IntegrityFailure)?;
        let now = clock
            .observe()
            .map_err(|_| TransactionError::RetryableUnavailable)?
            .wall_utc;
        self.inner
            .outcome(
                filesystem,
                principal.digest(),
                key,
                now,
                outcome_limits()?,
                &mut cache,
            )
            .map_err(Into::into)
    }

    pub fn transaction_outcome(
        &self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
        transaction: TransactionId,
        clock: &mut impl Clock,
    ) -> Result<Option<TransactionOutcome>, AuthorizedError> {
        self.authorize(principal, Action::ReadOwnOutcome)?;
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| AuthorizedError::IntegrityFailure)?;
        let now = clock
            .observe()
            .map_err(|_| TransactionError::RetryableUnavailable)?
            .wall_utc;
        self.inner
            .transaction_outcome(
                filesystem,
                principal.digest(),
                transaction,
                now,
                outcome_limits()?,
                &mut cache,
            )
            .map_err(Into::into)
    }

    /// Exact committed charges only. Staged reservations are intentionally not represented.
    pub fn committed_blob_usage(
        &self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
        limits: DiskBlobAccountingLimits,
    ) -> Result<CommittedBlobUsage, AuthorizedError> {
        self.authorize(principal, Action::InspectQuota)?;
        self.inner
            .committed_blob_usage(filesystem, principal.digest(), limits)
            .map_err(Into::into)
    }

    /// Exact indexed committed charges, requiring current InspectQuota authority and an
    /// admitted projection. Missing/corrupt caches never become a zero usage estimate.
    pub fn committed_blob_usage_indexed(
        &self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
        maximum_total_owners: u64,
    ) -> Result<CommittedBlobUsage, AuthorizedError> {
        self.authorize(principal, Action::InspectQuota)?;
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| AuthorizedError::IntegrityFailure)?;
        self.inner
            .committed_blob_usage_indexed(
                filesystem,
                principal.digest(),
                maximum_total_owners,
                blob_usage_limits()?,
                &mut cache,
            )
            .map_err(Into::into)
    }
}

// Metadata values are 24 bytes and principal aggregates 16. Fixed conservative page visits
// cover index-v1's hard page ceiling; consumers cannot probe via undersized per-lookup caps.
pub(crate) fn blob_usage_limits() -> Result<IndexGetLimits, AuthorizedError> {
    IndexGetLimits::new(64, 24)
        .map_err(TransactionError::Storage)
        .map_err(Into::into)
}

// Fixed-width v1 outcomes need at most 136 value bytes. The format admits at most 2^24
// pages: binary search plus the fixed-width entry's fragments fit within 64 visits.
// Caller-selected undersized budgets could distinguish another principal's transaction
// from absence before the ownership filter. Do not expose that tuning knob here.
fn outcome_limits() -> Result<IndexGetLimits, AuthorizedError> {
    IndexGetLimits::new(64, 136)
        .map_err(TransactionError::Storage)
        .map_err(Into::into)
}
