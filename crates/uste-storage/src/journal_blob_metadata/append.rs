use super::*;

/// Explicit per-call bounds for the trusted disk storage writer, not consumer quota policy.
/// Preparing a commit temporarily retains two copies of the bounded pending overlay.
#[derive(Clone, Copy, Debug)]
pub struct DiskBlobAppendLimits {
    pub maximum_pending_blobs: usize,
    pub maximum_pending_inventories: usize,
    pub maximum_pending_namespaces: usize,
    pub maximum_inventory_references: usize,
    pub maximum_verified_blob_bytes: u64,
    pub lookup: IndexGetLimits,
}

#[derive(Clone, Default)]
pub(in crate::journal) struct BlobMetadataPending {
    references: BTreeMap<(NamespaceRef, crate::blob::BlobId), (BlobReference, CommitRevision)>,
    inventories: BTreeMap<[u8; 32], (usize, CommitRevision)>,
    namespaces: BTreeMap<NamespaceRef, u64>,
    counts: BlobMetadataCounts,
}

impl BlobMetadataPending {
    pub(in crate::journal) fn new(counts: BlobMetadataCounts) -> Self {
        Self {
            counts,
            ..Self::default()
        }
    }

    fn admit(&self, limits: DiskBlobAppendLimits) -> Result<(), StorageError> {
        if limits.maximum_pending_blobs > MAX_COMMITTED_BLOBS_PER_JOURNAL
            || limits.maximum_pending_namespaces > MAX_COMMITTED_BLOBS_PER_JOURNAL
            || limits.maximum_pending_inventories as u64
                > CERTIFICATE_LOG_LIMIT / SMALL_ENVELOPE_BYTES - 1
            || limits.maximum_inventory_references > crate::blob::MAX_BLOBS_PER_INVENTORY
            || self.references.len() > limits.maximum_pending_blobs
            || self.inventories.len() > limits.maximum_pending_inventories
            || self.namespaces.len() > limits.maximum_pending_namespaces
        {
            return Err(StorageError::ResourceLimit);
        }
        Ok(())
    }
}

