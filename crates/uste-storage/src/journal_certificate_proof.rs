use super::*;
use std::sync::Arc;

#[path = "journal_certificate_proof/window.rs"]
mod window;
pub use window::{CertificateProofWindow, MAX_CERTIFICATE_PROOF_WINDOW};

/// Process-local identity only; retaining it holds neither an ownership lock nor a key.
pub(super) struct CertificateProofOwner;

/// Explicit certificate-only work allowance. This is not a group/blob or memory reservation.
#[derive(Clone, Copy, Debug)]
pub struct CertificateAnchorReadLimits {
    maximum_certificates: u64,
    maximum_encoded_bytes: u64,
}

impl CertificateAnchorReadLimits {
    pub const fn maximum_certificates(self) -> u64 {
        self.maximum_certificates
    }
    pub const fn maximum_encoded_bytes(self) -> u64 {
        self.maximum_encoded_bytes
    }
    pub fn new(
        maximum_certificates: u64,
        maximum_encoded_bytes: u64,
    ) -> Result<Self, StorageError> {
        if maximum_certificates == 0
            || maximum_certificates > CERTIFICATE_LOG_LIMIT / SMALL_ENVELOPE_BYTES - 1
            || maximum_encoded_bytes == 0
            || maximum_encoded_bytes > maximum_certificates * SMALL_ENVELOPE_BYTES
        {
            return Err(StorageError::ResourceLimit);
        }
        Ok(Self {
            maximum_certificates,
            maximum_encoded_bytes,
        })
    }
}

/// Complete successful proof work; excludes group, segment header, inventory and blob reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CertificateAnchorReadReport {
    pub certificates: u64,
    pub encoded_bytes: u64,
}

/// Opaque evidence connecting an exact historical certificate to this authenticated frontier.
/// This is content binding, not consumer authorization or transaction/domain validation.
#[derive(Clone)]
pub struct CertificateAnchorProof {
    owner: Arc<CertificateProofOwner>,
    database: DatabaseId,
    epoch: KeyEpoch,
    writer: WriterIncarnationId,
    log: [u8; 16],
    anchor: (CommitRevision, [u8; 32]),
    frontier: (CommitRevision, [u8; 32]),
    report: CertificateAnchorReadReport,
}

/// One bounded index cursor carrying its own fixed-size historical certificate evidence.
pub struct ProvenIndexRunCursor<F: FileSystem> {
    proof: CertificateAnchorProof,
    cursor: IndexRunCursor<F>,
}

impl fmt::Debug for CertificateAnchorProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CertificateAnchorProof")
            .field("revision", &self.anchor.0)
            .field("frontier", &self.frontier.0)
            .finish_non_exhaustive()
    }
}

