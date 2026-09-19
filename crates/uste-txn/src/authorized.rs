//! Mandatory policy facade over the trusted raw transaction coordinator.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard},
};

use uste_crypto::{EntropySource, KeyAdapter};
use uste_policy::{
    Action, AuthenticatedPrincipal, AuthorizationLease, AuthorizationRequirements, NamespacePolicy,
    PolicyError, PolicyKernel, PolicyVersion, PrincipalDigest, QuotaLimits, Target,
};
use uste_storage::{
    BlobId, BlobInventory, BlobReference, BlobUpload, BlobUploadToken, Clock, OwnershipFileSystem,
    journal::{DurableKeyEnvelope, RecoveryReport, StorageError},
};
use uste_types::{IdempotencyKey, NamespaceRef, TransactionId};

use crate::{
    ApplyError, Cancellation, CommitCoordinator, ReadView, TransactionError, TransactionOutcome,
    TransactionRequest, TransactionState,
};

/// Maximum unresolved authorized upload reservations retained in one coordinator.
pub const MAX_STAGED_UPLOAD_RESERVATIONS: usize = 32;

/// Reducers admitted to the consumer facade must declare bounded targets without reading state.
pub trait AuthorizedTransactionState: TransactionState {
    /// Whether this reducer requires a durable policy in every authorized snapshot.
    const REQUIRES_DURABLE_POLICY: bool = false;

    fn authorization_requirements(
        canonical_request: &[u8],
        blob_inventory: Option<&BlobInventory>,
    ) -> Result<AuthorizationRequirements, ApplyError>;

    fn durable_namespace_policy(_snapshot: &Self::Snapshot) -> Option<NamespacePolicy> {
        None
    }

    fn durable_policy_change(
        _canonical_request: &[u8],
    ) -> Result<Option<DurablePolicyChange>, ApplyError> {
        Ok(None)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurablePolicyChange {
    pub expected: PolicyVersion,
    pub next: NamespacePolicy,
}

/// Reducer-owned authorized projection. Callers can supply data, but never receive a raw snapshot.
pub trait AuthorizedReadState: AuthorizedTransactionState {
    type ReadRequest;
    type ReadOutput;
    type ReadError;

    fn read_authorization_requirements(
        request: &Self::ReadRequest,
    ) -> Result<AuthorizationRequirements, ApplyError>;

    fn read_authorized(
        snapshot: &Self::Snapshot,
        request: &Self::ReadRequest,
        authorize_candidate: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Result<Self::ReadOutput, Self::ReadError>;
}

/// Reducer-owned optimized index projection used through the same mandatory authorization facade.
/// Implementations receive the raw coordinator only after top-level request authorization succeeds;
/// candidate authorization remains controlled by the reducer implementation rather than callers.
pub trait AuthorizedIndexedReadState<F, W, E, I>: AuthorizedReadState + Sized
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    type IndexRoot;
    type IndexReport;
    type IndexError;

    fn publish_current_index(
        coordinator: &mut CommitCoordinator<Self, F, W, E, I>,
        filesystem: &mut F,
    ) -> Result<Self::IndexRoot, Self::IndexError>;

    fn load_current_index_roots(
        coordinator: &CommitCoordinator<Self, F, W, E, I>,
        filesystem: &mut F,
    ) -> Result<Vec<Self::IndexRoot>, Self::IndexError>;

    fn index_report(root: &Self::IndexRoot) -> Result<Self::IndexReport, Self::IndexError>;

    fn clear_index_cache(root: &Self::IndexRoot) -> Result<(), Self::IndexError>;

    fn read_index_authorized(
        snapshot: &Self::Snapshot,
        coordinator: &CommitCoordinator<Self, F, W, E, I>,
        filesystem: &mut F,
        root: &Self::IndexRoot,
        request: &Self::ReadRequest,
        authorize_candidate: &mut dyn FnMut(Action, Target) -> bool,
    ) -> Result<Self::ReadOutput, Self::IndexError>;
}

/// Opaque derived-index capability bound to the authorized coordinator that issued it.
pub struct AuthorizedIndexRoot<R> {
    inner: R,
    instance: Arc<()>,
}

impl<R: core::fmt::Debug> core::fmt::Debug for AuthorizedIndexRoot<R> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_tuple("AuthorizedIndexRoot")
            .field(&self.inner)
            .finish()
    }
}

/// Content-free failures returned by the mandatory authorization facade.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorizedError {
    Unauthorized,
    ResourceLimit,
    StalePolicy,
    InvalidPolicy,
    IntegrityFailure,
    Transaction(TransactionError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthorizedReadError<E> {
    Authorization(AuthorizedError),
    Domain(E),
}

impl From<PolicyError> for AuthorizedError {
    fn from(error: PolicyError) -> Self {
        match error {
            PolicyError::Unauthorized => Self::Unauthorized,
            PolicyError::ResourceLimit => Self::ResourceLimit,
            PolicyError::InvalidPolicy => Self::InvalidPolicy,
            PolicyError::StalePolicy => Self::StalePolicy,
        }
    }
}

impl From<TransactionError> for AuthorizedError {
    fn from(error: TransactionError) -> Self {
        match error {
            TransactionError::ResourceLimit => Self::ResourceLimit,
            TransactionError::IntegrityFailure => Self::IntegrityFailure,
            other => Self::Transaction(other),
        }
    }
}

/// An external commit request. The journaled principal is derived from authentication, never here.
#[derive(Clone, Copy)]
pub struct AuthorizedTransactionRequest<'a> {
    pub idempotency_key: IdempotencyKey,
    pub transaction_id: TransactionId,
    pub canonical_request: &'a [u8],
    pub blob_inventory: Option<&'a BlobInventory>,
}

