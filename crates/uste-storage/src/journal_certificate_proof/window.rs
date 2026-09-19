//! Fixed-size certificate windows authenticated backward from one terminal proof.
use super::*;

/// Hard ceiling on retained proof receipts, independent of journal length.
pub const MAX_CERTIFICATE_PROOF_WINDOW: u64 = 64;

/// One authenticated window. Proof receipts share the reported acquisition work; their reports
/// must not be summed as if each receipt had independently reread the certificate suffix.
pub struct CertificateProofWindow {
    proofs: Vec<CertificateAnchorProof>,
    report: CertificateAnchorReadReport,
}

impl CertificateProofWindow {
    pub fn report(&self) -> CertificateAnchorReadReport {
        self.report
    }

    pub fn len(&self) -> usize {
        self.proofs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.proofs.is_empty()
    }

    /// Ascending-revision receipts, with no further I/O. Each still requires owner/frontier
    /// validation at use; extracting it does not authorize consumer access or publication.
    pub fn into_proofs(self) -> std::vec::IntoIter<CertificateAnchorProof> {
        self.proofs.into_iter()
    }
}

impl<F, W, E, I> JournalStore<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Authenticate at most 64 contiguous anchors with one suffix proof and a reverse window
    /// pass. `suffix_limits` bounds the initial proof; `maximum_encoded_bytes` separately
    /// bounds all acquisition reads, including the repeated last certificate. No receipt
    /// escapes until every selected certificate succeeds. No resident map is consulted.
    pub fn authenticate_certificate_window(
        &self,
        filesystem: &mut F,
        first: CommitRevision,
        last: CommitRevision,
        suffix_limits: CertificateAnchorReadLimits,
        maximum_encoded_bytes: u64,
    ) -> Result<CertificateProofWindow, StorageError> {
        let frontier = self.checkpoint_anchor().ok_or(StorageError::InvalidState)?;
        if self.poisoned || first > last || last > frontier.0 {
            return Err(StorageError::InvalidState);
        }
        let count = last.get() - first.get() + 1;
        let suffix_count = frontier.0.get() - last.get() + 1;
        let certificates = suffix_count
            .checked_add(if count == 1 { 0 } else { count })
            .ok_or(StorageError::ResourceLimit)?;
        let encoded_bytes = certificates
            .checked_mul(SMALL_ENVELOPE_BYTES)
            .ok_or(StorageError::ResourceLimit)?;
        if count > MAX_CERTIFICATE_PROOF_WINDOW || encoded_bytes > maximum_encoded_bytes {
            return Err(StorageError::ResourceLimit);
        }
        // Validate both allowances before allocation or any filesystem operation.
        if suffix_count > suffix_limits.maximum_certificates()
            || suffix_count * SMALL_ENVELOPE_BYTES > suffix_limits.maximum_encoded_bytes()
        {
            return Err(StorageError::ResourceLimit);
        }
        let mut proofs = Vec::new();
        proofs
            .try_reserve_exact(count as usize)
            .map_err(|_| StorageError::ResourceLimit)?;
        let terminal = self.authenticate_certificate_revision(filesystem, last, suffix_limits)?;
        if count == 1 {
            let report = terminal.report();
            proofs.push(terminal);
            return Ok(CertificateProofWindow { proofs, report });
        }
        let mut expected = terminal.anchor().1;
        let report = CertificateAnchorReadReport {
            certificates,
            encoded_bytes,
        };
        for sequence in (first.get()..=last.get()).rev() {
            let revision =
                CommitRevision::new(sequence).map_err(|_| StorageError::IntegrityFailure)?;
            let encoded = read_bounded(
                filesystem,
                &self.certificate_file,
                sequence
                    .checked_mul(SMALL_ENVELOPE_BYTES)
                    .ok_or(StorageError::ResourceLimit)?,
                SMALL_ENVELOPE_BYTES,
            )?;
            let digest = sha256(&encoded);
            if digest != expected {
                return Err(StorageError::IntegrityFailure);
            }
            let certificate = decode_certificate(
                &self.vault,
                self.database,
                self.epoch,
                self.certificate_log_id,
                self.writer,
                revision,
                &encoded,
            )?;
            if certificate.revision != sequence
                || certificate.group_sequence != sequence
                || (sequence == 1 && certificate.previous_digest != [0; 32])
            {
                return Err(StorageError::IntegrityFailure);
            }
            expected = certificate.previous_digest;
            let mut proof = terminal.clone();
            proof.anchor = (revision, digest);
            proof.report = report;
            proofs.push(proof);
        }
        proofs.reverse();
        Ok(CertificateProofWindow { proofs, report })
    }
}