impl CertificateAnchorProof {
    pub(crate) fn same_owner_as(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.owner, &other.owner)
    }
    pub fn anchor(&self) -> (CommitRevision, [u8; 32]) {
        self.anchor
    }
    pub fn report(&self) -> CertificateAnchorReadReport {
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
    /// Prove an exact certificate using only on-disk certificates and the terminal anchor.
    /// Admission precedes I/O; one certificate is retained at a time. The resident historical
    /// anchor map is neither consulted nor rebuilt. Cost is linear in the suffix length.
    pub fn authenticate_certificate_anchor(
        &self,
        filesystem: &mut F,
        revision: CommitRevision,
        certificate_digest: [u8; 32],
        limits: CertificateAnchorReadLimits,
    ) -> Result<CertificateAnchorProof, StorageError> {
        self.authenticate_certificate_internal(
            filesystem,
            revision,
            Some(certificate_digest),
            limits,
        )
    }

    /// Resolve an exact revision directly against the pinned terminal certificate. Callers with
    /// an independently expected digest should use `authenticate_certificate_anchor` instead.
    /// This performs the same bounded chain authentication, without a preliminary digest read.
    pub fn authenticate_certificate_revision(
        &self,
        filesystem: &mut F,
        revision: CommitRevision,
        limits: CertificateAnchorReadLimits,
    ) -> Result<CertificateAnchorProof, StorageError> {
        self.authenticate_certificate_internal(filesystem, revision, None, limits)
    }

    fn authenticate_certificate_internal(
        &self,
        filesystem: &mut F,
        revision: CommitRevision,
        mut certificate_digest: Option<[u8; 32]>,
        limits: CertificateAnchorReadLimits,
    ) -> Result<CertificateAnchorProof, StorageError> {
        let frontier = self.checkpoint_anchor().ok_or(StorageError::InvalidState)?;
        if self.poisoned || revision > frontier.0 {
            return Err(StorageError::InvalidState);
        }
        let certificates = frontier.0.get() - revision.get() + 1;
        let encoded_bytes = certificates
            .checked_mul(SMALL_ENVELOPE_BYTES)
            .ok_or(StorageError::ResourceLimit)?;
        if certificates > limits.maximum_certificates
            || encoded_bytes > limits.maximum_encoded_bytes
        {
            return Err(StorageError::ResourceLimit);
        }
        let mut previous = None;
        for sequence in revision.get()..=frontier.0.get() {
            let current =
                CommitRevision::new(sequence).map_err(|_| StorageError::IntegrityFailure)?;
            let offset = sequence
                .checked_mul(SMALL_ENVELOPE_BYTES)
                .ok_or(StorageError::ResourceLimit)?;
            let encoded = read_bounded(
                filesystem,
                &self.certificate_file,
                offset,
                SMALL_ENVELOPE_BYTES,
            )?;
            let digest = sha256(&encoded);
            if sequence == revision.get() {
                if certificate_digest.is_some_and(|expected| digest != expected) {
                    return Err(StorageError::IntegrityFailure);
                }
                certificate_digest = Some(digest);
            }
            let certificate = decode_certificate(
                &self.vault,
                self.database,
                self.epoch,
                self.certificate_log_id,
                self.writer,
                current,
                &encoded,
            )?;
            if certificate.revision != sequence
                || certificate.group_sequence != sequence
                || previous.is_some_and(|prior| certificate.previous_digest != prior)
                || (sequence == 1 && certificate.previous_digest != [0; 32])
            {
                return Err(StorageError::IntegrityFailure);
            }
            previous = Some(digest);
        }
        if previous != Some(frontier.1) {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(CertificateAnchorProof {
            owner: Arc::clone(&self.proof_owner),
            database: self.database,
            epoch: self.epoch,
            writer: self.writer,
            log: self.certificate_log_id,
            anchor: (
                revision,
                certificate_digest.ok_or(StorageError::IntegrityFailure)?,
            ),
            frontier,
            report: CertificateAnchorReadReport {
                certificates,
                encoded_bytes,
            },
        })
    }

    /// Recheck the journal context and content frontier without I/O. Any append invalidates it.
    pub fn validate_certificate_anchor_proof(
        &self,
        proof: &CertificateAnchorProof,
    ) -> Result<(), StorageError> {
        self.validate_historical_certificate_proof(proof)?;
        if self.checkpoint_anchor() != Some(proof.frontier) {
            return Err(StorageError::InvalidState);
        }
        Ok(())
    }

    pub(super) fn validate_historical_certificate_proof(
        &self,
        proof: &CertificateAnchorProof,
    ) -> Result<(), StorageError> {
        if self.poisoned
            || !Arc::ptr_eq(&self.proof_owner, &proof.owner)
            || proof.database != self.database
            || proof.epoch != self.epoch
            || proof.writer != self.writer
            || proof.log != self.certificate_log_id
            || self
                .frontier
                .is_none_or(|frontier| frontier < proof.frontier.0)
            || (self.frontier == Some(proof.frontier.0)
                && self.previous_certificate_digest != proof.frontier.1)
        {
            return Err(StorageError::InvalidState);
        }
        Ok(())
    }

    pub(super) fn validate_proven_index_root(
        &self,
        root: &RecoveredIndexRoot,
        proof: &CertificateAnchorProof,
    ) -> Result<(), StorageError> {
        self.validate_historical_certificate_proof(proof)?;
        if root.scope().database() != self.database
            || (root.revision(), *root.certificate_digest()) != proof.anchor
        {
            return Err(StorageError::InvalidState);
        }
        Ok(())
    }

    /// Attach already authenticated evidence to a root's existing bounded read handle. This
    /// does not scrub its runs or establish domain semantics, durability or consumer authority.
    pub fn bind_index_root_certificate(
        &self,
        mut root: RecoveredIndexRoot,
        proof: CertificateAnchorProof,
    ) -> Result<RecoveredIndexRoot, StorageError> {
        self.validate_proven_index_root(&root, &proof)?;
        root.certificate_proof = Some(proof);
        Ok(root)
    }

    /// Discover fixed root slots and bind each non-future manifest directly to the journal.
    /// Limits apply per candidate (at most two); report totals both candidates' proof reads.
    /// A certificate-proof error is fatal, not silently treated as an absent optional root.
    pub fn load_proven_index_root_manifests(
        &self,
        filesystem: &mut F,
        scope: NamespaceRef,
        index_profile: [u8; 32],
        limits: CertificateAnchorReadLimits,
    ) -> Result<(Vec<RecoveredIndexRoot>, CertificateAnchorReadReport), StorageError> {
        if self.poisoned || scope.database() != self.database {
            return Err(StorageError::InvalidState);
        }
        let roots = index::load_manifests(
            filesystem,
            IndexContext {
                database: self.database,
                epoch: self.epoch,
                writer: self.writer,
                directory: &self.database_directory,
            },
            &self.vault,
            scope,
            index_profile,
        )?;
        self.bind_discovered_index_roots(filesystem, roots, limits)
    }

    pub(super) fn bind_discovered_index_roots(
        &self,
        filesystem: &mut F,
        roots: Vec<RecoveredIndexRoot>,
        limits: CertificateAnchorReadLimits,
    ) -> Result<(Vec<RecoveredIndexRoot>, CertificateAnchorReadReport), StorageError> {
        let mut bound = Vec::new();
        let mut report = CertificateAnchorReadReport {
            certificates: 0,
            encoded_bytes: 0,
        };
        for root in roots {
            if self
                .frontier
                .is_none_or(|frontier| root.revision() > frontier)
            {
                continue;
            }
            let proof = self.authenticate_certificate_anchor(
                filesystem,
                root.revision(),
                *root.certificate_digest(),
                limits,
            )?;
            report.certificates = report
                .certificates
                .checked_add(proof.report.certificates)
                .ok_or(StorageError::ResourceLimit)?;
            report.encoded_bytes = report
                .encoded_bytes
                .checked_add(proof.report.encoded_bytes)
                .ok_or(StorageError::ResourceLimit)?;
            bound.push(self.bind_index_root_certificate(root, proof)?);
        }
        Ok((bound, report))
    }

    /// Only callers that just validated publication/stage binding may mint this no-I/O handle.
    pub(super) fn current_certificate_anchor_proof(
        &self,
    ) -> Result<CertificateAnchorProof, StorageError> {
        if self.poisoned {
            return Err(StorageError::InvalidState);
        }
        let frontier = self.checkpoint_anchor().ok_or(StorageError::InvalidState)?;
        Ok(CertificateAnchorProof {
            owner: Arc::clone(&self.proof_owner),
            database: self.database,
            epoch: self.epoch,
            writer: self.writer,
            log: self.certificate_log_id,
            anchor: frontier,
            frontier,
            report: CertificateAnchorReadReport {
                certificates: 0,
                encoded_bytes: 0,
            },
        })
    }

    /// Only callers that just validated publication/stage binding may mint this no-I/O handle.
    pub(super) fn attach_certified_root(
        &self,
        mut root: RecoveredIndexRoot,
    ) -> Result<RecoveredIndexRoot, StorageError> {
        if self.certificate_read_limits.is_some() {
            let frontier = self.checkpoint_anchor().ok_or(StorageError::InvalidState)?;
            root.certificate_proof = Some(CertificateAnchorProof {
                owner: Arc::clone(&self.proof_owner),
                database: self.database,
                epoch: self.epoch,
                writer: self.writer,
                log: self.certificate_log_id,
                anchor: (root.revision(), *root.certificate_digest()),
                frontier,
                report: CertificateAnchorReadReport {
                    certificates: 0,
                    encoded_bytes: 0,
                },
            });
        }
        Ok(root)
    }

    pub(super) fn certificate_anchor_matches(
        &self,
        filesystem: &mut F,
        revision: CommitRevision,
        digest: [u8; 32],
    ) -> Result<bool, StorageError> {
        if let Some(limits) = self.certificate_read_limits {
            if self.frontier.is_none_or(|frontier| revision > frontier) {
                return Ok(false);
            }
            self.authenticate_certificate_anchor(filesystem, revision, digest, limits)
                .map(|_| true)
        } else {
            Ok(self.certificate_anchors.get(&revision) == Some(&digest))
        }
    }

    /// Bounded lookup with explicit disk-derived certificate evidence; no resident anchor lookup.
    /// A proof remains valid across successful appends by this same exclusive owner, never reopen.
    #[allow(clippy::too_many_arguments)]
    pub fn index_get_proven(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        proof: &CertificateAnchorProof,
        family: u8,
        key: &[u8],
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<(Option<Vec<u8>>, IndexReadStats), StorageError> {
        self.validate_proven_index_root(root, proof)?;
        index::get_bounded(
            filesystem,
            &IndexContext {
                database: self.database,
                epoch: self.epoch,
                writer: self.writer,
                directory: &self.database_directory,
            },
            &self.vault,
            root,
            family,
            key,
            limits,
            cache,
        )
    }

    pub fn open_proven_index_run_cursor(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        proof: CertificateAnchorProof,
        family: u8,
        limits: IndexRunReadLimits,
    ) -> Result<ProvenIndexRunCursor<F>, StorageError> {
        self.validate_proven_index_root(root, &proof)?;
        let cursor = index::open_run_cursor(
            filesystem,
            &IndexContext {
                database: self.database,
                epoch: self.epoch,
                writer: self.writer,
                directory: &self.database_directory,
            },
            &self.vault,
            root,
            family,
            limits,
        )?;
        Ok(ProvenIndexRunCursor { proof, cursor })
    }

    pub fn next_proven_index_run_entry(
        &self,
        filesystem: &mut F,
        cursor: &mut ProvenIndexRunCursor<F>,
    ) -> Result<Option<IndexEntry>, StorageError> {
        self.validate_proven_index_root(cursor.cursor.root(), &cursor.proof)?;
        index::next_run_entry(
            filesystem,
            &IndexContext {
                database: self.database,
                epoch: self.epoch,
                writer: self.writer,
                directory: &self.database_directory,
            },
            &self.vault,
            &mut cursor.cursor,
        )
    }

    pub fn finish_proven_index_run_cursor(
        &self,
        cursor: ProvenIndexRunCursor<F>,
    ) -> Result<IndexRunReadReport, StorageError> {
        self.validate_proven_index_root(cursor.cursor.root(), &cursor.proof)?;
        index::finish_run_cursor(cursor.cursor)
    }
}
