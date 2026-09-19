use super::*;
use crate::packed_root_manifest::{
    ENCODED_MANIFEST_BYTES, PackedRootClaims, PackedRootContext, PackedRootFamily,
    PackedRootManifest, open_manifest, seal_manifest,
};

pub const MAX_PACKED_ROOT_ATTEMPTS: u8 = 64;
const FILE_BYTES: u64 = 16 + ENCODED_MANIFEST_BYTES as u64;

#[derive(Clone, Copy)]
pub struct PackedRootDiscoveryLimits {
    maximum_attempts: u8,
    maximum_encoded_bytes: u64,
}
impl PackedRootDiscoveryLimits {
    pub fn new(maximum_attempts: u8, maximum_encoded_bytes: u64) -> Result<Self, StorageError> {
        admit_attempts(maximum_attempts)?;
        if maximum_encoded_bytes == 0
            || maximum_encoded_bytes > maximum_attempts as u64 * FILE_BYTES
        {
            return Err(StorageError::ResourceLimit);
        }
        Ok(Self {
            maximum_attempts,
            maximum_encoded_bytes,
        })
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackedRootDiscoveryReport {
    pub slots: u8,
    /// Exact-length files read, including authenticated/structural cache refusals.
    pub manifest_files: u8,
    pub encoded_bytes: u64,
}
/// Certificate binding only; domain/canonical admission and consumer authorization are separate.
pub struct CertifiedPackedRoot {
    manifest: PackedRootManifest,
    proof: CertificateAnchorProof,
}
impl CertifiedPackedRoot {
    pub fn manifest(&self) -> &PackedRootManifest {
        &self.manifest
    }
}
fn admit_attempts(attempts: u8) -> Result<(), StorageError> {
    if attempts == 0 || attempts > MAX_PACKED_ROOT_ATTEMPTS {
        return Err(StorageError::ResourceLimit);
    }
    Ok(())
}
fn slot_name<W, E: EntropySource>(
    vault: &KeyVault<W, E>,
    context: PackedRootContext,
    revision: CommitRevision,
    attempt: u8,
) -> Result<EntryName, StorageError> {
    let mut hash = Sha256::new();
    hash.update(b"USTE-PACKED-ROOT-NAME-V1\0");
    hash.update(context.scope.namespace().as_bytes());
    hash.update(context.profile);
    hash.update(revision.get().to_be_bytes());
    hash.update([attempt]);
    let input: [u8; 32] = hash.finalize().into();
    let token = vault.derive_opaque_identifier(
        CryptoContext::new(
            context.scope.database(),
            Scope::Namespace(context.scope.namespace()),
            context.epoch,
            ObjectRole::IndexName,
            CryptoObjectId::from_bytes([0; 16]),
            attempt as u64,
            context.writer,
            2,
            0,
            FrameClass::Small4KiB,
        ),
        &input,
    )?;
    let mut name = String::from("p-");
    use core::fmt::Write as _;
    for byte in token {
        write!(&mut name, "{byte:02x}").map_err(|_| StorageError::ResourceLimit)?;
    }
    EntryName::new(name).map_err(|_| StorageError::IntegrityFailure)
}
fn cache_invalid(error: StorageError) -> Result<(), StorageError> {
    match error {
        StorageError::IntegrityFailure
        | StorageError::UnsupportedProfile
        | StorageError::Crypto(
            CryptoError::IntegrityFailure
            | CryptoError::InvalidEnvelope
            | CryptoError::UnsupportedProfile,
        ) => Ok(()),
        StorageError::Adapter(AdapterErrorKind::UnexpectedEof) => Ok(()),
        error => Err(error),
    }
}
impl<F, W, E, I> JournalStore<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    fn packed_root_context(
        &self,
        scope: NamespaceRef,
        profile: [u8; 32],
        object: [u8; 16],
    ) -> PackedRootContext {
        PackedRootContext {
            scope,
            profile,
            object,
            epoch: self.epoch,
            writer: self.writer,
        }
    }
    /// Privileged publication of caller-domain-validated contents at the exact current certificate.
    /// Create-new attempts never remove a fallback. The returned handle is not semantic authority.
    /// `claims.generation` is assigned from the selected attempt, not taken from the caller.
    pub fn publish_packed_root(
        &mut self,
        filesystem: &mut F,
        scope: NamespaceRef,
        profile: [u8; 32],
        mut claims: PackedRootClaims,
        families: &[PackedRootFamily],
        maximum_attempts: u8,
    ) -> Result<CertifiedPackedRoot, StorageError> {
        admit_attempts(maximum_attempts)?;
        if self.poisoned
            || scope.database() != self.database
            || self.checkpoint_anchor() != Some((claims.revision, claims.certificate_digest))
        {
            return Err(StorageError::InvalidState);
        }
        let proof = self.current_certificate_anchor_proof()?;
        let object = random_nonzero_id(&mut self.identity_entropy)?;
        let context = self.packed_root_context(scope, profile, object);
        for attempt in 0..maximum_attempts {
            claims.generation = attempt as u64 + 1;
            // Complete framing/crypto validation and bounded allocation before creating an entry.
            let encoded = seal_manifest(&mut self.vault, context, claims, families)?;
            let manifest = open_manifest(&self.vault, context, &encoded)?;
            let name = slot_name(&self.vault, context, claims.revision, attempt)?;
            let file = match filesystem.create_new(&self.database_directory, &name) {
                Ok(file) => file,
                Err(error) if error.kind() == AdapterErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            };
            write_all_at(filesystem, &file, 0, &object)?;
            write_all_at(filesystem, &file, 16, &encoded)?;
            filesystem.set_len(&file, FILE_BYTES)?;
            filesystem.sync_all(&file)?;
            filesystem.sync_directory(&self.database_directory)?;
            return Ok(CertifiedPackedRoot { manifest, proof });
        }
        Err(StorageError::ResourceLimit)
    }
    /// Validate process-local certificate ownership; this does not authorize consumers or content.
    pub fn validate_packed_root_certificate(
        &self,
        root: &CertifiedPackedRoot,
    ) -> Result<(), StorageError> {
        self.validate_historical_certificate_proof(&root.proof)?;
        let claims = root.manifest.claims();
        if root.manifest.context().scope.database() != self.database
            || root.proof.anchor() != (claims.revision, claims.certificate_digest)
        {
            return Err(StorageError::InvalidState);
        }
        Ok(())
    }
    /// Read bounded immutable manifest candidates without tree I/O or resident certificate history.
    pub fn discover_packed_roots_proven(
        &self,
        filesystem: &mut F,
        scope: NamespaceRef,
        profile: [u8; 32],
        proof: &CertificateAnchorProof,
        limits: PackedRootDiscoveryLimits,
    ) -> Result<(Vec<CertifiedPackedRoot>, PackedRootDiscoveryReport), StorageError> {
        self.validate_certificate_anchor_proof(proof)?;
        if scope.database() != self.database {
            return Err(StorageError::InvalidState);
        }
        let (revision, digest) = proof.anchor();
        let mut roots = Vec::new();
        roots
            .try_reserve_exact(limits.maximum_attempts as usize)
            .map_err(|_| StorageError::ResourceLimit)?;
        let mut report = PackedRootDiscoveryReport::default();
        for attempt in 0..limits.maximum_attempts {
            report.slots += 1;
            let context = self.packed_root_context(scope, profile, [0; 16]);
            let name = slot_name(&self.vault, context, revision, attempt)?;
            let file = match filesystem.open_existing(&self.database_directory, &name) {
                Ok(file) => file,
                Err(error) if error.kind() == AdapterErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if filesystem.metadata(&file)?.len != FILE_BYTES {
                continue;
            }
            if FILE_BYTES > limits.maximum_encoded_bytes - report.encoded_bytes {
                return Err(StorageError::ResourceLimit);
            }
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(FILE_BYTES as usize)
                .map_err(|_| StorageError::ResourceLimit)?;
            bytes.resize(FILE_BYTES as usize, 0);
            if let Err(error) = read_exact_at(filesystem, &file, 0, &mut bytes) {
                cache_invalid(error.into())?;
                continue;
            }
            report.manifest_files += 1;
            report.encoded_bytes += FILE_BYTES;
            let object: [u8; 16] = bytes[..16]
                .try_into()
                .map_err(|_| StorageError::IntegrityFailure)?;
            if object == [0; 16] {
                continue;
            }
            let context = PackedRootContext { object, ..context };
            let manifest = match open_manifest(&self.vault, context, &bytes[16..]) {
                Ok(manifest) => manifest,
                Err(error) => {
                    cache_invalid(error)?;
                    continue;
                }
            };
            let claims = manifest.claims();
            if claims.revision != revision
                || claims.certificate_digest != digest
                || claims.generation != attempt as u64 + 1
            {
                continue;
            }
            roots.push(CertifiedPackedRoot {
                manifest,
                proof: proof.clone(),
            });
        }
        Ok((roots, report))
    }
}
