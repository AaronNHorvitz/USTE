//! Domain-owned projections over a ready disk coordinator; no raw state escapes.

use super::*;
use crate::{AuthorizedReadError, Cancellation};
use uste_policy::AuthorizationRequirements;

/// Trusted reducer contract. Implementations must filter embedded/candidate references with
/// `authorize_candidate` and return no partial successful result on error.
pub trait AuthorizedDiskReadState<F, W, E, I>: AuthorizedDiskPolicyState + Sized
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    type ReadRequest;
    type ReadOutput;
    type ReadError;
    type ReadLimits;

    fn read_requirements(
        request: &Self::ReadRequest,
    ) -> Result<AuthorizationRequirements, ApplyError>;

    fn read_disk_authorized(
        coordinator: &DiskCommitCoordinator<Self, F, W, E, I>,
        filesystem: &mut F,
        request: &Self::ReadRequest,
        limits: &Self::ReadLimits,
        cache: &mut PageCache,
        authorize_candidate: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Result<Self::ReadOutput, Self::ReadError>;
}

/// Restricted domain read capability. Cache and work diagnostics remain private.
pub struct AuthorizedDiskReader<'a, S, F, W, E, I>
where
    S: AuthorizedDiskReadState<F, W, E, I>,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    metadata: AuthorizedDiskMetadata<'a, S, F, W, E, I>,
    limits: S::ReadLimits,
}

impl<'a, S, F, W, E, I> AuthorizedDiskReader<'a, S, F, W, E, I>
where
    S: AuthorizedDiskReadState<F, W, E, I>,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Trusted adapter selects resource admission once; callers cannot tune hidden candidate work.
    pub fn new(
        inner: &'a DiskCommitCoordinator<S, F, W, E, I>,
        policy: &'a PolicyKernel,
        limits: S::ReadLimits,
    ) -> Result<Self, AuthorizedError> {
        Ok(Self {
            metadata: AuthorizedDiskMetadata::new(inner, policy)?,
            limits,
        })
    }

    pub fn read(
        &self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
        request: &S::ReadRequest,
        cancellation: &impl Cancellation,
    ) -> Result<S::ReadOutput, AuthorizedReadError<S::ReadError>> {
        let auth = AuthorizedReadError::Authorization;
        self.metadata
            .authorize(principal, Action::ReadRecord)
            .map_err(auth)?;
        let requirements = S::read_requirements(request)
            .map_err(super::super::authorized::map_requirement_error)
            .map_err(auth)?;
        let scope = self.metadata.inner.scope();
        for requirement in requirements.iter() {
            if requirement.target.scope() != scope {
                return Err(auth(AuthorizedError::Unauthorized));
            }
            self.metadata
                .policy
                .authorize(principal, requirement.action, requirement.target)
                .map_err(AuthorizedError::from)
                .map_err(auth)?;
        }
        if cancellation.is_cancelled() {
            return Err(cancelled());
        }
        let mut cache = self
            .metadata
            .cache
            .lock()
            .map_err(|_| auth(AuthorizedError::IntegrityFailure))?;
        let mut observed_cancel = false;
        let mut authorize_candidate = |action: Action, target: Target| {
            observed_cancel |= cancellation.is_cancelled();
            !observed_cancel
                && target.scope() == scope
                && self
                    .metadata
                    .policy
                    .authorize(principal, action, target)
                    .is_ok()
        };
        let output = S::read_disk_authorized(
            self.metadata.inner,
            filesystem,
            request,
            &self.limits,
            &mut cache,
            &mut authorize_candidate,
        );
        if observed_cancel || cancellation.is_cancelled() {
            return Err(cancelled());
        }
        output.map_err(AuthorizedReadError::Domain)
    }
}

fn cancelled<E>() -> AuthorizedReadError<E> {
    AuthorizedReadError::Authorization(AuthorizedError::Transaction(TransactionError::Cancelled))
}
