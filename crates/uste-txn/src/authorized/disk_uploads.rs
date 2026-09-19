//! Bounded staging reservations over a disk-backed first-owner ledger.
use super::*;
use crate::{AuthorizedDiskPolicyState, DiskBlobAccountingLimits, DiskCommitCoordinator};
use uste_storage::{IndexGetLimits, PageCache};

mod inventory;
pub use inventory::AuthorizedDiskInventoryError;

/// Unknown abandoned staging is never represented as a proven zero usage.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct DiskUploadUsage {
    pub known_usage: QuotaUsage,
    pub staging_complete: bool,
}

impl core::fmt::Debug for DiskUploadUsage {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("DiskUploadUsage([REDACTED])")
    }
}

/// Restricted upload capability. Every construction starts recovery-closed; trusted adapters
/// must retain and reconcile the complete durable token outbox before permitting new uploads.
/// No raw coordinator, committed-owner map or index maintenance is exposed. Ordinary inventory
/// commits require the separate opt-in constructor and a policy-preserving authorized reducer.
pub struct AuthorizedDiskUploads<'a, S, F, W, E, I>
where
    S: AuthorizedDiskPolicyState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    inner: &'a mut DiskCommitCoordinator<S, F, W, E, I>,
    policy: &'a PolicyKernel,
    // Only the <=32 staging reservations are used. The legacy committed map stays empty.
    ledger: Arc<Mutex<QuotaLedger>>,
    allow_new_uploads: bool,
    accounting: DiskBlobAccountingLimits,
    cache: PageCache,
    inventory_limits: Option<uste_storage::journal::DiskBlobAppendLimits>,
}

impl<'a, S, F, W, E, I> AuthorizedDiskUploads<'a, S, F, W, E, I>
where
    S: AuthorizedDiskPolicyState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Trusted adapter setup, not evidence that an empty caller outbox is complete.
    pub fn new(
        inner: &'a mut DiskCommitCoordinator<S, F, W, E, I>,
        policy: &'a PolicyKernel,
        accounting: DiskBlobAccountingLimits,
    ) -> Result<Self, AuthorizedError> {
        let facade = Self {
            inner,
            policy,
            ledger: Arc::new(Mutex::new(QuotaLedger::default())),
            allow_new_uploads: false,
            accounting,
            cache: PageCache::new(64 * 1024).map_err(TransactionError::Storage)?,
            inventory_limits: None,
        };
        facade.validate_policy()?;
        Ok(facade)
    }

    fn scope(&self) -> NamespaceRef {
        self.inner.scope()
    }

    fn validate_policy(&self) -> Result<(), AuthorizedError> {
        let durable = self
            .inner
            .state()?
            .current_durable_policy()
            .map_err(|_| AuthorizedError::InvalidPolicy)?;
        if durable.scope() != self.scope()
            || self.policy.namespace_policy(self.scope()) != Some(durable)
        {
            return Err(AuthorizedError::InvalidPolicy);
        }
        Ok(())
    }

    fn authorize(
        &self,
        principal: &AuthenticatedPrincipal,
        action: Action,
    ) -> Result<AuthorizationLease, AuthorizedError> {
        let lease = self
            .policy
            .authorize(principal, action, Target::Namespace(self.scope()))?;
        self.validate_policy()?;
        Ok(lease)
    }

    /// Exact current committed charges plus this capability's bounded reconciled reservations.
    /// Until complete-outbox reconciliation succeeds, unknown abandoned staging is not quantified.
    pub fn quota_usage(
        &self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
    ) -> Result<DiskUploadUsage, AuthorizedError> {
        self.authorize(principal, Action::InspectQuota)?;
        let mut usage = self.lock_ledger()?.usage(Some(principal.digest()))?;
        let committed =
            self.inner
                .committed_blob_usage(filesystem, principal.digest(), self.accounting)?;
        usage.namespace_committed_bytes = committed.namespace_bytes;
        usage.principal_committed_bytes = committed.principal_bytes;
        Ok(DiskUploadUsage {
            known_usage: usage,
            staging_complete: self.allow_new_uploads,
        })
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

    /// Reopen new-upload admission using the trusted adapter's complete durable token outbox.
    /// Record every token durably before staging bytes. Every supplied token must be committed,
    /// durably aborted, or have no durable bytes. This does not enumerate filesystem orphans.
    /// Finalized but uncommitted blobs remain unresolved until an opted-in inventory commit or
    /// later lifecycle work resolves them. Reconciliation does not physically erase them.
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
        if tokens
            .windows(2)
            .any(|pair| pair[0].upload_id() == pair[1].upload_id())
        {
            return Err(AuthorizedError::IntegrityFailure);
        }
        for token in tokens {
            if token.scope() != self.scope() {
                return Err(AuthorizedError::Unauthorized);
            }
            if self
                .inner
                .committed_blob_metadata(
                    filesystem,
                    token.blob_id(),
                    IndexGetLimits::new(64, 136).map_err(TransactionError::Storage)?,
                    &mut self.cache,
                )?
                .is_some()
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