impl core::fmt::Debug for AuthorizedTransactionRequest<'_> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("AuthorizedTransactionRequest([REDACTED])")
    }
}

/// A snapshot whose content is inaccessible unless its authorization lease is still current.
pub struct AuthorizedReadView<S> {
    inner: ReadView<S>,
    lease: AuthorizationLease,
    instance: Arc<()>,
}

impl<S> core::fmt::Debug for AuthorizedReadView<S> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("AuthorizedReadView([REDACTED])")
    }
}

/// Current exact logical-byte accounting, exposed only by the trusted coordinator surface.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub struct QuotaUsage {
    pub namespace_committed_bytes: u64,
    pub namespace_staged_bytes: u64,
    pub principal_committed_bytes: u64,
    pub principal_staged_bytes: u64,
    pub principal_live_uploads: u32,
}

impl core::fmt::Debug for QuotaUsage {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("QuotaUsage([REDACTED])")
    }
}

#[derive(Clone, Copy)]
struct CommittedCharge {
    reference: BlobReference,
    principal: PrincipalDigest,
}

#[derive(Clone, Copy)]
struct StagedCharge {
    blob: BlobId,
    principal: PrincipalDigest,
    accepted_bytes: u64,
    finalized: Option<BlobReference>,
    live_handles: u32,
}

#[derive(Default)]
struct QuotaLedger {
    committed: BTreeMap<BlobId, CommittedCharge>,
    staged: BTreeMap<[u8; 16], StagedCharge>,
    staged_by_blob: BTreeMap<BlobId, [u8; 16]>,
}

impl QuotaLedger {
    fn from_coordinator<S, F, W, E, I>(
        coordinator: &CommitCoordinator<S, F, W, E, I>,
    ) -> Result<Self, AuthorizedError>
    where
        S: TransactionState,
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        let mut ledger = Self::default();
        for (reference, principal) in coordinator.committed_blob_owners() {
            if ledger
                .committed
                .insert(
                    reference.id(),
                    CommittedCharge {
                        reference,
                        principal,
                    },
                )
                .is_some()
            {
                return Err(AuthorizedError::IntegrityFailure);
            }
        }
        ledger.usage(None)?;
        Ok(ledger)
    }

    fn usage(&self, principal: Option<PrincipalDigest>) -> Result<QuotaUsage, AuthorizedError> {
        let mut usage = QuotaUsage::default();
        for charge in self.committed.values() {
            usage.namespace_committed_bytes = usage
                .namespace_committed_bytes
                .checked_add(charge.reference.byte_len())
                .ok_or(AuthorizedError::ResourceLimit)?;
            if principal == Some(charge.principal) {
                usage.principal_committed_bytes = usage
                    .principal_committed_bytes
                    .checked_add(charge.reference.byte_len())
                    .ok_or(AuthorizedError::ResourceLimit)?;
            }
        }
        for charge in self.staged.values() {
            usage.namespace_staged_bytes = usage
                .namespace_staged_bytes
                .checked_add(charge.accepted_bytes)
                .ok_or(AuthorizedError::ResourceLimit)?;
            if principal == Some(charge.principal) {
                usage.principal_staged_bytes = usage
                    .principal_staged_bytes
                    .checked_add(charge.accepted_bytes)
                    .ok_or(AuthorizedError::ResourceLimit)?;
                usage.principal_live_uploads = usage
                    .principal_live_uploads
                    .checked_add(charge.live_handles)
                    .ok_or(AuthorizedError::ResourceLimit)?;
            }
        }
        Ok(usage)
    }
}

/// Policy-bound upload handle. The inner storage capability is never exposed.
pub struct AuthorizedBlobUpload {
    inner: BlobUpload,
    principal: PrincipalDigest,
    scope: NamespaceRef,
    upload_id: [u8; 16],
    lease: AuthorizationLease,
    ledger: Arc<Mutex<QuotaLedger>>,
    active_handle: bool,
}

impl core::fmt::Debug for AuthorizedBlobUpload {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("AuthorizedBlobUpload([REDACTED])")
    }
}

impl AuthorizedBlobUpload {
    #[must_use]
    pub const fn token(&self) -> BlobUploadToken {
        self.inner.token()
    }

    #[must_use]
    pub fn accepted_bytes(&self) -> u64 {
        self.inner.accepted_bytes()
    }
}

impl Drop for AuthorizedBlobUpload {
    fn drop(&mut self) {
        if let Ok(mut ledger) = self.ledger.lock()
            && let Some(charge) = ledger.staged.get_mut(&self.upload_id)
            && self.active_handle
        {
            charge.live_handles = charge.live_handles.saturating_sub(1);
        }
    }
}

