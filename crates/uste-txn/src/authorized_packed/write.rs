//! Restricted inventory-free writes; trusted adapters alone configure work and maintenance.
use super::*;
use crate::{
    ApplyError, AuthorizedDiskWriteError, AuthorizedTransactionRequest, Cancellation,
    DiskCommitCheck, ExternallyPreparedTransactionState, PackedCommitLimits, TransactionRequest,
};
use uste_policy::{AuthorizationRequirements, NamespacePolicy};
use uste_storage::{AdapterError, ClockObservation, PageCache};

/// Trusted domain hooks. Policy includes certified pending changes; errors contain no hidden
/// dependency identities/counts. Preparation and publication do not grant caller authorization.
pub trait AuthorizedPackedWriteState<F, W, E, I>:
    PackedCoordinatorState + ExternallyPreparedTransactionState + Sized
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    type PrepareLimits;
    type PublishLimits;
    fn committed_write_policy(&self) -> Result<&NamespacePolicy, ApplyError>;
    fn write_requirements(request: &[u8]) -> Result<AuthorizationRequirements, ApplyError>;
    fn prepare_packed_write(
        coordinator: &mut PackedCommitCoordinator<Self, F, W, E, I>,
        fs: &mut F,
        request: &[u8],
        limits: &Self::PrepareLimits,
    ) -> Result<Self::Prepared, TransactionError>;
    fn publish_packed_write(
        coordinator: &mut PackedCommitCoordinator<Self, F, W, E, I>,
        fs: &mut F,
        outcome: TransactionOutcome,
        limits: &Self::PublishLimits,
    ) -> Result<(), TransactionError>;
}
pub struct AuthorizedPackedWriter<'a, S, F, W, E, I>
where
    S: AuthorizedPackedWriteState<F, W, E, I>,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    inner: &'a mut PackedCommitCoordinator<S, F, W, E, I>,
    policy: &'a mut PolicyKernel,
    preparation: S::PrepareLimits,
    publication: S::PublishLimits,
    // Shared coordinator API scratch; this is not a claim of packed-page caching.
    cache: PageCache,
}
impl<'a, S, F, W, E, I> AuthorizedPackedWriter<'a, S, F, W, E, I>
where
    S: AuthorizedPackedWriteState<F, W, E, I>,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    pub fn new(
        inner: &'a mut PackedCommitCoordinator<S, F, W, E, I>,
        policy: &'a mut PolicyKernel,
        preparation: S::PrepareLimits,
        publication: S::PublishLimits,
    ) -> Result<Self, AuthorizedError> {
        let writer = Self {
            inner,
            policy,
            preparation,
            publication,
            cache: PageCache::new(64 * 1024).map_err(TransactionError::Storage)?,
        };
        writer.validate_policy()?;
        Ok(writer)
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
        fs: &mut F,
        principal: &AuthenticatedPrincipal,
        request: AuthorizedTransactionRequest<'_>,
        clock: &mut impl Clock,
        cancellation: &impl Cancellation,
    ) -> Result<TransactionOutcome, AuthorizedDiskWriteError<TransactionError>> {
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
        if u64::try_from(request.canonical_request.len())
            .map_err(|_| auth(AuthorizedError::ResourceLimit))?
            > quota.max_request_bytes()
        {
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
        let limits = PackedCommitLimits {
            lookup: lookup_limits(),
            storage: None,
        };
        let checked = self
            .inner
            .check_commit(fs, raw, &mut sampled, cancellation, limits, &mut self.cache)
            .map_err(AuthorizedError::from)
            .map_err(auth)?;
        let outcome = match checked {
            DiskCommitCheck::Retry(outcome) => outcome,
            DiskCommitCheck::Ready { .. } => {
                let prepared = S::prepare_packed_write(
                    self.inner,
                    fs,
                    request.canonical_request,
                    &self.preparation,
                )
                .map_err(AuthorizedDiskWriteError::Preparation)?;
                self.inner
                    .commit_prepared(
                        fs,
                        raw,
                        prepared,
                        &mut sampled,
                        cancellation,
                        limits,
                        &mut self.cache,
                    )
                    .map_err(AuthorizedError::from)
                    .map_err(auth)?
            }
        };
        // Certified revocation precedes derived-root repair, including on repair failure.
        self.synchronize_policy(principal)
            .map_err(|error| AuthorizedDiskWriteError::CommittedPolicy { outcome, error })?;
        S::publish_packed_write(self.inner, fs, outcome, &self.publication)
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
