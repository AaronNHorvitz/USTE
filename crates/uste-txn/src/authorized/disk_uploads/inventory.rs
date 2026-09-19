//! Ordinary, policy-preserving inventory commits sharing the bounded upload reservation ledger.
use super::*;
use uste_storage::journal::DiskBlobAppendLimits;

/// A known certified outcome is never disguised as an ordinary rejection after publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorizedDiskInventoryError {
    Authorization(AuthorizedError),
    CommittedPolicy {
        outcome: TransactionOutcome,
        error: AuthorizedError,
    },
}

impl<'a, S, F, W, E, I> AuthorizedDiskUploads<'a, S, F, W, E, I>
where
    S: AuthorizedDiskPolicyState + AuthorizedTransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Trusted opt-in configuration. Consumers cannot choose storage or accounting limits per
    /// request. This capability accepts only ordinary reducers and policy-preserving requests.
    pub fn new_with_inventory_commits(
        inner: &'a mut DiskCommitCoordinator<S, F, W, E, I>,
        policy: &'a PolicyKernel,
        accounting: DiskBlobAccountingLimits,
        storage: DiskBlobAppendLimits,
    ) -> Result<Self, AuthorizedError> {
        let mut facade = Self::new(inner, policy, accounting)?;
        facade.inventory_limits = Some(storage);
        Ok(facade)
    }

    /// Authorize current targets and first ownership; project exact committed quotas; then
    /// certify and remove only this principal's exact finalized reservations. No complete
    /// committed ledger is reconstructed, and no fallible charge transfer follows certification.
    pub fn commit_inventory(
        &mut self,
        filesystem: &mut F,
        principal: &AuthenticatedPrincipal,
        request: AuthorizedTransactionRequest<'_>,
        clock: &mut impl Clock,
        cancellation: &impl Cancellation,
    ) -> Result<TransactionOutcome, AuthorizedDiskInventoryError> {
        let auth = AuthorizedDiskInventoryError::Authorization;
        self.authorize(principal, Action::Commit).map_err(auth)?;
        let storage = self.inventory_limits.ok_or_else(|| {
            auth(AuthorizedError::Transaction(
                TransactionError::InvalidRequest,
            ))
        })?;
        let quotas = self.quotas(principal).map_err(auth)?;
        let namespace_quotas = self
            .policy
            .namespace_quotas(self.scope())
            .map_err(AuthorizedError::from)
            .map_err(auth)?;
        if request.canonical_request.is_empty()
            || request.canonical_request.len() > crate::MAX_REQUEST_BYTES
            || request.blob_inventory.is_some_and(BlobInventory::is_empty)
        {
            return Err(auth(AuthorizedError::Transaction(
                TransactionError::InvalidRequest,
            )));
        }
        if request.canonical_request.len() as u64 > quotas.max_request_bytes() {
            return Err(auth(AuthorizedError::ResourceLimit));
        }
        if request
            .blob_inventory
            .is_some_and(|inventory| inventory.scope() != self.scope())
        {
            return Err(auth(AuthorizedError::Unauthorized));
        }
        if S::durable_policy_change(request.canonical_request)
            .map_err(map_requirement_error)
            .map_err(auth)?
            .is_some()
        {
            return Err(auth(AuthorizedError::Transaction(
                TransactionError::InvalidRequest,
            )));
        }
        let requirements =
            S::authorization_requirements(request.canonical_request, request.blob_inventory)
                .map_err(map_requirement_error)
                .map_err(auth)?;
        for requirement in requirements.iter() {
            if requirement.target.scope() != self.scope() {
                return Err(auth(AuthorizedError::Unauthorized));
            }
            self.policy
                .authorize(principal, requirement.action, requirement.target)
                .map_err(AuthorizedError::from)
                .map_err(auth)?;
        }
        let lookup = IndexGetLimits::new(64, 136)
            .map_err(TransactionError::Storage)
            .map_err(AuthorizedError::from)
            .map_err(auth)?;
        // Retain the lock before certification so mutex failure cannot become an ambiguous
        // postcommit quota failure. The raw coordinator/reducer has no access to this ledger.
        let ledger_owner = Arc::clone(&self.ledger);
        let mut ledger = ledger_owner
            .lock()
            .map_err(|_| auth(AuthorizedError::IntegrityFailure))?;
        let mut added_bytes = 0_u64;
        let mut added_owners = 0_u64;
        let mut release = Vec::new();
        if let Some(inventory) = request.blob_inventory {
            for reference in inventory.references() {
                let owner = self
                    .inner
                    .committed_blob_metadata(filesystem, reference.id(), lookup, &mut self.cache)
                    .map_err(AuthorizedError::from)
                    .map_err(auth)?;
                let staged = ledger
                    .staged_by_blob
                    .get(&reference.id())
                    .and_then(|upload| ledger.staged.get(upload).map(|charge| (*upload, *charge)));
                match owner {
                    Some((committed, owner)) => {
                        if committed != *reference || owner != principal.digest() {
                            return Err(auth(AuthorizedError::Unauthorized));
                        }
                    }
                    None => {
                        if !staged.is_some_and(|(_, charge)| {
                            charge.principal == principal.digest()
                                && charge.finalized == Some(*reference)
                                && charge.accepted_bytes == reference.byte_len()
                        }) {
                            return Err(auth(AuthorizedError::Unauthorized));
                        }
                        added_bytes = added_bytes
                            .checked_add(reference.byte_len())
                            .ok_or_else(|| auth(AuthorizedError::ResourceLimit))?;
                        added_owners = added_owners
                            .checked_add(1)
                            .ok_or_else(|| auth(AuthorizedError::ResourceLimit))?;
                    }
                }
                if let Some((upload, charge)) = staged
                    && charge.principal == principal.digest()
                    && charge.finalized == Some(*reference)
                {
                    if release.len() == MAX_STAGED_UPLOAD_RESERVATIONS {
                        return Err(auth(AuthorizedError::ResourceLimit));
                    }
                    release
                        .try_reserve(1)
                        .map_err(|_| auth(AuthorizedError::ResourceLimit))?;
                    release.push((upload, reference.id()));
                }
            }
        }
        if request.blob_inventory.is_some() {
            let committed = self
                .inner
                .committed_blob_usage(filesystem, principal.digest(), self.accounting)
                .map_err(AuthorizedError::from)
                .map_err(auth)?;
            if committed
                .owners
                .checked_add(added_owners)
                .ok_or_else(|| auth(AuthorizedError::ResourceLimit))?
                > self.accounting.maximum_total_owners
            {
                return Err(auth(AuthorizedError::ResourceLimit));
            }
            if committed
                .principal_bytes
                .checked_add(added_bytes)
                .ok_or_else(|| auth(AuthorizedError::ResourceLimit))?
                > quotas.max_committed_blob_bytes()
                || committed
                    .namespace_bytes
                    .checked_add(added_bytes)
                    .ok_or_else(|| auth(AuthorizedError::ResourceLimit))?
                    > namespace_quotas.max_committed_blob_bytes()
            {
                return Err(auth(AuthorizedError::ResourceLimit));
            }
        }
        let outcome = self
            .inner
            .commit_with_disk_inventory(
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
                lookup,
                storage,
                &mut self.cache,
            )
            .map_err(AuthorizedError::from)
            .map_err(auth)?;
        for (upload, blob) in release {
            ledger.staged.remove(&upload);
            ledger.staged_by_blob.remove(&blob);
        }
        drop(ledger);
        self.validate_policy()
            .map_err(|error| AuthorizedDiskInventoryError::CommittedPolicy { outcome, error })?;
        Ok(outcome)
    }
}