/// The consumer-facing transaction surface: default deny, current policy and exact-byte quotas.
pub struct AuthorizedCoordinator<S, F, W, E, I>
where
    S: AuthorizedTransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    inner: CommitCoordinator<S, F, W, E, I>,
    policy: PolicyKernel,
    ledger: Arc<Mutex<QuotaLedger>>,
    instance: Arc<()>,
    allow_new_uploads: bool,
}

impl<S, F, W, E, I> AuthorizedCoordinator<S, F, W, E, I>
where
    S: AuthorizedTransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    pub fn new(
        inner: CommitCoordinator<S, F, W, E, I>,
        policy: PolicyKernel,
    ) -> Result<Self, AuthorizedError> {
        let snapshot = inner.read_view()?.state;
        match S::durable_namespace_policy(&snapshot) {
            Some(durable) if policy.namespace_policy(durable.scope()) != Some(&durable) => {
                return Err(AuthorizedError::InvalidPolicy);
            }
            None if S::REQUIRES_DURABLE_POLICY => return Err(AuthorizedError::InvalidPolicy),
            _ => {}
        }
        let ledger = QuotaLedger::from_coordinator(&inner)?;
        let allow_new_uploads = !inner.was_recovered();
        Ok(Self {
            inner,
            policy,
            ledger: Arc::new(Mutex::new(ledger)),
            instance: Arc::new(()),
            allow_new_uploads,
        })
    }

    pub fn replace_namespace_policy(
        &mut self,
        authority: &AuthenticatedPrincipal,
        expected: PolicyVersion,
        next: NamespacePolicy,
    ) -> Result<(), AuthorizedError> {
        if next.scope() != self.scope() {
            return Err(AuthorizedError::Unauthorized);
        }
        self.policy.authorize(
            authority,
            Action::ManagePolicy,
            Target::Namespace(self.scope()),
        )?;
        let snapshot = self.inner.read_view()?.state;
        if S::REQUIRES_DURABLE_POLICY || S::durable_namespace_policy(&snapshot).is_some() {
            return Err(AuthorizedError::InvalidPolicy);
        }
        self.policy
            .replace_namespace_policy(authority, expected, next)
            .map_err(Into::into)
    }

    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.inner.scope()
    }

    pub fn quota_usage(
        &self,
        principal: &AuthenticatedPrincipal,
    ) -> Result<QuotaUsage, AuthorizedError> {
        self.authorize(principal, Action::InspectQuota)?;
        self.lock_ledger()?.usage(Some(principal.digest()))
    }

    pub fn read_view(
        &self,
        principal: &AuthenticatedPrincipal,
    ) -> Result<AuthorizedReadView<S::Snapshot>, AuthorizedError> {
        let lease = self.authorize(principal, Action::ReadRecord)?;
        let inner = self.inner.read_view()?;
        Ok(AuthorizedReadView {
            inner,
            lease,
            instance: Arc::clone(&self.instance),
        })
    }

    pub fn read_view_revision(
        &self,
        view: &AuthorizedReadView<S::Snapshot>,
    ) -> Result<Option<uste_types::CommitRevision>, AuthorizedError> {
        if self.inner.is_uncertain() {
            return Err(AuthorizedError::Transaction(
                TransactionError::OutcomeUnknown,
            ));
        }
        if !Arc::ptr_eq(&self.instance, &view.instance) {
            return Err(AuthorizedError::Unauthorized);
        }
        self.policy.revalidate(&view.lease)?;
        Ok(view.inner.revision())
    }

    pub fn read<R>(
        &self,
        principal: &AuthenticatedPrincipal,
        view: &AuthorizedReadView<S::Snapshot>,
        request: &R,
    ) -> Result<S::ReadOutput, AuthorizedReadError<S::ReadError>>
    where
        S: AuthorizedReadState<ReadRequest = R>,
    {
        self.read_cancellable(principal, view, request, &crate::NeverCancel)
    }

    /// Policy-authorized read with cooperative cancellation before dispatch and while reducer
    /// candidates are authorized. Reducers must route every result candidate through the supplied
    /// authorization callback, so cancellation cannot return a partial result as if it were
    /// complete.
    pub fn read_cancellable<R>(
        &self,
        principal: &AuthenticatedPrincipal,
        view: &AuthorizedReadView<S::Snapshot>,
        request: &R,
        cancellation: &impl Cancellation,
    ) -> Result<S::ReadOutput, AuthorizedReadError<S::ReadError>>
    where
        S: AuthorizedReadState<ReadRequest = R>,
    {
        if cancellation.is_cancelled() {
            return Err(cancelled_read());
        }
        if self.inner.is_uncertain() {
            return Err(AuthorizedReadError::Authorization(
                AuthorizedError::Transaction(TransactionError::OutcomeUnknown),
            ));
        }
        if !Arc::ptr_eq(&self.instance, &view.instance) {
            return Err(AuthorizedReadError::Authorization(
                AuthorizedError::Unauthorized,
            ));
        }
        self.policy
            .revalidate(&view.lease)
            .map_err(AuthorizedError::from)
            .map_err(AuthorizedReadError::Authorization)?;
        let requirements = S::read_authorization_requirements(request)
            .map_err(map_requirement_error)
            .map_err(AuthorizedReadError::Authorization)?;
        for requirement in requirements.iter() {
            if cancellation.is_cancelled() {
                return Err(cancelled_read());
            }
            if requirement.target.scope() != self.scope() {
                return Err(AuthorizedReadError::Authorization(
                    AuthorizedError::Unauthorized,
                ));
            }
            self.policy
                .authorize(principal, requirement.action, requirement.target)
                .map_err(AuthorizedError::from)
                .map_err(AuthorizedReadError::Authorization)?;
        }
        let mut cancelled = false;
        let result = {
            let mut authorize_candidate = |action: Action, target: Target| {
                if cancellation.is_cancelled() {
                    cancelled = true;
                    return false;
                }
                target.scope() == self.scope()
                    && self.policy.authorize(principal, action, target).is_ok()
            };
            S::read_authorized(&view.inner.state, request, &mut authorize_candidate)
        };
        if cancelled || cancellation.is_cancelled() {
            return Err(cancelled_read());
        }
        result.map_err(AuthorizedReadError::Domain)
    }

    /// Policy-authorized maintenance entry point for the reducer's current derived-index
    /// profile. Authorization is resolved before reducer code or storage I/O can run.
    pub fn publish_current_index(
        &mut self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
    ) -> Result<AuthorizedIndexRoot<S::IndexRoot>, AuthorizedReadError<S::IndexError>>
    where
        S: AuthorizedIndexedReadState<F, W, E, I>,
    {
        self.authorize(principal, Action::ManageSchema)
            .map_err(AuthorizedReadError::Authorization)?;
        let inner = S::publish_current_index(&mut self.inner, filesystem)
            .map_err(AuthorizedReadError::Domain)?;
        Ok(AuthorizedIndexRoot {
            inner,
            instance: Arc::clone(&self.instance),
        })
    }

    /// Policy-authorized discovery of current derived roots. Authorization is resolved before
    /// reducer code or storage I/O can run.
    pub fn load_current_index_roots(
        &self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
    ) -> Result<Vec<AuthorizedIndexRoot<S::IndexRoot>>, AuthorizedReadError<S::IndexError>>
    where
        S: AuthorizedIndexedReadState<F, W, E, I>,
    {
        self.authorize(principal, Action::ManageSchema)
            .map_err(AuthorizedReadError::Authorization)?;
        S::load_current_index_roots(&self.inner, filesystem)
            .map(|roots| {
                roots
                    .into_iter()
                    .map(|inner| AuthorizedIndexRoot {
                        inner,
                        instance: Arc::clone(&self.instance),
                    })
                    .collect()
            })
            .map_err(AuthorizedReadError::Domain)
    }

    /// Return privileged, cardinality-sensitive index diagnostics after current maintenance
    /// authorization and issuer-instance validation.
    pub fn index_report(
        &self,
        principal: &AuthenticatedPrincipal,
        root: &AuthorizedIndexRoot<S::IndexRoot>,
    ) -> Result<S::IndexReport, AuthorizedReadError<S::IndexError>>
    where
        S: AuthorizedIndexedReadState<F, W, E, I>,
    {
        if self.inner.is_uncertain() {
            return Err(AuthorizedReadError::Authorization(
                AuthorizedError::Transaction(TransactionError::OutcomeUnknown),
            ));
        }
        if !Arc::ptr_eq(&self.instance, &root.instance) {
            return Err(AuthorizedReadError::Authorization(
                AuthorizedError::Unauthorized,
            ));
        }
        self.authorize(principal, Action::ManageSchema)
            .map_err(AuthorizedReadError::Authorization)?;
        S::index_report(&root.inner).map_err(AuthorizedReadError::Domain)
    }

    /// Clear the derived index's userspace cache after current maintenance authorization and
    /// issuer-instance validation.
    pub fn clear_index_cache(
        &self,
        principal: &AuthenticatedPrincipal,
        root: &AuthorizedIndexRoot<S::IndexRoot>,
    ) -> Result<(), AuthorizedReadError<S::IndexError>>
    where
        S: AuthorizedIndexedReadState<F, W, E, I>,
    {
        if self.inner.is_uncertain() {
            return Err(AuthorizedReadError::Authorization(
                AuthorizedError::Transaction(TransactionError::OutcomeUnknown),
            ));
        }
        if !Arc::ptr_eq(&self.instance, &root.instance) {
            return Err(AuthorizedReadError::Authorization(
                AuthorizedError::Unauthorized,
            ));
        }
        self.authorize(principal, Action::ManageSchema)
            .map_err(AuthorizedReadError::Authorization)?;
        S::clear_index_cache(&root.inner).map_err(AuthorizedReadError::Domain)
    }

    /// Execute an optimized reducer-owned projection under the same lease and per-candidate
    /// authorization rules as the reference in-memory projection.
    pub fn read_indexed<R>(
        &self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
        view: &AuthorizedReadView<S::Snapshot>,
        root: &AuthorizedIndexRoot<S::IndexRoot>,
        request: &R,
    ) -> Result<S::ReadOutput, AuthorizedReadError<S::IndexError>>
    where
        S: AuthorizedIndexedReadState<F, W, E, I, ReadRequest = R>,
    {
        if self.inner.is_uncertain() {
            return Err(AuthorizedReadError::Authorization(
                AuthorizedError::Transaction(TransactionError::OutcomeUnknown),
            ));
        }
        if !Arc::ptr_eq(&self.instance, &view.instance) {
            return Err(AuthorizedReadError::Authorization(
                AuthorizedError::Unauthorized,
            ));
        }
        if !Arc::ptr_eq(&self.instance, &root.instance) {
            return Err(AuthorizedReadError::Authorization(
                AuthorizedError::Unauthorized,
            ));
        }
        self.policy
            .revalidate(&view.lease)
            .map_err(AuthorizedError::from)
            .map_err(AuthorizedReadError::Authorization)?;
        let requirements = S::read_authorization_requirements(request)
            .map_err(map_requirement_error)
            .map_err(AuthorizedReadError::Authorization)?;
        for requirement in requirements.iter() {
            if requirement.target.scope() != self.scope() {
                return Err(AuthorizedReadError::Authorization(
                    AuthorizedError::Unauthorized,
                ));
            }
            self.policy
                .authorize(principal, requirement.action, requirement.target)
                .map_err(AuthorizedError::from)
                .map_err(AuthorizedReadError::Authorization)?;
        }
        let mut authorize_candidate = |action: Action, target: Target| {
            target.scope() == self.scope()
                && self.policy.authorize(principal, action, target).is_ok()
        };
        S::read_index_authorized(
            &view.inner.state,
            &self.inner,
            filesystem,
            &root.inner,
            request,
            &mut authorize_candidate,
        )
        .map_err(AuthorizedReadError::Domain)
    }

    pub fn start_blob_upload(
        &mut self,
        principal: &AuthenticatedPrincipal,
    ) -> Result<AuthorizedBlobUpload, AuthorizedError> {
        let lease = self.authorize(principal, Action::StartUpload)?;
        if !self.allow_new_uploads {
            return Err(AuthorizedError::ResourceLimit);
        }
        let quotas = self.quotas(principal)?;
        let namespace_quotas = self.policy.namespace_quotas(self.scope())?;
        let usage = self.lock_ledger()?.usage(Some(principal.digest()))?;
        if usage.principal_live_uploads >= quotas.max_live_uploads()
            || self.live_uploads()? >= namespace_quotas.max_live_uploads()
            || self.lock_ledger()?.staged.len() >= MAX_STAGED_UPLOAD_RESERVATIONS
        {
            return Err(AuthorizedError::ResourceLimit);
        }
        let inner = self.inner.start_blob_upload(self.scope())?;
        let token = inner.token();
        let mut ledger = self.lock_ledger()?;
        if ledger.staged.contains_key(&token.upload_id())
            || ledger.staged_by_blob.contains_key(&token.blob_id())
        {
            return Err(AuthorizedError::IntegrityFailure);
        }
        ledger.staged.insert(
            token.upload_id(),
            StagedCharge {
                blob: token.blob_id(),
                principal: principal.digest(),
                accepted_bytes: 0,
                finalized: None,
                live_handles: 1,
            },
        );
        ledger
            .staged_by_blob
            .insert(token.blob_id(), token.upload_id());
        drop(ledger);
        Ok(AuthorizedBlobUpload {
            inner,
            principal: principal.digest(),
            scope: self.scope(),
            upload_id: token.upload_id(),
            lease,
            ledger: Arc::clone(&self.ledger),
            active_handle: true,
        })
    }

    /// Enable new uploads after a trusted derived-index adapter reconciles its complete durable
    /// upload outbox following reopen.
    ///
    /// The adapter must durably record every token before writing staging bytes and pass that
    /// complete set here. Each token must now be committed, durably aborted, or have no durable
    /// bytes. A resumable uncommitted token keeps the coordinator closed to new uploads. This is a
    /// bounded recovery protocol, not filesystem-wide orphan enumeration or garbage collection.
    pub fn complete_recovered_upload_reconciliation(
        &mut self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
        complete_outbox: &[BlobUploadToken],
    ) -> Result<(), AuthorizedError> {
        self.authorize(principal, Action::ManageSchema)?;
        self.authorize(principal, Action::ResumeUpload)?;
        let maximum_outbox = usize::try_from(self.quotas(principal)?.max_live_uploads())
            .map_err(|_| AuthorizedError::ResourceLimit)?;
        if complete_outbox.len() > MAX_STAGED_UPLOAD_RESERVATIONS
            || complete_outbox.len() > maximum_outbox
            || !self.lock_ledger()?.staged.is_empty()
        {
            return Err(AuthorizedError::ResourceLimit);
        }
        let mut tokens = complete_outbox.to_vec();
        tokens.sort_unstable_by_key(|token| token.upload_id());
        if tokens.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(AuthorizedError::IntegrityFailure);
        }
        for token in tokens {
            if token.scope() != self.scope() {
                return Err(AuthorizedError::Unauthorized);
            }
            if self
                .inner
                .committed_blob_owners()
                .any(|(reference, _)| reference.id() == token.blob_id())
            {
                continue;
            }
            match self.inner.resume_blob_upload(filesystem, token) {
                Ok(upload) if !upload.has_durable_resume_evidence() => {}
                Ok(_) => return Err(AuthorizedError::ResourceLimit),
                Err(TransactionError::Storage(StorageError::InvalidState)) => {}
                Err(error) => return Err(error.into()),
            }
        }
        self.allow_new_uploads = true;
        Ok(())
    }

    pub fn resume_blob_upload(
        &mut self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
        token: BlobUploadToken,
    ) -> Result<AuthorizedBlobUpload, AuthorizedError> {
        let lease = self.authorize(principal, Action::ResumeUpload)?;
        if token.scope() != self.scope() {
            return Err(AuthorizedError::Unauthorized);
        }
        let ledger = self.lock_ledger()?;
        let known = ledger.staged.contains_key(&token.upload_id());
        if let Some(charge) = ledger.staged.get(&token.upload_id()) {
            if charge.principal != principal.digest() {
                return Err(AuthorizedError::Unauthorized);
            }
            if charge.live_handles != 0 {
                return Err(AuthorizedError::ResourceLimit);
            }
        } else if ledger.staged.len() >= MAX_STAGED_UPLOAD_RESERVATIONS {
            return Err(AuthorizedError::ResourceLimit);
        }
        if !known && ledger.staged_by_blob.contains_key(&token.blob_id()) {
            return Err(AuthorizedError::IntegrityFailure);
        }
        drop(ledger);
        let quotas = self.quotas(principal)?;
        let namespace_quotas = self.policy.namespace_quotas(self.scope())?;
        let usage = self.lock_ledger()?.usage(Some(principal.digest()))?;
        if usage.principal_live_uploads >= quotas.max_live_uploads()
            || self.live_uploads()? >= namespace_quotas.max_live_uploads()
        {
            return Err(AuthorizedError::ResourceLimit);
        }
        let inner = self.inner.resume_blob_upload(filesystem, token)?;
        if !known && !inner.has_durable_resume_evidence() {
            return Err(AuthorizedError::Unauthorized);
        }
        let accepted_bytes = inner.accepted_bytes();
        let mut ledger = self.lock_ledger()?;
        if !known {
            ledger.staged.insert(
                token.upload_id(),
                StagedCharge {
                    blob: token.blob_id(),
                    principal: principal.digest(),
                    accepted_bytes,
                    finalized: None,
                    live_handles: 0,
                },
            );
            ledger
                .staged_by_blob
                .insert(token.blob_id(), token.upload_id());
        }
        let charge = ledger
            .staged
            .get_mut(&token.upload_id())
            .ok_or(AuthorizedError::IntegrityFailure)?;
        charge.accepted_bytes = accepted_bytes;
        charge.live_handles = charge
            .live_handles
            .checked_add(1)
            .ok_or(AuthorizedError::ResourceLimit)?;
        drop(ledger);
        Ok(AuthorizedBlobUpload {
            inner,
            principal: principal.digest(),
            scope: self.scope(),
            upload_id: token.upload_id(),
            lease,
            ledger: Arc::clone(&self.ledger),
            active_handle: true,
        })
    }

    pub fn write_blob_upload(
        &mut self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
        upload: &mut AuthorizedBlobUpload,
        input: &[u8],
    ) -> Result<(), AuthorizedError> {
        self.validate_upload(principal, upload, Action::WriteUpload)?;
        let before = upload.inner.accepted_bytes();
        let added = u64::try_from(input.len()).map_err(|_| AuthorizedError::ResourceLimit)?;
        self.check_staged_projection(principal, added)?;
        let result = self
            .inner
            .write_blob_upload(filesystem, &mut upload.inner, input);
        let after = upload.inner.accepted_bytes();
        if after < before
            || after
                > before
                    .checked_add(added)
                    .ok_or(AuthorizedError::ResourceLimit)?
        {
            return Err(AuthorizedError::IntegrityFailure);
        }
        let mut ledger = self.lock_ledger()?;
        let charge = ledger
            .staged
            .get_mut(&upload.upload_id)
            .ok_or(AuthorizedError::IntegrityFailure)?;
        if after < charge.accepted_bytes {
            return Err(AuthorizedError::IntegrityFailure);
        }
        charge.accepted_bytes = after;
        drop(ledger);
        result.map_err(Into::into)
    }

    pub fn finish_blob_upload(
        &mut self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
        upload: &mut AuthorizedBlobUpload,
    ) -> Result<BlobReference, AuthorizedError> {
        self.validate_upload(principal, upload, Action::FinishUpload)?;
        let reference = self
            .inner
            .finish_blob_upload(filesystem, &mut upload.inner)?;
        let mut ledger = self.lock_ledger()?;
        let charge = ledger
            .staged
            .get_mut(&upload.upload_id)
            .ok_or(AuthorizedError::IntegrityFailure)?;
        if charge.blob != reference.id() || charge.accepted_bytes != reference.byte_len() {
            return Err(AuthorizedError::IntegrityFailure);
        }
        charge.finalized = Some(reference);
        charge.live_handles = charge.live_handles.saturating_sub(1);
        upload.active_handle = false;
        Ok(reference)
    }

    pub fn abort_blob_upload(
        &mut self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
        upload: &mut AuthorizedBlobUpload,
    ) -> Result<(), AuthorizedError> {
        self.validate_upload(principal, upload, Action::AbortUpload)?;
        self.inner
            .abort_blob_upload(filesystem, &mut upload.inner)?;
        let mut ledger = self.lock_ledger()?;
        if let Some(charge) = ledger.staged.remove(&upload.upload_id) {
            ledger.staged_by_blob.remove(&charge.blob);
        }
        drop(ledger);
        upload.active_handle = false;
        Ok(())
    }

    pub fn read_blob_range(
        &self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
        reference: BlobReference,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, AuthorizedError> {
        self.authorize(principal, Action::ReadBlob)?;
        if reference.scope() != self.scope() {
            return Err(AuthorizedError::Unauthorized);
        }
        let requested = u32::try_from(output.len()).map_err(|_| AuthorizedError::ResourceLimit)?;
        if requested > self.quotas(principal)?.max_blob_read_bytes_per_call() {
            return Err(AuthorizedError::ResourceLimit);
        }
        self.inner
            .read_blob_range(filesystem, reference, offset, output)
            .map_err(Into::into)
    }

    pub fn outcome(
        &self,
        principal: &AuthenticatedPrincipal,
        key: IdempotencyKey,
        clock: &mut impl Clock,
    ) -> Result<Option<TransactionOutcome>, AuthorizedError> {
        self.authorize(principal, Action::ReadOwnOutcome)?;
        self.inner
            .outcome(principal.digest(), key, clock)
            .map_err(Into::into)
    }

    pub fn transaction_outcome(
        &self,
        principal: &AuthenticatedPrincipal,
        transaction: TransactionId,
        clock: &mut impl Clock,
    ) -> Result<Option<TransactionOutcome>, AuthorizedError> {
        self.authorize(principal, Action::ReadOwnOutcome)?;
        self.inner
            .transaction_outcome_for(principal.digest(), transaction, clock)
            .map_err(Into::into)
    }

    pub fn commit(
        &mut self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
        request: AuthorizedTransactionRequest<'_>,
        clock: &mut impl Clock,
        cancellation: &impl Cancellation,
    ) -> Result<TransactionOutcome, AuthorizedError> {
        self.authorize(principal, Action::Commit)?;
        let request_len = u64::try_from(request.canonical_request.len())
            .map_err(|_| AuthorizedError::ResourceLimit)?;
        if request_len > self.quotas(principal)?.max_request_bytes() {
            return Err(AuthorizedError::ResourceLimit);
        }
        let requirements =
            S::authorization_requirements(request.canonical_request, request.blob_inventory)
                .map_err(map_requirement_error)?;
        for requirement in requirements.iter() {
            if requirement.target.scope() != self.scope() {
                return Err(AuthorizedError::Unauthorized);
            }
            self.policy
                .authorize(principal, requirement.action, requirement.target)?;
        }
        self.check_inventory(principal, request.blob_inventory)?;
        let changes_policy =
            S::durable_policy_change(request.canonical_request).map_err(map_requirement_error)?;
        let result = self.inner.commit(
            filesystem,
            TransactionRequest {
                principal: principal.digest(),
                idempotency_key: request.idempotency_key,
                transaction_id: request.transaction_id,
                canonical_request: request.canonical_request,
                blob_inventory: request.blob_inventory,
            },
            clock,
            cancellation,
        );
        let outcome = result?;
        if let Some(inventory) = request.blob_inventory {
            self.commit_inventory(principal.digest(), inventory)?;
        }
        if changes_policy.is_some() {
            self.synchronize_durable_policy(principal)?;
        }
        Ok(outcome)
    }

    fn synchronize_durable_policy(
        &mut self,
        principal: &AuthenticatedPrincipal,
    ) -> Result<(), AuthorizedError> {
        let snapshot = self.inner.read_view()?.state;
        let Some(durable) = S::durable_namespace_policy(&snapshot) else {
            return if S::REQUIRES_DURABLE_POLICY {
                Err(AuthorizedError::IntegrityFailure)
            } else {
                Ok(())
            };
        };
        let Some(current) = self.policy.namespace_policy(durable.scope()) else {
            return Err(AuthorizedError::InvalidPolicy);
        };
        if current == &durable {
            return Ok(());
        }
        if durable.version() <= current.version() {
            return Err(AuthorizedError::IntegrityFailure);
        }
        self.policy
            .replace_namespace_policy(principal, current.version(), durable)
            .map_err(Into::into)
    }

    fn authorize(
        &self,
        principal: &AuthenticatedPrincipal,
        action: Action,
    ) -> Result<AuthorizationLease, AuthorizedError> {
        self.policy
            .authorize(principal, action, Target::Namespace(self.scope()))
            .map_err(Into::into)
    }

    fn quotas(&self, principal: &AuthenticatedPrincipal) -> Result<QuotaLimits, AuthorizedError> {
        self.policy
            .quotas(principal, self.scope())
            .map_err(Into::into)
    }

    fn validate_upload(
        &self,
        principal: &AuthenticatedPrincipal,
        upload: &AuthorizedBlobUpload,
        action: Action,
    ) -> Result<(), AuthorizedError> {
        if upload.scope != self.scope() || upload.principal != principal.digest() {
            return Err(AuthorizedError::Unauthorized);
        }
        if !Arc::ptr_eq(&upload.ledger, &self.ledger) {
            return Err(AuthorizedError::Unauthorized);
        }
        self.policy.revalidate(&upload.lease)?;
        self.authorize(principal, action)?;
        Ok(())
    }

    fn check_staged_projection(
        &self,
        principal: &AuthenticatedPrincipal,
        added: u64,
    ) -> Result<(), AuthorizedError> {
        let usage = self.lock_ledger()?.usage(Some(principal.digest()))?;
        let principal_projected = usage
            .principal_staged_bytes
            .checked_add(added)
            .ok_or(AuthorizedError::ResourceLimit)?;
        let namespace_projected = usage
            .namespace_staged_bytes
            .checked_add(added)
            .ok_or(AuthorizedError::ResourceLimit)?;
        if principal_projected > self.quotas(principal)?.max_staged_blob_bytes()
            || namespace_projected
                > self
                    .policy
                    .namespace_quotas(self.scope())?
                    .max_staged_blob_bytes()
        {
            return Err(AuthorizedError::ResourceLimit);
        }
        Ok(())
    }

    fn check_inventory(
        &self,
        principal: &AuthenticatedPrincipal,
        inventory: Option<&BlobInventory>,
    ) -> Result<(), AuthorizedError> {
        let Some(inventory) = inventory else {
            return Ok(());
        };
        if inventory.scope() != self.scope() {
            return Err(AuthorizedError::Unauthorized);
        }
        let ledger = self.lock_ledger()?;
        let mut added_bytes = 0_u64;
        for reference in inventory.references() {
            if let Some(committed) = ledger.committed.get(&reference.id()) {
                if committed.reference != *reference || committed.principal != principal.digest() {
                    return Err(AuthorizedError::Unauthorized);
                }
                continue;
            }
            let staged = ledger
                .staged_by_blob
                .get(&reference.id())
                .and_then(|upload| ledger.staged.get(upload));
            if !staged.is_some_and(|charge| {
                charge.principal == principal.digest() && charge.finalized == Some(*reference)
            }) {
                return Err(AuthorizedError::Unauthorized);
            }
            added_bytes = added_bytes
                .checked_add(reference.byte_len())
                .ok_or(AuthorizedError::ResourceLimit)?;
        }
        let usage = ledger.usage(Some(principal.digest()))?;
        drop(ledger);
        if usage
            .principal_committed_bytes
            .checked_add(added_bytes)
            .ok_or(AuthorizedError::ResourceLimit)?
            > self.quotas(principal)?.max_committed_blob_bytes()
            || usage
                .namespace_committed_bytes
                .checked_add(added_bytes)
                .ok_or(AuthorizedError::ResourceLimit)?
                > self
                    .policy
                    .namespace_quotas(self.scope())?
                    .max_committed_blob_bytes()
        {
            return Err(AuthorizedError::ResourceLimit);
        }
        Ok(())
    }

    fn commit_inventory(
        &self,
        principal: PrincipalDigest,
        inventory: &BlobInventory,
    ) -> Result<(), AuthorizedError> {
        let mut ledger = self.lock_ledger()?;
        for reference in inventory.references() {
            ledger
                .committed
                .entry(reference.id())
                .or_insert(CommittedCharge {
                    reference: *reference,
                    principal,
                });
            let upload = ledger.staged_by_blob.get(&reference.id()).copied();
            if let Some(upload) = upload {
                ledger.staged.remove(&upload);
                ledger.staged_by_blob.remove(&reference.id());
            }
        }
        Ok(())
    }

    fn live_uploads(&self) -> Result<u32, AuthorizedError> {
        self.lock_ledger()?
            .staged
            .values()
            .try_fold(0_u32, |count, charge| {
                count
                    .checked_add(charge.live_handles)
                    .ok_or(AuthorizedError::ResourceLimit)
            })
    }

    fn lock_ledger(&self) -> Result<MutexGuard<'_, QuotaLedger>, AuthorizedError> {
        self.ledger
            .lock()
            .map_err(|_| AuthorizedError::IntegrityFailure)
    }
}

