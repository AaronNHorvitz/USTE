use super::*;
use uste_storage::journal::{BlobReferenceProofLimits, CertificateAnchorReadLimits};

/// Independent work limits for disk owner/first-reference lookups and committed blob reads.
/// Discovery is needed only for new overlay owners or a legacy base lacking first-reference
/// evidence. Its range budget counts certificates/groups (including configured chain proofs),
/// not inventories/segment headers; those retain the journal's independent format bounds.
#[derive(Clone, Copy, Debug)]
pub struct DiskBlobReadLimits {
    pub lookup: IndexGetLimits,
    pub certificate: CertificateAnchorReadLimits,
    pub inventory: BlobReferenceProofLimits,
    pub maximum_discovery_groups: u64,
    pub maximum_discovery_encoded_bytes: u64,
}

impl<S, F, W, E, I> DiskCommitCoordinator<S, F, W, E, I>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Privileged committed-blob read using admitted disk owners and exact inventory evidence.
    /// This never consults storage's resident blob map. Current consumer authorization and quotas
    /// are separate prerequisites. Discard partial output on error, as with the storage API.
    #[allow(clippy::too_many_arguments)]
    pub fn read_blob_range(
        &self,
        filesystem: &mut F,
        reference: BlobReference,
        offset: u64,
        output: &mut [u8],
        limits: DiskBlobReadLimits,
        cache: &mut PageCache,
    ) -> Result<usize, TransactionError> {
        if self.inner.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        if reference.scope() != self.scope() {
            return Err(TransactionError::InvalidRequest);
        }
        if offset > reference.byte_len() || output.len() > uste_storage::BLOB_CHUNK_BYTES {
            return Err(TransactionError::ResourceLimit);
        }
        if self
            .committed_blob_owner(filesystem, reference, limits.lookup, cache)?
            .is_none()
        {
            return Err(TransactionError::InvalidRequest);
        }
        let overlay = self
            .inner
            .committed_blob_owners
            .contains_key(&(self.scope(), reference.id()));
        let certificate = if !overlay && self.base.has_first_reference_evidence() {
            let revision = self
                .base
                .first_reference_from_journal(
                    &self.inner.journal,
                    filesystem,
                    reference.id(),
                    limits.lookup,
                    cache,
                )?
                .ok_or(TransactionError::IntegrityFailure)?;
            self.inner
                .journal
                .authenticate_certificate_revision(filesystem, revision, limits.certificate)
                .map_err(TransactionError::Storage)?
        } else {
            let frontier = self
                .inner
                .journal
                .checkpoint_anchor()
                .ok_or(TransactionError::IntegrityFailure)?;
            let first = if overlay {
                CommitRevision::new(
                    self.base
                        .metadata
                        .revision()
                        .get()
                        .checked_add(1)
                        .ok_or(TransactionError::ResourceLimit)?,
                )
                .map_err(|_| TransactionError::IntegrityFailure)?
            } else {
                CommitRevision::FIRST
            };
            let mut anchor = None;
            self.inner
                .journal
                .visit_committed_range_report(
                    filesystem,
                    first,
                    frontier.0,
                    limits.maximum_discovery_groups,
                    limits.maximum_discovery_encoded_bytes,
                    |_, group| {
                        if anchor.is_none()
                            && group.blob_inventory.is_some_and(|inventory| {
                                inventory.references().binary_search(&reference).is_ok()
                            })
                        {
                            anchor = Some((group.revision, group.certificate_digest));
                        }
                        Ok(())
                    },
                )
                .map_err(TransactionError::Storage)?;
            let (revision, digest) = anchor.ok_or(TransactionError::IntegrityFailure)?;
            self.inner
                .journal
                .authenticate_certificate_anchor(filesystem, revision, digest, limits.certificate)
                .map_err(TransactionError::Storage)?
        };
        let proof = self
            .inner
            .journal
            .authenticate_blob_reference(filesystem, &certificate, reference, limits.inventory)
            .map_err(TransactionError::Storage)?;
        self.inner
            .journal
            .read_proven_blob_range(filesystem, &proof, offset, output)
            .map_err(TransactionError::Storage)
    }
}