impl<F, W, E, I> JournalStore<F, W, E, I>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    /// Historical inventory commitment from an owner-bound admitted base.
    /// This privileged identity lookup grants no consumer read or write authority.
    pub fn blob_metadata_inventory(
        &self,
        filesystem: &mut F,
        base: &BlobMetadataBase,
        digest: [u8; 32],
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<Option<(usize, CommitRevision)>, StorageError> {
        self.blob_metadata_lookup(
            filesystem,
            &base.root,
            INVENTORIES,
            &digest,
            limits,
            cache,
            &mut BlobMetadataAdmissionReport::default(),
        )?
        .map(|value| decode_inventory(base.revision(), &digest, &value))
        .transpose()
    }

    /// Current trusted reference identity across disk base and bounded pending overlay.
    pub fn disk_blob_reference(
        &self,
        filesystem: &mut F,
        scope: NamespaceRef,
        id: crate::blob::BlobId,
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<Option<(BlobReference, CommitRevision)>, StorageError> {
        if self.poisoned {
            return Err(StorageError::NeedsRecovery);
        }
        if scope.database() != self.database {
            return Err(StorageError::InvalidState);
        }
        let state = self
            .disk_blob_recovery
            .as_ref()
            .ok_or(StorageError::InvalidState)?;
        if let Some(reference) = state.pending.references.get(&(scope, id)) {
            return Ok(Some(*reference));
        }
        match &state.base {
            Some(base) => self.blob_metadata_reference(filesystem, base, scope, id, limits, cache),
            None => Ok(None),
        }
    }

    /// Pending reference/inventory/namespace entries. Historical base counts are separate.
    pub fn disk_blob_pending_residency(&self) -> Option<(usize, usize, usize)> {
        self.disk_blob_recovery.as_ref().map(|state| {
            (
                state.pending.references.len(),
                state.pending.inventories.len(),
                state.pending.namespaces.len(),
            )
        })
    }

    /// Certify a nonempty inventory without reconstructing complete history maps.
    /// All metadata lookup/allocation precedes inventory publication and certification. After
    /// certificate sync only the prepared overlay is installed; errors preserve it or quarantine
    /// the owner according to the existing journal durability contract.
    pub fn append_group_with_disk_inventory(
        &mut self,
        filesystem: &mut F,
        input: CommitInput<'_>,
        inventory: &BlobInventory,
        limits: DiskBlobAppendLimits,
        cache: &mut PageCache,
    ) -> Result<DurableCommit, StorageError> {
        if self.poisoned {
            return Err(StorageError::NeedsRecovery);
        }
        if inventory.scope().database() != self.database {
            return Err(StorageError::InvalidState);
        }
        let state = self
            .disk_blob_recovery
            .as_ref()
            .ok_or(StorageError::InvalidState)?;
        state.pending.admit(limits)?;
        if inventory.references().len() > limits.maximum_inventory_references
            || input.encoded_group.len() > MAX_PLAINTEXT_BYTES
        {
            return Err(StorageError::ResourceLimit);
        }
        let bytes = inventory
            .references()
            .iter()
            .try_fold(0_u64, |sum, reference| {
                checked_add(sum, reference.byte_len())
            })?;
        if bytes > limits.maximum_verified_blob_bytes {
            return Err(StorageError::ResourceLimit);
        }
        if inventory.is_empty() {
            return self.append_group(filesystem, input);
        }
        let revision = self.frontier.map_or(Ok(CommitRevision::FIRST), |current| {
            current
                .checked_next()
                .map_err(|_| StorageError::RevisionExhausted)
        })?;
        let mut pending = state.pending.clone();
        pending.counts.reference_bindings = checked_add(
            pending.counts.reference_bindings,
            inventory.references().len() as u64,
        )?;
        check_blob_reference_binding_limit(pending.counts.reference_bindings)?;
        let mut added_bytes = 0_u64;
        for reference in inventory.references() {
            if let Some((existing, _)) = self.disk_blob_reference(
                filesystem,
                reference.scope(),
                reference.id(),
                limits.lookup,
                cache,
            )? {
                if existing != *reference {
                    return Err(StorageError::IntegrityFailure);
                }
            } else {
                if pending.references.len() == limits.maximum_pending_blobs {
                    return Err(StorageError::ResourceLimit);
                }
                pending.counts.blobs = checked_add(pending.counts.blobs, 1)?;
                if pending.counts.blobs > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64 {
                    return Err(StorageError::ResourceLimit);
                }
                added_bytes = checked_add(added_bytes, reference.byte_len())?;
                pending
                    .references
                    .insert((reference.scope(), reference.id()), (*reference, revision));
            }
        }
        let scope = inventory.scope();
        let previous_bytes = match pending.namespaces.get(&scope).copied() {
            Some(bytes) => Some(bytes),
            None => match &state.base {
                Some(base) => self.blob_metadata_namespace_bytes(
                    filesystem,
                    base,
                    scope,
                    limits.lookup,
                    cache,
                )?,
                None => None,
            },
        };
        let total_bytes = checked_add(previous_bytes.unwrap_or(0), added_bytes)?;
        check_namespace_blob_byte_limit(total_bytes)?;
        if previous_bytes.is_none() {
            pending.counts.namespaces = checked_add(pending.counts.namespaces, 1)?;
        }
        // Retain only changed totals (including a new zero-byte namespace), not every touched scope.
        if previous_bytes != Some(total_bytes) {
            if !pending.namespaces.contains_key(&scope)
                && pending.namespaces.len() == limits.maximum_pending_namespaces
            {
                return Err(StorageError::ResourceLimit);
            }
            pending.namespaces.insert(scope, total_bytes);
        }
        let digest = inventory.digest();
        let previous_inventory = match pending.inventories.get(&digest).copied() {
            Some(value) => Some(value),
            None => match &state.base {
                Some(base) => {
                    self.blob_metadata_inventory(filesystem, base, digest, limits.lookup, cache)?
                }
                None => None,
            },
        };
        if let Some((count, _)) = previous_inventory {
            if count != inventory.references().len() {
                return Err(StorageError::IntegrityFailure);
            }
        } else {
            if pending.inventories.len() == limits.maximum_pending_inventories {
                return Err(StorageError::ResourceLimit);
            }
            pending.counts.inventories = checked_add(pending.counts.inventories, 1)?;
            pending
                .inventories
                .insert(digest, (inventory.references().len(), revision));
        }
        self.publish_blob_inventory(filesystem, inventory, previous_inventory.is_some())?;
        let durable = self.append_group_internal(filesystem, input, digest)?;
        self.committed_blob_reference_bindings = pending.counts.reference_bindings;
        self.disk_blob_recovery
            .as_mut()
            .expect("validated disk storage mode")
            .pending = pending;
        Ok(durable)
    }

    /// Independently rebuild and publish the exact current catalog, then release pending entries.
    /// On failure the previous base/overlay is retained. No consumer authorization is inferred.
    pub fn refresh_disk_blob_metadata(
        &mut self,
        filesystem: &mut F,
        limits: BlobMetadataRebuildLimits,
        cache: &mut PageCache,
    ) -> Result<BlobMetadataRebuildReport, StorageError> {
        if self.poisoned {
            return Err(StorageError::NeedsRecovery);
        }
        let expected = self
            .disk_blob_recovery
            .as_ref()
            .ok_or(StorageError::InvalidState)?
            .pending
            .counts;
        let (base, report) = self.stage_blob_metadata_rebuild(filesystem, limits, cache)?;
        if base.counts != expected {
            return Err(StorageError::IntegrityFailure);
        }
        let base = self.publish_admitted_blob_metadata(filesystem, base, limits.admission.run)?;
        let state = self
            .disk_blob_recovery
            .as_mut()
            .expect("validated disk storage mode");
        state.base = Some(base);
        state.pending = BlobMetadataPending::new(expected);
        Ok(report)
    }
}