/// Open a recovered raw coordinator and immediately bind the mandatory policy facade.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
pub fn open_authorized<S, F, W, E, I, A>(
    filesystem: &mut F,
    final_name: &uste_storage::EntryName,
    scope: NamespaceRef,
    retention: crate::RetentionDays,
    vault_entropy: E,
    identity_entropy: I,
    key_adapter: &mut A,
    initial_state: S,
    policy: PolicyKernel,
) -> Result<(AuthorizedCoordinator<S, F, W, E, I>, RecoveryReport), AuthorizedError>
where
    S: AuthorizedTransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
    A: KeyAdapter<Envelope = W>,
{
    let (coordinator, report) = CommitCoordinator::open(
        filesystem,
        final_name,
        scope,
        retention,
        vault_entropy,
        identity_entropy,
        key_adapter,
        initial_state,
    )?;
    Ok((AuthorizedCoordinator::new(coordinator, policy)?, report))
}

pub(crate) const fn map_requirement_error(error: ApplyError) -> AuthorizedError {
    AuthorizedError::Transaction(match error {
        ApplyError::Conflict => TransactionError::Conflict,
        ApplyError::SourceChanged => TransactionError::SourceChanged,
        ApplyError::InvalidRequest => TransactionError::InvalidRequest,
        ApplyError::ResourceLimit => TransactionError::ResourceLimit,
        ApplyError::UnsupportedPredicate => TransactionError::UnsupportedPredicate,
    })
}

const fn cancelled_read<E>() -> AuthorizedReadError<E> {
    AuthorizedReadError::Authorization(AuthorizedError::Transaction(TransactionError::Cancelled))
}
