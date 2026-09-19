//! Restricted domain reads. Trusted implementations own filtering and bounded work.
use super::*;
use crate::{ApplyError, AuthorizedReadError, Cancellation};
use uste_policy::AuthorizationRequirements;

pub trait AuthorizedPackedReadState<F, W, E, I>:
    PackedCoordinatorState + AuthorizedDiskPolicyState + Sized
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
    /// Filter embedded/candidate references through the callback; never return partial success.
    fn read_packed_authorized(
        coordinator: &PackedCommitCoordinator<Self, F, W, E, I>,
        fs: &mut F,
        request: &Self::ReadRequest,
        limits: &Self::ReadLimits,
        authorize_candidate: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Result<Self::ReadOutput, Self::ReadError>;
}
pub struct AuthorizedPackedReader<'a, S, F, W, E, I>
where
    S: AuthorizedPackedReadState<F, W, E, I>,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    metadata: AuthorizedPackedMetadata<'a, S, F, W, E, I>,
    limits: S::ReadLimits,
}
impl<'a, S, F, W, E, I> AuthorizedPackedReader<'a, S, F, W, E, I>
where
    S: AuthorizedPackedReadState<F, W, E, I>,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    pub fn new(
        inner: &'a PackedCommitCoordinator<S, F, W, E, I>,
        policy: &'a PolicyKernel,
        limits: S::ReadLimits,
    ) -> Result<Self, AuthorizedError> {
        Ok(Self {
            metadata: AuthorizedPackedMetadata::new(inner, policy)?,
            limits,
        })
    }
    pub fn read(
        &self,
        fs: &mut F,
        principal: &AuthenticatedPrincipal,
        request: &S::ReadRequest,
        cancellation: &impl Cancellation,
    ) -> Result<S::ReadOutput, AuthorizedReadError<S::ReadError>> {
        let auth = AuthorizedReadError::Authorization;
        self.metadata
            .authorize(principal, Action::ReadRecord)
            .map_err(auth)?;
        let requirements = S::read_requirements(request)
            .map_err(crate::authorized::map_requirement_error)
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
        let output = S::read_packed_authorized(
            self.metadata.inner,
            fs,
            request,
            &self.limits,
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
