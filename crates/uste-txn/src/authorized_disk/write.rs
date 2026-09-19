//! Authorized inventory-free writes with explicit certified-versus-derived publication results.

use super::*;
use crate::{
    AuthorizedTransactionRequest, Cancellation, DiskCommitCheck,
    ExternallyPreparedTransactionState, TransactionRequest,
};
use uste_policy::AuthorizationRequirements;
use uste_storage::{AdapterError, ClockObservation};

/// Trusted domain implementation. The committed policy must include a certified pending change,
/// even when its derived root is not ready for reads. No uncommitted policy may be returned.
/// Domain errors must be content-free: proof diagnostics can expose unauthorized dependencies.
pub trait AuthorizedDiskWriteState<F, W, E, I>: ExternallyPreparedTransactionState + Sized
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    type PrepareLimits;
    type PublishLimits;
    type WriteError;

    fn committed_write_policy(&self) -> Result<&NamespacePolicy, ApplyError>;
    fn write_requirements(request: &[u8]) -> Result<AuthorizationRequirements, ApplyError>;
    fn prepare_disk_write(
        coordinator: &DiskCommitCoordinator<Self, F, W, E, I>,
        filesystem: &mut F,
        request: &[u8],
        limits: &Self::PrepareLimits,
        cache: &mut PageCache,
    ) -> Result<Self::Prepared, Self::WriteError>;
    fn publish_disk_write(
        coordinator: &mut DiskCommitCoordinator<Self, F, W, E, I>,
        filesystem: &mut F,
        outcome: TransactionOutcome,
        limits: &Self::PublishLimits,
    ) -> Result<(), Self::WriteError>;
}

/// A certified commit is never represented as an ordinary pre-commit rejection.
#[derive(Debug, Eq, PartialEq)]
pub enum AuthorizedDiskWriteError<E> {
    Authorization(AuthorizedError),
    Preparation(E),
    CommittedPolicy {
        outcome: TransactionOutcome,
        error: AuthorizedError,
    },
    CommittedPublication {
        outcome: TransactionOutcome,
        error: E,
    },
}

/// Restricted single-writer capability. This increment admits no blob inventories or uploads.
/// Trusted adapters retain responsibility for metadata rebase and recovery/repair orchestration.
pub struct AuthorizedDiskWriter<'a, S, F, W, E, I>
where
    S: AuthorizedDiskWriteState<F, W, E, I>,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    inner: &'a mut DiskCommitCoordinator<S, F, W, E, I>,
    policy: &'a mut PolicyKernel,
    preparation: S::PrepareLimits,
    publication: S::PublishLimits,
    cache: PageCache,
}

