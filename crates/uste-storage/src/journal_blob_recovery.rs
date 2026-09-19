use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlobCatalogRecovery {
    /// Admit an exact current candidate; rebuild only when no current candidate is discovered.
    AdmitOrRebuild,
    /// Explicitly reconstruct derived state from the authenticated journal, ignoring candidates.
    Rebuild,
}

/// Opt-in cold recovery bounds. Each validation/replay pass has its own encoded range and
/// logical payload-verification allowance. Repeated inventories are charged each time.
#[derive(Clone, Copy, Debug)]
pub struct BlobRecoveryLimits {
    pub catalog: BlobMetadataRebuildLimits,
    pub catalog_recovery: BlobCatalogRecovery,
    pub maximum_verified_blob_bytes_per_pass: u64,
    pub maximum_uncommitted_segment_tails: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BlobRecoveryScanReport {
    pub groups: u64,
    pub encoded_group_certificate_bytes: u64,
    pub reference_bindings: u64,
    pub verified_blob_bytes: u64,
    pub maximum_live_inventory_references: usize,
    pub peak_pending_segment_tails: usize,
}

/// Work is split by pass. Encoded range bytes exclude independently bounded inventory/header
/// reads; logical verified payload bytes are not filesystem/device bytes or peak RSS.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BlobRecoveryReport {
    pub validation: BlobRecoveryScanReport,
    pub replay: BlobRecoveryScanReport,
    pub used_existing_catalog: bool,
    pub discovery_certificate_bytes: u64,
    pub admission: Option<BlobMetadataAdmissionReport>,
    pub rebuild: Option<BlobMetadataRebuildReport>,
}

pub(super) struct DiskBlobRecoveryState {
    pub(super) base: Option<BlobMetadataBase>,
    pub(super) pending: blob_metadata::BlobMetadataPending,
    report: BlobRecoveryReport,
}

