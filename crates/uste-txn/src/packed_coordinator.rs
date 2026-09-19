//! Opt-in trusted live writes over admitted packed metadata; consumer authorization is separate.
use super::*;
mod rebase;
use crate::disk_coordinator::{DiskCommitMetadata, DiskMetadataBase};
pub use rebase::{
    PackedCoordinatorPublicationState, PackedMetadataRebaseLimits, PackedMetadataRebaseReport,
};
use uste_storage::{
    journal::{CertifiedPackedRoot, DiskBlobAppendLimits},
    packed_root_manifest::PackedRootClaims,
    packed_tree_lookup::TreeLookupLimits,
};

/// Trusted independent validation of ready domain state, not a metadata-root equivalence check.
/// Implementations must check scope, revision/certificate, reducer and state commitment profile
/// and digest. Pending/unpublished domain states must fail. Generation is publication-local.
pub trait PackedCoordinatorState: TransactionState {
    fn validate_packed_metadata(
        &self,
        scope: NamespaceRef,
        claims: PackedRootClaims,
    ) -> Result<(), ApplyError>;
}

#[derive(Clone, Copy)]
pub struct PackedCommitLimits {
    pub lookup: TreeLookupLimits,
    /// Required to select map-free storage inventory append; never silently enables that mode.
    pub storage: Option<DiskBlobAppendLimits>,
}

/// Raw privileged coordinator. No legacy memory-only authorization facade is exposed.
pub struct PackedCommitCoordinator<S, F, W, E, I>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    inner: CommitCoordinator<S, F, W, E, I>,
    primary: PackedCoordinatorPrefix,
    quota: PackedQuotaPrefix,
    overlay: CoordinatorRecoveryLimits,
    profiles: ([u8; 32], [u8; 32]),
    rebase_required: bool,
}