impl<'a, S, F, W, E, I> AuthorizedDiskWriter<'a, S, F, W, E, I>
where
    S: AuthorizedDiskWriteState<F, W, E, I>,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    pub fn new(
        inner: &'a mut DiskCommitCoordinator<S, F, W, E, I>,
        policy: &'a mut PolicyKernel,
        preparation: S::PrepareLimits,
        publication: S::PublishLimits,
    ) -> Result<Self, AuthorizedError> {
        Self::new_with_cache_budget(inner, policy, preparation, publication, 64 * 1024)
    }

    /// Trusted adapter configuration under storage's fixed cache cap, never a request override.
    /// Proof and publication work limits remain independent of cache residency.
    pub fn new_with_cache_budget(
        inner: &'a mut DiskCommitCoordinator<S, F, W, E, I>,
        policy: &'a mut PolicyKernel,
        preparation: S::PrepareLimits,
        publication: S::PublishLimits,
        cache_bytes: usize,
    ) -> Result<Self, AuthorizedError> {
        let writer = Self {
            inner,
            policy,
            preparation,
            publication,
            cache: PageCache::new(cache_bytes).map_err(TransactionError::Storage)?,
        };
        writer.validate_policy()?;
        Ok(writer)
    }

    /// Fixed constructor configuration only; exposes no candidate-dependent cache counters.
    #[must_use]
    pub fn cache_budget_bytes(&self) -> usize {
        self.cache.budget()
    }

    fn validate_policy(&self) -> Result<(), AuthorizedError> {
        let durable = self
            .inner
            .state()?
            .committed_write_policy()
            .map_err(|_| AuthorizedError::InvalidPolicy)?;
        if durable.scope() != self.inner.scope()
            || self.policy.namespace_policy(self.inner.scope()) != Some(durable)
        {
            return Err(AuthorizedError::InvalidPolicy);
        }
        Ok(())
    }

    pub fn commit(
        &mut self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
        request: AuthorizedTransactionRequest<'_>,
        clock: &mut impl Clock,
        cancellation: &impl Cancellation,
    ) -> Result<TransactionOutcome, AuthorizedDiskWriteError<S::WriteError>> {
        let auth = AuthorizedDiskWriteError::Authorization;
        self.policy
            .authorize(
                principal,
                Action::Commit,
                Target::Namespace(self.inner.scope()),
            )
            .map_err(AuthorizedError::from)
            .map_err(auth)?;
        self.validate_policy().map_err(auth)?;
        let quota = self
            .policy
            .quotas(principal, self.inner.scope())
            .map_err(AuthorizedError::from)
            .map_err(auth)?;
        let request_bytes = u64::try_from(request.canonical_request.len())
            .map_err(|_| auth(AuthorizedError::ResourceLimit))?;
        if request_bytes > quota.max_request_bytes() {
            return Err(auth(AuthorizedError::ResourceLimit));
        }
        if request.blob_inventory.is_some()
            || request.canonical_request.is_empty()
            || request.canonical_request.len() > crate::MAX_REQUEST_BYTES
        {
            return Err(auth(AuthorizedError::Transaction(
                TransactionError::InvalidRequest,
            )));
        }
        let requirements = S::write_requirements(request.canonical_request)
            .map_err(crate::authorized::map_requirement_error)
            .map_err(auth)?;
        for requirement in requirements.iter() {
            if requirement.target.scope() != self.inner.scope() {
                return Err(auth(AuthorizedError::Unauthorized));
            }
            self.policy
                .authorize(principal, requirement.action, requirement.target)
                .map_err(AuthorizedError::from)
                .map_err(auth)?;
        }
        // Sample once, preserving the normal commit's pre-preparation acceptance time.
        let mut sampled = SampledClock(clock.observe().map_err(|_| {
            auth(AuthorizedError::Transaction(
                TransactionError::RetryableUnavailable,
            ))
        })?);
        let raw = TransactionRequest {
            principal: principal.digest(),
            idempotency_key: request.idempotency_key,
            transaction_id: request.transaction_id,
            canonical_request: request.canonical_request,
            blob_inventory: None,
        };
        let lookup = outcome_limits().map_err(auth)?;
        let checked = self
            .inner
            .check_commit(
                filesystem,
                raw,
                &mut sampled,
                cancellation,
                lookup,
                &mut self.cache,
            )
            .map_err(AuthorizedError::from)
            .map_err(auth)?;
        let outcome = match checked {
            DiskCommitCheck::Retry(outcome) => outcome,
            DiskCommitCheck::Ready { .. } => {
                let prepared = S::prepare_disk_write(
                    self.inner,
                    filesystem,
                    request.canonical_request,
                    &self.preparation,
                    &mut self.cache,
                )
                .map_err(AuthorizedDiskWriteError::Preparation)?;
                self.inner
                    .commit_prepared(
                        filesystem,
                        raw,
                        prepared,
                        &mut sampled,
                        cancellation,
                        lookup,
                        &mut self.cache,
                    )
                    .map_err(AuthorizedError::from)
                    .map_err(auth)?
            }
        };
        // Revocation becomes effective as soon as the journal certifies it, even if root repair fails.
        self.synchronize_policy(principal)
            .map_err(|error| AuthorizedDiskWriteError::CommittedPolicy { outcome, error })?;
        S::publish_disk_write(self.inner, filesystem, outcome, &self.publication)
            .map_err(|error| AuthorizedDiskWriteError::CommittedPublication { outcome, error })?;
        Ok(outcome)
    }

    fn synchronize_policy(
        &mut self,
        principal: &AuthenticatedPrincipal,
    ) -> Result<(), AuthorizedError> {
        let durable = self
            .inner
            .state()?
            .committed_write_policy()
            .map_err(|_| AuthorizedError::InvalidPolicy)?
            .clone();
        let current = self
            .policy
            .namespace_policy(self.inner.scope())
            .ok_or(AuthorizedError::InvalidPolicy)?;
        if current == &durable {
            return Ok(());
        }
        if durable.scope() != self.inner.scope() || durable.version() <= current.version() {
            return Err(AuthorizedError::IntegrityFailure);
        }
        self.policy
            .replace_namespace_policy(principal, current.version(), durable)
            .map_err(Into::into)
    }
}

struct SampledClock(ClockObservation);
impl Clock for SampledClock {
    fn observe(&mut self) -> Result<ClockObservation, AdapterError> {
        Ok(self.0)
    }
}