impl<F, W, E, I> JournalStore<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Cold-open without history-sized certificate/blob/inventory/namespace maps. Every committed
    /// payload is verified before any replay callback; the disk catalog is independently checked.
    /// Nonempty legacy inventory appends are deliberately unavailable in this mode.
    #[allow(clippy::too_many_arguments)]
    pub fn open_with_disk_blob_metadata<A, V>(
        filesystem: &mut F,
        final_name: &EntryName,
        expected_database: DatabaseId,
        vault_entropy: E,
        identity_entropy: I,
        key_adapter: &mut A,
        limits: BlobRecoveryLimits,
        cache: &mut PageCache,
        visitor: V,
    ) -> Result<(Self, RecoveryReport), StorageError>
    where
        A: KeyAdapter<Envelope = W>,
        V: FnMut(RecoveredGroup<'_>) -> Result<(), StorageError>,
    {
        blob_metadata::validate_counts(BlobMetadataCounts::default(), limits.catalog.admission)?;
        if limits.maximum_uncommitted_segment_tails as u64
            > CERTIFICATE_LOG_LIMIT / SMALL_ENVELOPE_BYTES - 1
        {
            return Err(StorageError::ResourceLimit);
        }
        Self::open_internal(
            filesystem,
            final_name,
            expected_database,
            vault_entropy,
            identity_entropy,
            key_adapter,
            Some(limits.catalog.admission.certificates),
            Some((limits, cache)),
            visitor,
        )
    }

    /// Historical admitted base; empty journals have no base. Empty-inventory appends do not
    /// change its blob contents, but this handle's recorded revision remains historical.
    pub fn disk_blob_metadata(&self) -> Option<&BlobMetadataBase> {
        self.disk_blob_recovery
            .as_ref()
            .and_then(|state| state.base.as_ref())
    }

    pub fn blob_recovery_report(&self) -> Option<&BlobRecoveryReport> {
        self.disk_blob_recovery.as_ref().map(|state| &state.report)
    }

    /// Trusted diagnostics: legacy residency enabled, references, inventory IDs, namespaces.
    pub fn blob_metadata_residency(&self) -> (bool, usize, usize, usize) {
        (
            self.disk_blob_recovery.is_none(),
            self.committed_blobs.len(),
            self.committed_blob_inventories.len(),
            self.committed_blob_bytes.len(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_disk_blob_recovery<V>(
        mut self,
        filesystem: &mut F,
        manifest: Manifest,
        initial_segment_file: F::File,
        certificate_count: u64,
        complete_certificate_bytes: u64,
        repaired_certificate_tail_bytes: u64,
        validated: ScanState<F::File>,
        limits: BlobRecoveryLimits,
        cache: &mut PageCache,
        mut visitor: V,
    ) -> Result<(Self, RecoveryReport), StorageError>
    where
        V: FnMut(RecoveredGroup<'_>) -> Result<(), StorageError>,
    {
        let mut work = BlobRecoveryReport {
            validation: validated.blob_work.clone(),
            ..Default::default()
        };
        let mut base = None;
        let mut staged = false;
        if let Some(frontier) = self.frontier {
            let (roots, discovery) = if limits.catalog_recovery == BlobCatalogRecovery::Rebuild {
                (
                    Vec::new(),
                    CertificateAnchorReadReport {
                        certificates: 0,
                        encoded_bytes: 0,
                    },
                )
            } else {
                self.load_proven_index_root_manifests(
                    filesystem,
                    blob_metadata::metadata_scope(self.database),
                    BLOB_METADATA_PROFILE_V1,
                    limits.catalog.admission.certificates,
                )?
            };
            work.discovery_certificate_bytes = discovery.encoded_bytes;
            if let Some(root) = roots.into_iter().find(|root| root.revision() == frontier) {
                // An exact candidate that fails semantic admission is not silently accepted or
                // replaced by an older frontier. Explicit reconstruction is a separate operation.
                let (admitted, report) =
                    self.admit_blob_metadata(filesystem, root, limits.catalog.admission, cache)?;
                work.used_existing_catalog = true;
                work.admission = Some(report);
                base = Some(admitted);
            } else {
                let (admitted, report) =
                    self.stage_blob_metadata_rebuild(filesystem, limits.catalog, cache)?;
                work.rebuild = Some(report);
                base = Some(admitted);
                staged = true;
            }
            if base.as_ref().is_none_or(|base| {
                base.counts().reference_bindings != validated.committed_blob_reference_bindings
            }) {
                return Err(StorageError::IntegrityFailure);
            }
        }
        // No journal repair or replay is performed until full payload and catalog validation.
        if repaired_certificate_tail_bytes != 0 {
            filesystem.set_len(
                &self.certificate_file,
                SMALL_ENVELOPE_BYTES + complete_certificate_bytes,
            )?;
        }
        filesystem.sync_data(&self.certificate_file)?;
        let mut ignored_uncommitted_journal_bytes = 0_u64;
        for tail in &validated.uncommitted_tails {
            let file = filesystem
                .open_existing(&self.database_directory, &segment_name(tail.segment_id)?)
                .map_err(recovery_adapter_error)?;
            filesystem.set_len(&file, tail.committed_end)?;
            filesystem.sync_data(&file)?;
            ignored_uncommitted_journal_bytes = ignored_uncommitted_journal_bytes
                .checked_add(tail.extra_bytes)
                .ok_or(StorageError::ResourceLimit)?;
        }
        if staged {
            base = Some(self.publish_admitted_blob_metadata(
                filesystem,
                base.ok_or(StorageError::InvalidState)?,
                limits.catalog.admission.run,
            )?);
        }
        let replayed = scan_certificates(
            filesystem,
            &self.vault,
            &self.database_directory,
            &self.certificate_file,
            initial_segment_file,
            manifest,
            self.epoch,
            self.writer,
            certificate_count,
            &BTreeMap::new(),
            false,
            Some(limits),
            true,
            &mut visitor,
        )?;
        if replayed.current_segment_id != validated.current_segment_id
            || replayed.current_segment_offset != validated.current_segment_offset
            || replayed.frontier != validated.frontier
            || replayed.previous_certificate_digest != validated.previous_certificate_digest
            || replayed.committed_blob_reference_bindings
                != validated.committed_blob_reference_bindings
            || replayed.blob_work.verified_blob_bytes != validated.blob_work.verified_blob_bytes
            || !replayed.certificate_anchors.is_empty()
            || !replayed.committed_blobs.is_empty()
            || !replayed.committed_blob_inventories.is_empty()
            || !replayed.committed_blob_bytes.is_empty()
            || !replayed.verified_blob_inventories.is_empty()
        {
            return Err(StorageError::IntegrityFailure);
        }
        work.replay = replayed.blob_work;
        let report = RecoveryReport {
            frontier: self.frontier,
            certificate_digest: self.previous_certificate_digest,
            repaired_certificate_tail_bytes,
            ignored_uncommitted_journal_bytes,
        };
        let counts = base
            .as_ref()
            .map_or(BlobMetadataCounts::default(), BlobMetadataBase::counts);
        self.disk_blob_recovery = Some(DiskBlobRecoveryState {
            base,
            pending: blob_metadata::BlobMetadataPending::new(counts),
            report: work,
        });
        Ok((self, report))
    }
}
