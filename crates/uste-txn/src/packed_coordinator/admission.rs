//! Exact published family and independently admitted ready-domain binding.
use super::*;
pub(super) fn validate_packed_base<S, F, W, E, I>(
    recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
    primary: &PackedCoordinatorPrefix,
    quota: &PackedQuotaPrefix,
    primary_root: &CertifiedPackedRoot,
    quota_root: &CertifiedPackedRoot,
    state: &S,
) -> Result<PackedRootClaims, TransactionError>
where
    S: PackedCoordinatorState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let scope = recovery.scope();
    if primary.scope() != scope
        || recovery
            .journal
            .checkpoint_anchor()
            .is_none_or(|anchor| primary.anchor().0 > anchor.0)
    {
        return Err(TransactionError::IntegrityFailure);
    }
    quota.validate_live_pair(&recovery.journal, primary)?;
    state.validate_packed_owner(&recovery.journal)?;
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
    Ok(claims)
}