impl<S, F, W, E, I> PackedCommitCoordinator<S, F, W, E, I>
where
    S: PackedCoordinatorState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Consume the exclusive owner without reopening or constructing journal-origin maps.
    /// Both published receipts must match the independently admitted physical prefix families.
    #[allow(clippy::too_many_arguments)]
    pub fn from_admitted_prefixes(
        recovery: AuthenticatedIndexRecovery<F, W, E, I>,
        primary: PackedCoordinatorPrefix,
        quota: PackedQuotaPrefix,
        primary_root: &CertifiedPackedRoot,
        quota_root: &CertifiedPackedRoot,
        state: S,
        retention: RetentionDays,
        overlay: CoordinatorRecoveryLimits,
    ) -> Result<Self, TransactionError> {
        let scope = recovery.scope();
        if primary.scope() != scope
            || recovery.journal.checkpoint_anchor() != Some(primary.anchor())
        {
            return Err(TransactionError::IntegrityFailure);
        }
        quota.validate_live_pair(&recovery.journal, &primary)?;
        for (root, profile, families) in [
            (
                primary_root,
                COORDINATOR_PACKED_PROFILE_V1,
                primary.families().to_vec(),
            ),
            (
                quota_root,
                COORDINATOR_PACKED_USAGE_PROFILE_V1,
                quota.families().to_vec(),
            ),
        ] {
            recovery
                .journal
                .validate_packed_root_certificate(root)
                .map_err(TransactionError::Storage)?;
            let manifest = root.manifest();
            let claims = manifest.claims();
            if manifest.context().scope != scope
                || manifest.context().profile != profile
                || (claims.revision, claims.certificate_digest) != primary.anchor()
                || manifest.families() != families
            {
                return Err(TransactionError::IntegrityFailure);
            }
        }
        let claims = primary_root.manifest().claims();
        let accounting = quota_root.manifest().claims();
        if claims.reducer_profile != accounting.reducer_profile
            || claims.state_commitment_profile != accounting.state_commitment_profile
            || claims.state_digest != accounting.state_digest
        {
            return Err(TransactionError::IntegrityFailure);
        }
        state
            .validate_packed_metadata(scope, claims)
            .map_err(map_apply_error)?;
        Ok(Self {
            inner: CommitCoordinator {
                scope,
                retention,
                journal: recovery.journal,
                state,
                outcomes: BTreeMap::new(),
                transactions: BTreeMap::new(),
                committed_blob_owners: BTreeMap::new(),
                recovered: true,
                uncertain: false,
            },
            primary,
            quota,
            overlay,
            profiles: (claims.reducer_profile, claims.state_commitment_profile),
            rebase_required: false,
        })
    }

    pub fn state(&self) -> Result<&S, TransactionError> {
        if self.inner.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        Ok(&self.inner.state)
    }
    /// Resident post-base outcomes and first owners only; not complete storage residency.
    pub fn overlay_counts(&self) -> (usize, usize) {
        (
            self.inner.outcomes.len(),
            self.inner.committed_blob_owners.len(),
        )
    }
    /// Historical installed accounting anchor, not current usage after new commits.
    pub fn base_anchor(&self) -> (CommitRevision, [u8; 32]) {
        self.quota.anchor()
    }
    pub fn rebase_required(&self) -> bool {
        self.rebase_required
    }
    fn admitted_overlay_limits(&self) -> CoordinatorRecoveryLimits {
        if self.rebase_required {
            CoordinatorRecoveryLimits {
                maximum_outcomes: 0,
                ..self.overlay
            }
        } else {
            self.overlay
        }
    }

    /// Trusted staging only; does not grant authorization or reserve consumer quota.
    pub fn start_blob_upload(
        &mut self,
        scope: NamespaceRef,
    ) -> Result<BlobUpload, TransactionError> {
        self.inner.start_blob_upload(scope)
    }
    pub fn resume_blob_upload(
        &mut self,
        fs: &mut F,
        token: BlobUploadToken,
    ) -> Result<BlobUpload, TransactionError> {
        self.inner.resume_blob_upload(fs, token)
    }
    pub fn write_blob_upload(
        &mut self,
        fs: &mut F,
        upload: &mut BlobUpload,
        input: &[u8],
    ) -> Result<(), TransactionError> {
        self.inner.write_blob_upload(fs, upload, input)
    }
    pub fn finish_blob_upload(
        &mut self,
        fs: &mut F,
        upload: &mut BlobUpload,
    ) -> Result<BlobReference, TransactionError> {
        self.inner.finish_blob_upload(fs, upload)
    }
    pub fn abort_blob_upload(
        &mut self,
        fs: &mut F,
        upload: &mut BlobUpload,
    ) -> Result<(), TransactionError> {
        self.inner.abort_blob_upload(fs, upload)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn check_commit(
        &self,
        fs: &mut F,
        request: TransactionRequest<'_>,
        clock: &mut impl Clock,
        cancellation: &impl Cancellation,
        limits: PackedCommitLimits,
        cache: &mut PageCache,
    ) -> Result<DiskCommitCheck, TransactionError> {
        let admitted = self.inner.admit_commit(
            fs,
            request,
            clock,
            cancellation,
            Some(DiskCommitMetadata {
                base: DiskMetadataBase::Packed(&self.primary, limits.lookup),
                overlay: self.admitted_overlay_limits(),
                storage: limits.storage,
                cache,
            }),
        )?;
        Ok(match admitted {
            commit_admission::CommitAdmission::Retry(outcome) => DiskCommitCheck::Retry(outcome),
            commit_admission::CommitAdmission::Fresh(fresh) => DiskCommitCheck::Ready {
                revision: fresh.revision,
            },
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit(
        &mut self,
        fs: &mut F,
        request: TransactionRequest<'_>,
        clock: &mut impl Clock,
        cancellation: &impl Cancellation,
        limits: PackedCommitLimits,
        cache: &mut PageCache,
    ) -> Result<TransactionOutcome, TransactionError> {
        let overlay = self.admitted_overlay_limits();
        self.inner.commit_with_preparation(
            fs,
            request,
            clock,
            cancellation,
            Some(DiskCommitMetadata {
                base: DiskMetadataBase::Packed(&self.primary, limits.lookup),
                overlay,
                storage: limits.storage,
                cache,
            }),
            |state, revision| {
                state.prepare(request.canonical_request, request.blob_inventory, revision)
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_prepared(
        &mut self,
        fs: &mut F,
        request: TransactionRequest<'_>,
        prepared: S::Prepared,
        clock: &mut impl Clock,
        cancellation: &impl Cancellation,
        limits: PackedCommitLimits,
        cache: &mut PageCache,
    ) -> Result<TransactionOutcome, TransactionError>
    where
        S: ExternallyPreparedTransactionState,
    {
        let overlay = self.admitted_overlay_limits();
        self.inner.commit_with_preparation(
            fs,
            request,
            clock,
            cancellation,
            Some(DiskCommitMetadata {
                base: DiskMetadataBase::Packed(&self.primary, limits.lookup),
                overlay,
                storage: limits.storage,
                cache,
            }),
            move |state, revision| {
                state.validate_external_prepared(
                    request.canonical_request,
                    request.blob_inventory,
                    revision,
                    &prepared,
                )?;
                Ok(prepared)
            },
        )
    }
}
