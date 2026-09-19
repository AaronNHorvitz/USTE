//! Separately versioned encrypted manifest framing. Raw claims are not journal/domain authority.
use crate::{
    journal::StorageError,
    ordered_commitment::{self as logical, CommitmentContext, OrderedCommitment},
    packed_index_page::PackedPageContext,
    packed_tree_record::PackedLocator,
};
use uste_crypto::{
    CryptoContext, CryptoObjectId, EncryptedEnvelope, EntropySource, FrameClass, KeyEpoch,
    KeyVault, ObjectRole, Scope, WriterIncarnationId,
};
use uste_types::{CommitRevision, NamespaceRef};
use zeroize::Zeroizing;

pub const MANIFEST_BYTES: usize = 2048;
pub const ENCODED_MANIFEST_BYTES: usize = 4161;
pub const MAX_MANIFEST_FAMILIES: usize = 16;
const HEADER: usize = 224;
const FAMILY_BYTES: usize = 112;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct PackedRootContext {
    pub scope: NamespaceRef,
    pub profile: [u8; 32],
    pub epoch: KeyEpoch,
    pub writer: WriterIncarnationId,
    pub object: [u8; 16],
}
impl PackedRootContext {
    fn validate(self) -> Result<(), StorageError> {
        if self.object == [0; 16] {
            return Err(StorageError::InvalidState);
        }
        Ok(())
    }
    fn crypto(self) -> CryptoContext {
        CryptoContext::new(
            self.scope.database(),
            Scope::Namespace(self.scope.namespace()),
            self.epoch,
            ObjectRole::IndexPage,
            CryptoObjectId::from_bytes(self.object),
            0,
            self.writer,
            2,
            0,
            FrameClass::Small4KiB,
        )
    }
    fn owner(self, revision: CommitRevision, family: u8) -> PackedPageContext {
        PackedPageContext {
            scope: self.scope,
            profile: self.profile,
            epoch: self.epoch,
            writer: self.writer,
            object: self.object,
            creation_revision: revision,
            family,
            page: 0,
        }
    }
}
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct PackedRootClaims {
    pub revision: CommitRevision,
    pub generation: u64,
    pub certificate_digest: [u8; 32],
    pub reducer_profile: [u8; 32],
    pub state_commitment_profile: [u8; 32],
    pub state_digest: [u8; 32],
}
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct PackedRootFamily {
    pub family: u8,
    pub commitment: OrderedCommitment,
    pub root: Option<PackedLocator>,
}
pub struct PackedRootManifest {
    context: PackedRootContext,
    claims: PackedRootClaims,
    families: Vec<PackedRootFamily>,
}
impl PackedRootManifest {
    pub fn context(&self) -> PackedRootContext {
        self.context
    }
    pub fn claims(&self) -> PackedRootClaims {
        self.claims
    }
    pub fn families(&self) -> &[PackedRootFamily] {
        &self.families
    }
}

