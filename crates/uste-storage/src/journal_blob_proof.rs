use super::*;

/// Work admitted for one certificate re-read plus its inventory, not blob payload or an earlier
/// certificate-chain proof. Inventory count is checked before reference-vector allocation.
#[derive(Clone, Copy, Debug)]
pub struct BlobReferenceProofLimits {
    maximum_inventory_references: usize,
    maximum_encoded_bytes: u64,
}

impl BlobReferenceProofLimits {
    pub fn new(
        maximum_inventory_references: usize,
        maximum_encoded_bytes: u64,
    ) -> Result<Self, StorageError> {
        if maximum_inventory_references == 0
            || maximum_inventory_references > crate::blob::MAX_BLOBS_PER_INVENTORY
            || maximum_encoded_bytes == 0
            || maximum_encoded_bytes > SMALL_ENVELOPE_BYTES + MAX_ENCODED_BLOB_INVENTORY_BYTES
        {
            return Err(StorageError::ResourceLimit);
        }
        Ok(Self {
            maximum_inventory_references,
            maximum_encoded_bytes,
        })
    }
}

/// Successful metadata work only; no partial report or proof escapes a failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlobReferenceProofReport {
    pub inventory_references: usize,
    pub encoded_bytes: u64,
}

/// Exact committed-reference evidence for this live journal owner. It conveys neither principal
/// authorization nor first-owner/quota evidence and retains no inventory body, key or lock.
pub struct BlobReferenceProof {
    certificate: CertificateAnchorProof,
    reference: BlobReference,
    report: BlobReferenceProofReport,
}

impl fmt::Debug for BlobReferenceProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlobReferenceProof")
            .field("reference", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl BlobReferenceProof {
    pub fn reference(&self) -> BlobReference {
        self.reference
    }
    pub fn report(&self) -> BlobReferenceProofReport {
        self.report
    }
}

impl<F, W, E, I> JournalStore<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Prove exact membership in one committed inventory without consulting resident blob maps.
    /// The caller supplies separately bounded certificate-chain evidence; this re-reads and
    /// authenticates its selected certificate and inventory. Normal journal open still verifies
    /// all committed payloads. This operation is not a payload scrub or consumer authorization.
    pub fn authenticate_blob_reference(
        &self,
        filesystem: &mut F,
        certificate: &CertificateAnchorProof,
        reference: BlobReference,
        limits: BlobReferenceProofLimits,
    ) -> Result<BlobReferenceProof, StorageError> {
        self.validate_historical_certificate_proof(certificate)?;
        if reference.scope().database() != self.database {
            return Err(StorageError::InvalidState);
        }
        let remaining = limits
            .maximum_encoded_bytes
            .checked_sub(SMALL_ENVELOPE_BYTES)
            .ok_or(StorageError::ResourceLimit)?;
        let (revision, digest) = certificate.anchor();
        let encoded = read_bounded(
            filesystem,
            &self.certificate_file,
            revision
                .get()
                .checked_mul(SMALL_ENVELOPE_BYTES)
                .ok_or(StorageError::ResourceLimit)?,
            SMALL_ENVELOPE_BYTES,
        )?;
        if sha256(&encoded) != digest {
            return Err(StorageError::IntegrityFailure);
        }
        let selected = decode_certificate(
            &self.vault,
            self.database,
            self.epoch,
            self.certificate_log_id,
            self.writer,
            revision,
            &encoded,
        )?;
        if selected.revision != revision.get() || selected.group_sequence != revision.get() {
            return Err(StorageError::IntegrityFailure);
        }
        if selected.blob_inventory_digest == EMPTY_BLOB_INVENTORY_DIGEST {
            return Err(StorageError::InvalidState);
        }
        let (inventory, encoded_bytes) = load_blob_inventory_bounded(
            filesystem,
            &self.vault,
            &self.database_directory,
            self.database,
            self.epoch,
            self.writer,
            selected.blob_inventory_digest,
            false,
            limits.maximum_inventory_references,
            remaining,
        )?;
        let entry = inventory
            .references()
            .binary_search_by_key(&reference.id(), |item| item.id())
            .ok()
            .map(|index| inventory.references()[index]);
        if inventory.scope() != reference.scope() || entry != Some(reference) {
            return Err(StorageError::InvalidState);
        }
        Ok(BlobReferenceProof {
            certificate: certificate.clone(),
            reference,
            report: BlobReferenceProofReport {
                inventory_references: inventory.references().len(),
                encoded_bytes: SMALL_ENVELOPE_BYTES + encoded_bytes,
            },
        })
    }

    /// Read using owner-bound committed-reference evidence, never a resident blob-map lookup.
    /// Valid after successful appends by this owner, not after reopen, poison or owner change.
    /// Existing chunk/output bounds apply; the caller must discard partial output on error.
    /// Current consumer authorization must be checked separately before entering this raw API.
    pub fn read_proven_blob_range(
        &self,
        filesystem: &mut F,
        proof: &BlobReferenceProof,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, StorageError> {
        self.validate_historical_certificate_proof(&proof.certificate)?;
        read_range(
            filesystem,
            &self.database_directory,
            &self.vault,
            self.epoch,
            self.writer,
            proof.reference,
            offset,
            output,
        )
    }
}
