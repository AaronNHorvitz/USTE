//! Map-free authenticated suffix reconstruction; no partial live state escapes failure.
use super::rebase::Work;
use super::*;

/// Trusted private reducer publication bound to an already authenticated transaction receipt.
/// Install exactly `prepared` and advance ready scope/revision/certificate to this receipt.
/// Do not expose provisional state, append transactions or substitute another prepared result.
pub trait PackedCoordinatorRecoveryState: PackedCoordinatorPublicationState {
    fn publish_recovered(
        &mut self,
        prepared: Self::Prepared,
        transaction: &RecoveredFrontierTransaction,
    ) -> Result<(), ApplyError>;
}

impl<S, F, W, E, I> PackedCommitCoordinator<S, F, W, E, I>
where
    S: PackedCoordinatorRecoveryState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Retain only one decoded transaction/preparation and private paired roots, never suffix
    /// outcome/owner maps. No partially recovered coordinator escapes a late failure.
    #[allow(clippy::too_many_arguments)]
    pub fn recover_from_admitted_prefixes(
        mut recovery: AuthenticatedIndexRecovery<F, W, E, I>,
        fs: &mut F,
        mut primary: PackedCoordinatorPrefix,
        mut quota: PackedQuotaPrefix,
        primary_root: &CertifiedPackedRoot,
        quota_root: &CertifiedPackedRoot,
        mut state: S,
        retention: RetentionDays,
        overlay: CoordinatorRecoveryLimits,
        limits: PackedMetadataRebaseLimits,
    ) -> Result<(Self, Option<PackedMetadataRebaseReport>), TransactionError> {
        let anchor = recovery
            .journal
            .checkpoint_anchor()
            .ok_or(TransactionError::IntegrityFailure)?;
        if primary.anchor() == anchor {
            return Self::from_admitted_prefixes(
                recovery,
                primary,
                quota,
                primary_root,
                quota_root,
                state,
                retention,
                overlay,
            )
            .map(|coordinator| (coordinator, None));
        }
        let base = validate_packed_base(
            &recovery,
            &primary,
            &quota,
            primary_root,
            quota_root,
            &state,
        )?;
        if limits.maximum_publication_attempts == 0 || limits.maximum_publication_attempts > 64 {
            return Err(TransactionError::ResourceLimit);
        }
        let first = primary
            .anchor()
            .0
            .checked_next()
            .map_err(|_| TransactionError::RevisionExhausted)?;
        let mut cursor = recovery.open_transaction_cursor_with_certificate_window(
            first,
            anchor.0,
            limits.maximum_groups,
            limits.maximum_encoded_bytes,
            limits.certificate_window,
        )?;
        let mut work = Work::default();
        while let Some(transaction) = recovery.next_recovered_transaction(fs, &mut cursor)? {
            let prepared = state
                .prepare(
                    transaction.canonical_request(),
                    transaction.blob_inventory(),
                    transaction.revision(),
                )
                .map_err(map_apply_error)?;
            if S::result_digest(&prepared) != transaction.outcome().result_digest {
                return Err(TransactionError::IntegrityFailure);
            }
            let (next_primary, p) = stage_packed_coordinator_prefix(
                &mut recovery,
                fs,
                Some(&primary),
                &transaction,
                limits.staging,
            )?;
            let (next_quota, q) = stage_packed_quota_prefix(
                &mut recovery,
                fs,
                Some(&quota),
                &next_primary,
                &transaction,
                limits.staging,
            )?;
            work.prefixes(p, q)?;
            state
                .publish_recovered(prepared, &transaction)
                .map_err(map_apply_error)?;
            primary = next_primary;
            quota = next_quota;
        }
        let journal = recovery.finish_transaction_cursor(cursor)?;
        if primary.anchor() != anchor
            || quota.anchor() != anchor
            || journal.groups
                != anchor
                    .0
                    .get()
                    .checked_sub(base.revision.get())
                    .ok_or(TransactionError::IntegrityFailure)?
        {
            return Err(TransactionError::IntegrityFailure);
        }
        let claims = state
            .packed_publication_claims(recovery.scope(), anchor)
            .map_err(map_apply_error)?;
        if (claims.revision, claims.certificate_digest) != anchor
            || claims.reducer_profile != base.reducer_profile
            || claims.state_commitment_profile != base.state_commitment_profile
        {
            return Err(TransactionError::IntegrityFailure);
        }
        // Validate terminal ready state before creating either manifest, not only at installation.
        state
            .validate_packed_metadata(recovery.scope(), claims)
            .map_err(map_apply_error)?;
        quota.validate_live_pair(&recovery.journal, &primary)?;
        let primary_root = recovery.publish_recovered_packed_root(
            fs,
            COORDINATOR_PACKED_PROFILE_V1,
            claims,
            &primary.families(),
            limits.maximum_publication_attempts,
        )?;
        let quota_root = recovery.publish_recovered_packed_root(
            fs,
            COORDINATOR_PACKED_USAGE_PROFILE_V1,
            claims,
            &quota.families(),
            limits.maximum_publication_attempts,
        )?;
        let coordinator = Self::from_admitted_prefixes(
            recovery,
            primary,
            quota,
            &primary_root,
            &quota_root,
            state,
            retention,
            overlay,
        )?;
        Ok((
            coordinator,
            Some(work.finish(journal, primary_root, quota_root)),
        ))
    }
}