fn validate_families(
    context: PackedRootContext,
    claims: PackedRootClaims,
    families: &[PackedRootFamily],
) -> Result<(), StorageError> {
    context.validate()?;
    if claims.generation == 0 || families.is_empty() {
        return Err(StorageError::InvalidState);
    }
    if families.len() > MAX_MANIFEST_FAMILIES {
        return Err(StorageError::ResourceLimit);
    }
    let mut previous = 0;
    for family in families {
        if family.family <= previous {
            return Err(StorageError::IntegrityFailure);
        }
        previous = family.family;
        let logical_context = CommitmentContext::new(context.scope, context.profile, family.family)
            .map_err(|_| StorageError::IntegrityFailure)?;
        match family.root {
            None if family.commitment != logical::empty_commitment(logical_context) => {
                return Err(StorageError::IntegrityFailure);
            }
            Some(location) => {
                if family.commitment.entries() == 0 {
                    return Err(StorageError::IntegrityFailure);
                }
                location.resolve(
                    context.scope,
                    context.profile,
                    family.family,
                    claims.revision,
                )?;
            }
            None => {}
        }
    }
    Ok(())
}
fn encode_plain(
    context: PackedRootContext,
    claims: PackedRootClaims,
    families: &[PackedRootFamily],
) -> Result<Zeroizing<Vec<u8>>, StorageError> {
    validate_families(context, claims, families)?;
    let mut out = Zeroizing::new(Vec::new());
    out.try_reserve_exact(MANIFEST_BYTES)
        .map_err(|_| StorageError::ResourceLimit)?;
    out.resize(MANIFEST_BYTES, 0);
    out[..4].copy_from_slice(b"UPRT");
    out[4] = 2;
    out[6] = families.len() as u8;
    out[8..24].copy_from_slice(context.scope.namespace().as_bytes());
    out[24..32].copy_from_slice(&claims.revision.get().to_be_bytes());
    out[32..40].copy_from_slice(&claims.generation.to_be_bytes());
    out[40..72].copy_from_slice(&claims.certificate_digest);
    out[72..104].copy_from_slice(&claims.reducer_profile);
    out[104..136].copy_from_slice(&claims.state_commitment_profile);
    out[136..168].copy_from_slice(&claims.state_digest);
    out[168..200].copy_from_slice(&context.profile);
    out[200..216].copy_from_slice(&context.object);
    for (index, family) in families.iter().enumerate() {
        let base = HEADER + index * FAMILY_BYTES;
        out[base] = family.family;
        out[base + 8..base + 16].copy_from_slice(&family.commitment.entries().to_be_bytes());
        out[base + 16..base + 24].copy_from_slice(&family.commitment.logical_bytes().to_be_bytes());
        out[base + 24..base + 56].copy_from_slice(family.commitment.digest());
        if let Some(location) = family.root {
            out[base + 56] = 1;
            out[base + 57..base + 111].copy_from_slice(&location.encode_fixed());
        }
    }
    Ok(out)
}
fn array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], StorageError> {
    bytes
        .get(offset..offset + N)
        .ok_or(StorageError::IntegrityFailure)?
        .try_into()
        .map_err(|_| StorageError::IntegrityFailure)
}
fn decode_plain(
    context: PackedRootContext,
    bytes: &[u8],
) -> Result<PackedRootManifest, StorageError> {
    context.validate()?;
    if bytes.len() != MANIFEST_BYTES
        || &bytes[..4] != b"UPRT"
        || bytes[4] != 2
        || bytes[5] != 0
        || bytes[7] != 0
        || bytes[216..HEADER].iter().any(|byte| *byte != 0)
        || &bytes[8..24] != context.scope.namespace().as_bytes()
        || bytes[168..200] != context.profile
        || bytes[200..216] != context.object
    {
        return Err(StorageError::IntegrityFailure);
    }
    let count = bytes[6] as usize;
    if count == 0
        || count > MAX_MANIFEST_FAMILIES
        || bytes[HEADER + count * FAMILY_BYTES..]
            .iter()
            .any(|byte| *byte != 0)
    {
        return Err(StorageError::IntegrityFailure);
    }
    let claims = PackedRootClaims {
        revision: CommitRevision::new(u64::from_be_bytes(array(bytes, 24)?))
            .map_err(|_| StorageError::IntegrityFailure)?,
        generation: u64::from_be_bytes(array(bytes, 32)?),
        certificate_digest: array(bytes, 40)?,
        reducer_profile: array(bytes, 72)?,
        state_commitment_profile: array(bytes, 104)?,
        state_digest: array(bytes, 136)?,
    };
    if claims.generation == 0 {
        return Err(StorageError::IntegrityFailure);
    }
    let mut families = Vec::new();
    families
        .try_reserve_exact(count)
        .map_err(|_| StorageError::ResourceLimit)?;
    for index in 0..count {
        let base = HEADER + index * FAMILY_BYTES;
        let family = bytes[base];
        if family == 0
            || bytes[base + 1..base + 8].iter().any(|byte| *byte != 0)
            || bytes[base + 111] != 0
        {
            return Err(StorageError::IntegrityFailure);
        }
        let entries = u64::from_be_bytes(array(bytes, base + 8)?);
        let logical_bytes = u64::from_be_bytes(array(bytes, base + 16)?);
        let digest = array(bytes, base + 24)?;
        let root = match bytes[base + 56] {
            0 if bytes[base + 57..base + 111].iter().all(|byte| *byte == 0) => None,
            1 => Some(PackedLocator::decode_fixed(
                &bytes[base + 57..base + 111],
                context.owner(claims.revision, family),
            )?),
            _ => return Err(StorageError::IntegrityFailure),
        };
        let commitment = if entries == 0 {
            let c = CommitmentContext::new(context.scope, context.profile, family)
                .map_err(|_| StorageError::IntegrityFailure)?;
            let empty = logical::empty_commitment(c);
            if logical_bytes != 0 || digest != *empty.digest() {
                return Err(StorageError::IntegrityFailure);
            }
            empty
        } else {
            OrderedCommitment::claimed_nonempty(entries, logical_bytes, digest)
                .map_err(|_| StorageError::IntegrityFailure)?
        };
        families.push(PackedRootFamily {
            family,
            commitment,
            root,
        });
    }
    validate_families(context, claims, &families)?;
    Ok(PackedRootManifest {
        context,
        claims,
        families,
    })
}

pub fn seal_manifest<W, E: EntropySource>(
    vault: &mut KeyVault<W, E>,
    context: PackedRootContext,
    claims: PackedRootClaims,
    families: &[PackedRootFamily],
) -> Result<Vec<u8>, StorageError> {
    let plaintext = encode_plain(context, claims, families)?;
    let encoded = vault.encrypt(context.crypto(), &plaintext)?.encode()?;
    if encoded.len() != ENCODED_MANIFEST_BYTES {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(encoded)
}
pub fn open_manifest<W, E: EntropySource>(
    vault: &KeyVault<W, E>,
    context: PackedRootContext,
    encoded: &[u8],
) -> Result<PackedRootManifest, StorageError> {
    context.validate()?;
    if encoded.len() != ENCODED_MANIFEST_BYTES {
        return Err(StorageError::IntegrityFailure);
    }
    let envelope = EncryptedEnvelope::decode(encoded)?;
    let plaintext = vault.decrypt(context.crypto(), &envelope)?;
    decode_plain(context, plaintext.as_slice())
}
