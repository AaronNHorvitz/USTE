use super::*;

impl<F, W, E, I> JournalStore<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Rebuild derived storage metadata using one inventory and bounded immutable merges at a
    /// time. Intermediate roots are private; the complete prefix is independently admitted
    /// before terminal publication. This does not remove legacy journal-open resident maps.
    pub fn rebuild_blob_metadata(
        &mut self,
        filesystem: &mut F,
        limits: BlobMetadataRebuildLimits,
        cache: &mut PageCache,
    ) -> Result<(BlobMetadataBase, BlobMetadataRebuildReport), StorageError> {
        let (base, report) = self.stage_blob_metadata_rebuild(filesystem, limits, cache)?;
        let base = self.publish_admitted_blob_metadata(filesystem, base, limits.admission.run)?;
        Ok((base, report))
    }

    pub(in crate::journal) fn stage_blob_metadata_rebuild(
        &mut self,
        filesystem: &mut F,
        limits: BlobMetadataRebuildLimits,
        cache: &mut PageCache,
    ) -> Result<(BlobMetadataBase, BlobMetadataRebuildReport), StorageError> {
        validate_counts(BlobMetadataCounts::default(), limits.admission)?;
        let frontier = self.frontier.ok_or(StorageError::InvalidState)?;
        if self.poisoned {
            return Err(StorageError::InvalidState);
        }
        if frontier.get() > limits.admission.maximum_journal_groups {
            return Err(StorageError::ResourceLimit);
        }
        let mut counts = BlobMetadataCounts::default();
        let mut staged: Option<crate::StagedIndexRoot> = None;
        let mut report = BlobMetadataRebuildReport::default();
        for number in 1..=frontier.get() {
            let revision = CommitRevision::new(number).map_err(|_| StorageError::InvalidState)?;
            let remaining = limits
                .admission
                .maximum_journal_encoded_bytes
                .checked_sub(report.journal.encoded_bytes)
                .ok_or(StorageError::ResourceLimit)?;
            let mut inventory = None;
            let work = self.visit_committed_range_report(
                filesystem,
                revision,
                revision,
                1,
                remaining,
                |_, group| {
                    inventory = group.blob_inventory.cloned();
                    Ok(())
                },
            )?;
            report.journal.groups = checked_add(report.journal.groups, work.groups)?;
            report.journal.encoded_bytes =
                checked_add(report.journal.encoded_bytes, work.encoded_bytes)?;
            if inventory.is_none() && revision != frontier {
                continue;
            }
            let base = staged.as_ref().map(crate::StagedIndexRoot::read_root);
            let previous = counts;
            let mut deltas: [Vec<IndexDelta>; 4] = Default::default();
            if let Some(inventory) = inventory {
                let mut lookup = |family, key: &[u8]| match base {
                    Some(root) => self.blob_metadata_lookup(
                        filesystem,
                        root,
                        family,
                        key,
                        limits.admission.lookup,
                        cache,
                        &mut report.lookups,
                    ),
                    None => Ok(None),
                };
                let mut new_bytes = 0_u64;
                for reference in inventory.references() {
                    let key = reference_key(*reference);
                    if let Some(value) = lookup(BLOBS, &key)? {
                        if decode_reference(self.database, revision, &key, &value)?.0 != *reference
                        {
                            return Err(StorageError::IntegrityFailure);
                        }
                    } else {
                        counts.blobs = checked_add(counts.blobs, 1)?;
                        new_bytes = checked_add(new_bytes, reference.byte_len())?;
                        deltas[1].push(IndexDelta::new(
                            key.to_vec(),
                            None,
                            Some(encode_reference(*reference, revision).to_vec()),
                        )?);
                    }
                }
                let scope = inventory
                    .references()
                    .first()
                    .ok_or(StorageError::IntegrityFailure)?
                    .scope();
                let key = *scope.namespace().as_bytes();
                let before = lookup(NAMESPACES, &key)?;
                let old_bytes = before
                    .as_ref()
                    .map(|value| {
                        decode_namespace(self.database, &key, value).map(|(_, bytes)| bytes)
                    })
                    .transpose()?
                    .unwrap_or(0);
                let bytes = checked_add(old_bytes, new_bytes)?;
                if bytes > MAX_NAMESPACE_BLOB_BYTES {
                    return Err(StorageError::ResourceLimit);
                }
                if before.is_none() {
                    counts.namespaces = checked_add(counts.namespaces, 1)?;
                }
                if before.is_none() || new_bytes != 0 {
                    deltas[2].push(IndexDelta::new(
                        key.to_vec(),
                        before,
                        Some(bytes.to_be_bytes().to_vec()),
                    )?);
                }
                let key = inventory.digest();
                if let Some(value) = lookup(INVENTORIES, &key)? {
                    if decode_inventory(revision, &key, &value)?.0 != inventory.references().len()
                        || !deltas[1].is_empty()
                    {
                        return Err(StorageError::IntegrityFailure);
                    }
                } else {
                    counts.inventories = checked_add(counts.inventories, 1)?;
                    deltas[3].push(IndexDelta::new(
                        key.to_vec(),
                        None,
                        Some(encode_inventory(inventory.references().len(), revision)?.to_vec()),
                    )?);
                }
                counts.reference_bindings = checked_add(
                    counts.reference_bindings,
                    inventory.references().len() as u64,
                )?;
            }
            validate_counts(counts, limits.admission)?;
            deltas[0].push(IndexDelta::new(
                META_KEY.to_vec(),
                base.map(|_| encode_counts(previous).to_vec()),
                Some(encode_counts(counts).to_vec()),
            )?);
            let proof = self.authenticate_certificate_revision(
                filesystem,
                revision,
                limits.admission.certificates,
            )?;
            report.certificate_bytes =
                checked_add(report.certificate_bytes, proof.report().encoded_bytes)?;
            let stage =
                self.open_proven_index_recovery_stage(metadata_scope(self.database), &proof)?;
            let mut runs = base
                .map(|root| root.runs().copied().collect::<Vec<_>>())
                .unwrap_or_default();
            for (index, changes) in deltas.into_iter().enumerate() {
                // index-v1 binds every run to the root revision. Even an unchanged family
                // must be streamed into that revision; retaining its old descriptor is invalid.
                if changes.is_empty()
                    && base.is_none_or(|root| !has_family(root, (index + 1) as u8))
                {
                    continue;
                }
                let family = (index + 1) as u8;
                let bytes = [
                    META_KEY.len() as u64 + 48,
                    counts
                        .blobs
                        .checked_mul(88)
                        .ok_or(StorageError::ResourceLimit)?,
                    counts
                        .namespaces
                        .checked_mul(24)
                        .ok_or(StorageError::ResourceLimit)?,
                    counts
                        .inventories
                        .checked_mul(48)
                        .ok_or(StorageError::ResourceLimit)?,
                ][index];
                report.merge_output_bytes = checked_add(report.merge_output_bytes, bytes)?;
                if report.merge_output_bytes > limits.maximum_merge_output_bytes {
                    return Err(StorageError::ResourceLimit);
                }
                let merged = self.stage_index_merge_visit(
                    filesystem,
                    &stage,
                    BLOB_METADATA_PROFILE_V1,
                    family,
                    base,
                    limits.merge,
                    changes.into_iter().map(Ok),
                    &mut |_, _| Ok(()),
                )?;
                report.merge_pages_read =
                    checked_add(report.merge_pages_read, merged.report.base.stats.pages_read)?;
                runs.retain(|run| run.family() != family);
                runs.push(merged.run.ok_or(StorageError::IntegrityFailure)?);
            }
            runs.sort_by_key(IndexRunDescriptor::family);
            let input = IndexRootInput {
                scope: metadata_scope(self.database),
                revision,
                certificate_digest: proof.anchor().1,
                reducer_profile: BLOB_METADATA_PROFILE_V1,
                logical_state_digest: state_digest(revision, counts, &runs),
                index_profile: BLOB_METADATA_PROFILE_V1,
            };
            staged = Some(self.finish_index_recovery_stage(stage, input, &runs)?);
        }
        let root = staged
            .ok_or(StorageError::InvalidState)?
            .read_root()
            .clone();
        let (admitted, admission) =
            self.admit_blob_metadata_internal(filesystem, root, limits.admission, cache)?;
        report.admission = admission;
        Ok((admitted, report))
    }

    pub(in crate::journal) fn publish_admitted_blob_metadata(
        &mut self,
        filesystem: &mut F,
        mut admitted: BlobMetadataBase,
        limits: IndexRunReadLimits,
    ) -> Result<BlobMetadataBase, StorageError> {
        let root = &admitted.root;
        let proof = root
            .certificate_proof
            .clone()
            .ok_or(StorageError::InvalidState)?;
        let runs = root.runs().copied().collect::<Vec<_>>();
        let input = IndexRootInput {
            scope: root.scope(),
            revision: root.revision(),
            certificate_digest: *root.certificate_digest(),
            reducer_profile: BLOB_METADATA_PROFILE_V1,
            logical_state_digest: *root.logical_state_digest(),
            index_profile: BLOB_METADATA_PROFILE_V1,
        };
        let published =
            self.publish_index_root_recovered_bounded(filesystem, input, &runs, limits)?;
        admitted.root = self.bind_index_root_certificate(published, proof)?;
        Ok(admitted)
    }
}
