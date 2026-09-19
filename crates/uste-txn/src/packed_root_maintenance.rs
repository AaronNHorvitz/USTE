//! Scoped packed-root discovery and terminal publication. Domain claims remain untrusted.
use super::*;
use uste_storage::journal::{
    CertificateAnchorReadLimits, CertifiedPackedRoot, PackedRootDiscoveryLimits,
    PackedRootDiscoveryReport,
};
use uste_storage::packed_root_manifest::{PackedRootClaims, PackedRootFamily};

impl<F, W, E, I> DerivedIndexMaintenance<'_, F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Privileged terminal cache publication; not reducer/domain or consumer admission.
    pub fn publish_packed_root(
        &mut self,
        filesystem: &mut F,
        profile: [u8; 32],
        claims: PackedRootClaims,
        families: &[PackedRootFamily],
        maximum_attempts: u8,
    ) -> Result<CertifiedPackedRoot, TransactionError> {
        self.journal
            .publish_packed_root(
                filesystem,
                self.scope,
                profile,
                claims,
                families,
                maximum_attempts,
            )
            .map_err(TransactionError::Storage)
    }
    pub fn discover_packed_roots(
        &self,
        filesystem: &mut F,
        profile: [u8; 32],
        certificate_limits: CertificateAnchorReadLimits,
        discovery_limits: PackedRootDiscoveryLimits,
    ) -> Result<(Vec<CertifiedPackedRoot>, PackedRootDiscoveryReport), TransactionError> {
        let (revision, digest) = self
            .journal
            .checkpoint_anchor()
            .ok_or(TransactionError::InvalidRequest)?;
        let proof = self
            .journal
            .authenticate_certificate_anchor(filesystem, revision, digest, certificate_limits)
            .map_err(TransactionError::Storage)?;
        self.journal
            .discover_packed_roots_proven(filesystem, self.scope, profile, &proof, discovery_limits)
            .map_err(TransactionError::Storage)
    }
}

impl<F, W, E, I> AuthenticatedIndexRecovery<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Private recovery may discover an explicitly selected old base; it does not become current.
    pub fn discover_packed_roots_at_revision(
        &self,
        filesystem: &mut F,
        profile: [u8; 32],
        revision: CommitRevision,
        certificate_limits: CertificateAnchorReadLimits,
        discovery_limits: PackedRootDiscoveryLimits,
    ) -> Result<(Vec<CertifiedPackedRoot>, PackedRootDiscoveryReport), TransactionError> {
        let proof = self
            .journal
            .authenticate_certificate_revision(filesystem, revision, certificate_limits)
            .map_err(TransactionError::Storage)?;
        self.journal
            .discover_packed_roots_proven(
                filesystem,
                self.scope(),
                profile,
                &proof,
                discovery_limits,
            )
            .map_err(TransactionError::Storage)
    }
    /// Publish only the exact current frontier. Intermediate recovery targets remain private.
    pub fn publish_recovered_packed_root(
        &mut self,
        filesystem: &mut F,
        profile: [u8; 32],
        claims: PackedRootClaims,
        families: &[PackedRootFamily],
        maximum_attempts: u8,
    ) -> Result<CertifiedPackedRoot, TransactionError> {
        self.journal
            .publish_packed_root(
                filesystem,
                self.scope(),
                profile,
                claims,
                families,
                maximum_attempts,
            )
            .map_err(TransactionError::Storage)
    }
}
