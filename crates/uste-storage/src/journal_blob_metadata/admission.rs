use super::*;

impl<F, W, E, I> JournalStore<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Independently authenticate a published storage catalog against its complete journal prefix.
    /// No resident blob/inventory/namespace map is used as comparator or reconstructed here.
    pub fn admit_blob_metadata(
        &self,
        filesystem: &mut F,
        root: RecoveredIndexRoot,
        limits: BlobMetadataAdmissionLimits,
        cache: &mut PageCache,
    ) -> Result<(BlobMetadataBase, BlobMetadataAdmissionReport), StorageError> {
        if root.generation() == 0 {
            return Err(StorageError::InvalidState);
        }
        self.admit_blob_metadata_internal(filesystem, root, limits, cache)
    }

    pub(super) fn admit_blob_metadata_internal(
        &self,
        filesystem: &mut F,
        root: RecoveredIndexRoot,
        limits: BlobMetadataAdmissionLimits,
        cache: &mut PageCache,
    ) -> Result<(BlobMetadataBase, BlobMetadataAdmissionReport), StorageError> {
        validate_counts(BlobMetadataCounts::default(), limits)?;
        if self.poisoned
            || root.scope() != metadata_scope(self.database)
            || root.index_profile() != &BLOB_METADATA_PROFILE_V1
            || root.reducer_profile() != &BLOB_METADATA_PROFILE_V1
        {
            return Err(StorageError::InvalidState);
        }
        if root.revision().get() > limits.maximum_journal_groups {
            return Err(StorageError::ResourceLimit);
        }
        let proof = self.authenticate_certificate_anchor(
            filesystem,
            root.revision(),
            *root.certificate_digest(),
            limits.certificates,
        )?;
        let mut report = BlobMetadataAdmissionReport {
            certificate_bytes: proof.report().encoded_bytes,
            ..Default::default()
        };
        let root = self.bind_index_root_certificate(root, proof)?;
        let value = self
            .blob_metadata_lookup(
                filesystem,
                &root,
                META,
                META_KEY,
                limits.lookup,
                cache,
                &mut report,
            )?
            .ok_or(StorageError::IntegrityFailure)?;
        let counts = decode_counts(&value)?;
        validate_counts(counts, limits)?;
        let expected = [
            (META, 1),
            (BLOBS, counts.blobs),
            (NAMESPACES, counts.namespaces),
            (INVENTORIES, counts.inventories),
        ]
        .into_iter()
        .filter(|(_, count)| *count != 0)
        .collect::<Vec<_>>();
        let runs = root.runs().copied().collect::<Vec<_>>();
        if runs
            .iter()
            .map(|run| (run.family(), run.entry_count()))
            .collect::<Vec<_>>()
            != expected
            || state_digest(root.revision(), counts, &runs) != *root.logical_state_digest()
        {
            return Err(StorageError::IntegrityFailure);
        }
        let mut namespace = None;
        let mut namespace_count = 0_u64;
        for run in &runs {
            let mut cursor =
                self.open_index_run_cursor(filesystem, &root, run.family(), limits.run)?;
            while let Some(entry) = self.next_index_run_entry(filesystem, &mut cursor)? {
                match run.family() {
                    META => {
                        if entry.key != META_KEY || decode_counts(&entry.value)? != counts {
                            return Err(StorageError::IntegrityFailure);
                        }
                    }
                    BLOBS => {
                        let (reference, _) = decode_reference(
                            self.database,
                            root.revision(),
                            &entry.key,
                            &entry.value,
                        )?;
                        if let Some((scope, bytes)) = namespace
                            && scope != reference.scope()
                        {
                            self.check_blob_namespace_total(
                                filesystem,
                                &root,
                                scope,
                                bytes,
                                limits.lookup,
                                cache,
                                &mut report,
                            )?;
                            namespace_count = checked_add(namespace_count, 1)?;
                            namespace = None;
                        }
                        let bytes = checked_add(
                            namespace.map_or(0, |(_, bytes)| bytes),
                            reference.byte_len(),
                        )?;
                        if bytes > MAX_NAMESPACE_BLOB_BYTES {
                            return Err(StorageError::IntegrityFailure);
                        }
                        namespace = Some((reference.scope(), bytes));
                    }
                    NAMESPACES => {
                        decode_namespace(self.database, &entry.key, &entry.value)?;
                    }
                    INVENTORIES => {
                        decode_inventory(root.revision(), &entry.key, &entry.value)?;
                    }
                    _ => return Err(StorageError::IntegrityFailure),
                }
            }
            let work = self.finish_index_run_cursor(cursor)?;
            report.run_pages = checked_add(report.run_pages, work.stats.pages_read)?;
            report.run_entries = checked_add(report.run_entries, work.entries)?;
        }
        if let Some((scope, bytes)) = namespace {
            self.check_blob_namespace_total(
                filesystem,
                &root,
                scope,
                bytes,
                limits.lookup,
                cache,
                &mut report,
            )?;
            namespace_count = checked_add(namespace_count, 1)?;
        }
        if namespace_count != counts.namespaces {
            return Err(StorageError::IntegrityFailure);
        }
        let mut first_blobs = 0_u64;
        let mut first_inventories = 0_u64;
        let mut bindings = 0_u64;
        // Each declared first revision must match exactly once and precede every use. These
        // correspondence/count checks are order-independent; construction remains forward.
        report.journal = self.visit_committed_range_reverse_report(
            filesystem,
            CommitRevision::FIRST,
            root.revision(),
            limits.maximum_journal_groups,
            limits.maximum_journal_encoded_bytes,
            |filesystem, group| {
                if let Some(inventory) = group.blob_inventory {
                    bindings = checked_add(bindings, inventory.references().len() as u64)?;
                    if bindings > counts.reference_bindings {
                        return Err(StorageError::IntegrityFailure);
                    }
                    let value = self
                        .blob_metadata_lookup(
                            filesystem,
                            &root,
                            INVENTORIES,
                            &inventory.digest(),
                            limits.lookup,
                            cache,
                            &mut report,
                        )?
                        .ok_or(StorageError::IntegrityFailure)?;
                    let (expected_count, first) =
                        decode_inventory(root.revision(), &inventory.digest(), &value)?;
                    if expected_count != inventory.references().len() || first > group.revision {
                        return Err(StorageError::IntegrityFailure);
                    }
                    if first == group.revision {
                        first_inventories = checked_add(first_inventories, 1)?;
                    }
                    for reference in inventory.references() {
                        let key = reference_key(*reference);
                        let value = self
                            .blob_metadata_lookup(
                                filesystem,
                                &root,
                                BLOBS,
                                &key,
                                limits.lookup,
                                cache,
                                &mut report,
                            )?
                            .ok_or(StorageError::IntegrityFailure)?;
                        let (expected, first) =
                            decode_reference(self.database, root.revision(), &key, &value)?;
                        if expected != *reference || first > group.revision {
                            return Err(StorageError::IntegrityFailure);
                        }
                        if first == group.revision {
                            first_blobs = checked_add(first_blobs, 1)?;
                        }
                    }
                }
                Ok(())
            },
        )?;
        if bindings != counts.reference_bindings
            || first_blobs != counts.blobs
            || first_inventories != counts.inventories
        {
            return Err(StorageError::IntegrityFailure);
        }
        Ok((BlobMetadataBase { root, counts }, report))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn blob_metadata_lookup(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        family: u8,
        key: &[u8],
        limits: IndexGetLimits,
        cache: &mut PageCache,
        report: &mut BlobMetadataAdmissionReport,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        self.validate_index_cursor_root(root)?;
        if !has_family(root, family) {
            return Ok(None);
        }
        let (value, stats) =
            self.index_get_bounded(filesystem, root, family, key, limits, cache)?;
        report.lookup_pages = checked_add(report.lookup_pages, stats.pages_read)?;
        report.lookup_cache_hits = checked_add(report.lookup_cache_hits, stats.cache_hits)?;
        report.lookup_fragments = checked_add(report.lookup_fragments, stats.fragments_visited)?;
        report.lookup_result_bytes = checked_add(report.lookup_result_bytes, stats.result_bytes)?;
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    fn check_blob_namespace_total(
        &self,
        filesystem: &mut F,
        root: &RecoveredIndexRoot,
        scope: NamespaceRef,
        expected: u64,
        limits: IndexGetLimits,
        cache: &mut PageCache,
        report: &mut BlobMetadataAdmissionReport,
    ) -> Result<(), StorageError> {
        let key = *scope.namespace().as_bytes();
        let value = self
            .blob_metadata_lookup(filesystem, root, NAMESPACES, &key, limits, cache, report)?
            .ok_or(StorageError::IntegrityFailure)?;
        if decode_namespace(self.database, &key, &value)? != (scope, expected) {
            return Err(StorageError::IntegrityFailure);
        }
        Ok(())
    }

    /// Trusted exact lookup from the admitted disk catalog, not current consumer authorization.
    pub fn blob_metadata_reference(
        &self,
        filesystem: &mut F,
        base: &BlobMetadataBase,
        scope: NamespaceRef,
        id: crate::blob::BlobId,
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<Option<(BlobReference, CommitRevision)>, StorageError> {
        if scope.database() != self.database {
            return Err(StorageError::InvalidState);
        }
        let mut key = [0; 32];
        key[..16].copy_from_slice(scope.namespace().as_bytes());
        key[16..].copy_from_slice(&id.as_bytes());
        self.blob_metadata_lookup(
            filesystem,
            &base.root,
            BLOBS,
            &key,
            limits,
            cache,
            &mut BlobMetadataAdmissionReport::default(),
        )?
        .map(|value| decode_reference(self.database, base.root.revision(), &key, &value))
        .transpose()
    }

    /// Exact historical namespace logical bytes, not a staged/current principal quota answer.
    pub fn blob_metadata_namespace_bytes(
        &self,
        filesystem: &mut F,
        base: &BlobMetadataBase,
        scope: NamespaceRef,
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<Option<u64>, StorageError> {
        if scope.database() != self.database {
            return Err(StorageError::InvalidState);
        }
        let key = *scope.namespace().as_bytes();
        self.blob_metadata_lookup(
            filesystem,
            &base.root,
            NAMESPACES,
            &key,
            limits,
            cache,
            &mut BlobMetadataAdmissionReport::default(),
        )?
        .map(|value| decode_namespace(self.database, &key, &value).map(|(_, bytes)| bytes))
        .transpose()
    }
}
