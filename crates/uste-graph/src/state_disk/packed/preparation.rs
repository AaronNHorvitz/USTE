//! Explicit-I/O proof closure, with the existing pure graph reducer as semantic authority.
use super::*;
mod staging;
pub use staging::{
    GraphPackedLivePublication, GraphPackedLiveSnapshot, GraphPackedLiveState, PackedGraphDelta,
    PackedGraphExpansionLimits, PackedGraphOriginRecoveryLimits, PackedGraphOriginRecoveryReport,
    PackedGraphReadLimits, PackedGraphStageLimits, PackedGraphStageReport,
    PackedGraphSuffixRecoveryLimits, PackedGraphSuffixRecoveryReport,
    PackedGraphWritePreparationLimits, PackedGraphWritePublicationLimits,
    prepare_packed_graph_delta, publish_packed_graph_live_base, recover_packed_graph_origin,
    recover_packed_graph_suffix, stage_packed_graph_delta,
};
use uste_storage::packed_page_cache::{PackedCacheReport, PackedPageCache};
use uste_storage::packed_tree_lookup::{PackedLookupValue, TreeLookupLimits, TreeLookupReport};
use uste_txn::PackedIndexMaintenance;

#[derive(Clone, Copy)]
pub struct PackedGraphPreparationLimits {
    pub proof: GraphDiskPreparationLimits,
    pub lookup: TreeLookupLimits,
    pub maximum_point_lookups: u64,
    pub maximum_pages: u64,
    pub maximum_encoded_bytes: u64,
    pub maximum_scan_candidates: u64,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackedGraphReadReport {
    pub point_lookups: u64,
    pub pages: u64,
    pub encoded_bytes: u64,
    pub scan_candidates: u64,
}
pub struct PackedPreparedGraph {
    prepared: PreparedGraph,
    base_anchor: (CommitRevision, [u8; 32]),
    base_digest: [u8; 32],
    base_counts: [u64; 8],
    base_policy: Option<NamespacePolicy>,
}
impl PackedPreparedGraph {
    pub fn result_digest(&self) -> [u8; 32] {
        self.prepared.result_digest
    }
    pub fn revision(&self) -> CommitRevision {
        self.prepared.revision
    }
    pub fn change_count(&self) -> usize {
        self.prepared.change_count()
    }
    pub fn base_anchor(&self) -> (CommitRevision, [u8; 32]) {
        self.base_anchor
    }
    pub fn base_commitment(&self) -> &[u8; 32] {
        &self.base_digest
    }
    pub fn base_counts(&self) -> [u64; 8] {
        self.base_counts
    }
    pub fn base_policy(&self) -> Option<&NamespacePolicy> {
        self.base_policy.as_ref()
    }
}

struct Reader<
    'a,
    'j,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
> {
    maintenance: &'a PackedIndexMaintenance<'j, F, W, E, I>,
    fs: &'a mut F,
    base: &'a PackedGraphBase,
    limits: PackedGraphPreparationLimits,
    report: PackedGraphReadReport,
    cache: Option<&'a mut PackedPageCache>,
}
impl<F: OwnershipFileSystem, W: DurableKeyEnvelope, E: EntropySource, I: EntropySource>
    Reader<'_, '_, F, W, E, I>
{
    fn get(
        &mut self,
        family: u8,
        key: &[u8],
        maximum_value: u64,
    ) -> Result<Option<PackedLookupValue>, GraphDiskError> {
        if self.report.point_lookups >= self.limits.maximum_point_lookups {
            return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
        }
        self.report.point_lookups += 1;
        let mut limits = self.limits.lookup;
        limits.maximum_pages = limits
            .maximum_pages
            .min(self.limits.maximum_pages - self.report.pages);
        limits.maximum_encoded_bytes = limits
            .maximum_encoded_bytes
            .min(self.limits.maximum_encoded_bytes - self.report.encoded_bytes);
        limits.maximum_value_bytes = limits.maximum_value_bytes.min(maximum_value);
        let tree = &self.base.trees[usize::from(family - 1)];
        let result = match self.cache.as_deref_mut() {
            Some(cache) => self
                .maintenance
                .as_reader()
                .get_cached(self.fs, tree, key, limits, cache)?,
            None => self.maintenance.get(self.fs, tree, key, limits)?,
        };
        self.charge(result.report);
        Ok(result.value)
    }
    fn charge(&mut self, work: TreeLookupReport) {
        self.report.pages += work.pages;
        self.report.encoded_bytes += work.encoded_bytes;
    }
    fn scan(
        &mut self,
        family: u8,
        prefix: &[u8],
        maximum_bytes: u64,
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<(), GraphDiskError>,
    ) -> Result<(), GraphDiskError> {
        let mut upper = prefix.to_vec();
        while upper.last() == Some(&255) {
            upper.pop();
        }
        let upper = if let Some(last) = upper.last_mut() {
            *last += 1;
            Some(upper)
        } else {
            None
        };
        let limits = TreeCursorLimits {
            maximum_path_branches: self.limits.lookup.maximum_path_branches,
            maximum_candidates: self.limits.maximum_scan_candidates - self.report.scan_candidates,
            maximum_returned_bytes: maximum_bytes,
            maximum_pages: self.limits.maximum_pages - self.report.pages,
            maximum_encoded_bytes: self.limits.maximum_encoded_bytes - self.report.encoded_bytes,
        };
        let mut cursor = self.maintenance.cursor(
            &self.base.trees[usize::from(family - 1)],
            prefix,
            upper.as_deref(),
            limits,
        )?;
        loop {
            let entry = match self.cache.as_deref_mut() {
                Some(cache) => {
                    self.maintenance
                        .as_reader()
                        .next_cached(self.fs, &mut cursor, cache)?
                }
                None => self.maintenance.next(self.fs, &mut cursor)?,
            };
            let Some(entry) = entry else {
                break;
            };
            if !entry.key().starts_with(prefix) {
                return Err(GraphDiskError::IndexCorrupt);
            }
            visit(entry.key(), entry.value())?;
        }
        let work = cursor.report();
        self.report.pages += work.pages;
        self.report.encoded_bytes += work.encoded_bytes;
        self.report.scan_candidates += work.candidates;
        Ok(())
    }
}

/// Trusted explicit-I/O preparation, not consumer authority or permission to commit.
/// The middle report covers logical proof retention only: its legacy run/cache counters stay zero.
/// The final report supplies successful packed primitive work; failed/adapter I/O is not included.
pub fn prepare_packed_graph_transaction<F, W, E, I>(
    maintenance: &PackedIndexMaintenance<'_, F, W, E, I>,
    fs: &mut F,
    base: &PackedGraphBase,
    transaction: GraphTransaction,
    limits: PackedGraphPreparationLimits,
) -> Result<
    (
        PackedPreparedGraph,
        GraphDiskPreparationReport,
        PackedGraphReadReport,
    ),
    GraphDiskError,
>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    prepare_with_cache(maintenance, fs, base, transaction, limits, None)
}

/// Trusted fresh per-preparation cache. All logical proof limits and reducer checks are unchanged.
/// The cache is dropped before return; callers cannot supply warmth across requests or mutations.
/// Invalid cache sizes fail before I/O. Cache accounting is not process RSS or physical I/O.
pub fn prepare_packed_graph_transaction_buffered<F, W, E, I>(
    maintenance: &PackedIndexMaintenance<'_, F, W, E, I>,
    fs: &mut F,
    base: &PackedGraphBase,
    transaction: GraphTransaction,
    limits: PackedGraphPreparationLimits,
    cache_bytes: usize,
) -> Result<
    (
        PackedPreparedGraph,
        GraphDiskPreparationReport,
        PackedGraphReadReport,
        PackedCacheReport,
    ),
    GraphDiskError,
>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let mut cache = PackedPageCache::new(cache_bytes)?;
    let (prepared, proof, work) =
        prepare_with_cache(maintenance, fs, base, transaction, limits, Some(&mut cache))?;
    Ok((prepared, proof, work, cache.report()?))
}

fn prepare_with_cache<F, W, E, I>(
    maintenance: &PackedIndexMaintenance<'_, F, W, E, I>,
    fs: &mut F,
    base: &PackedGraphBase,
    transaction: GraphTransaction,
    limits: PackedGraphPreparationLimits,
    cache: Option<&mut PackedPageCache>,
) -> Result<
    (
        PackedPreparedGraph,
        GraphDiskPreparationReport,
        PackedGraphReadReport,
    ),
    GraphDiskError,
>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    validate_disk_preparation_subset(&transaction)?;
    validate_request_limits(&transaction)?;
    if transaction.scope() != base.scope {
        return Err(GraphDiskError::Graph(
            crate::GraphError::TransactionScopeMismatch,
        ));
    }
    for tree in &base.trees {
        maintenance.validate_tree_binding(tree)?;
    }
    let proof_limits = limits.proof;
    let mut report = GraphDiskPreparationReport::default();
    let mut reader = Reader {
        maintenance,
        fs,
        base,
        limits,
        report: PackedGraphReadReport::default(),
        cache,
    };
    let mut pending = transaction_required_ids(&transaction, &mut report, &proof_limits)?;
    charge_logical_bytes(&mut report, &proof_limits, b"graph-state-v1".len() + 80 + 1)?;
    if let Some(policy) = &base.policy {
        charge_logical_bytes(
            &mut report,
            &proof_limits,
            encode_result_policy(Some(policy))?.len(),
        )?;
    }
    let mut current = BTreeMap::new();
    while let Some(id) = pending.pop_first() {
        if current.contains_key(&id) {
            continue;
        }
        report.record_proofs += 1;
        let encoded = reader.get(
            FAMILY_CURRENT_RECORD,
            id.record().as_bytes(),
            remaining_scan_bytes(&report, &proof_limits)? as u64,
        )?;
        let proof = match encoded {
            None => {
                report.absent_records += 1;
                CurrentRecordProof::Absent
            }
            Some(encoded) => {
                charge_logical_bytes(&mut report, &proof_limits, encoded.as_slice().len())?;
                let record = decode_stored_record(encoded.as_slice())?;
                if record.id() != id || record.modified_revision() > base.anchor.0 {
                    return Err(GraphDiskError::IndexCorrupt);
                }
                report.present_records += 1;
                CurrentRecordProof::Present(Box::new(record))
            }
        };
        current.insert(id, proof);
        if needs_retained_references(&transaction, id)
            && let Some(CurrentRecordProof::Present(record)) = current.get(&id)
        {
            try_visit_record_references(record, &mut |reference, _| {
                charge_reference(&mut report, &proof_limits)?;
                add_pending(
                    reference,
                    &current,
                    &mut pending,
                    &mut report,
                    &proof_limits,
                )
            })?;
        }
    }
    let mut history = BTreeMap::new();
    for target in history_proof_targets(&transaction) {
        let mut versions = Vec::new();
        reader.scan(
            FAMILY_RECORD_HISTORY,
            target.record().as_bytes(),
            remaining_scan_bytes(&report, &proof_limits)? as u64,
            |key, value| {
                if report.history_versions >= proof_limits.maximum_history_versions {
                    return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
                }
                charge_logical_bytes(&mut report, &proof_limits, key.len() + value.len())?;
                if key.len() != 24 {
                    return Err(GraphDiskError::IndexCorrupt);
                }
                let revision = CommitRevision::new(read_u64_be(&key[16..])?)
                    .map_err(|_| GraphDiskError::IndexCorrupt)?;
                let record = decode_stored_record(value)?;
                if record.id() != target
                    || record.modified_revision() != revision
                    || revision > base.anchor.0
                    || versions
                        .last()
                        .is_some_and(|prior: &Record| prior.modified_revision() >= revision)
                {
                    return Err(GraphDiskError::IndexCorrupt);
                }
                report.history_versions += 1;
                versions.push(record);
                Ok(())
            },
        )?;
        history.insert(target, versions);
    }
    let mut reverse = BTreeMap::new();
    for target in reverse_proof_targets(&transaction) {
        let mut owners = BTreeMap::new();
        reader.scan(
            FAMILY_REVERSE,
            target.record().as_bytes(),
            remaining_scan_bytes(&report, &proof_limits)? as u64,
            |key, value| {
                if report.reverse_references >= proof_limits.maximum_reverse_references {
                    return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
                }
                charge_logical_bytes(&mut report, &proof_limits, key.len() + value.len())?;
                if key.len() != 32 || value.len() != 24 || value[20..] != [0; 4] {
                    return Err(GraphDiskError::IndexCorrupt);
                }
                let owner = record_key(base.scope, &key[16..])?;
                let owner_revision = CommitRevision::new(read_u64_be(&value[12..20])?)
                    .map_err(|_| GraphDiskError::IndexCorrupt)?;
                let owner_version = crate::RecordVersion::new(read_u64_be(&value[4..12])?)
                    .map_err(|_| GraphDiskError::IndexCorrupt)?;
                if owner_revision > base.anchor.0 {
                    return Err(GraphDiskError::IndexCorrupt);
                }
                let reference = ReverseReference {
                    owner_kind: value[0],
                    owner_state: value[1],
                    roles: u16::from_be_bytes([value[2], value[3]]),
                    owner_version,
                    owner_revision,
                };
                if owners.insert(owner, reference).is_some() {
                    return Err(GraphDiskError::IndexCorrupt);
                }
                report.reverse_references += 1;
                Ok(())
            },
        )?;
        reverse.insert(target, owners);
    }
    let records = current
        .into_iter()
        .filter_map(|(id, proof)| match proof {
            CurrentRecordProof::Absent => None,
            CurrentRecordProof::Present(record) => Some((id, *record)),
        })
        .collect();
    let (prepared, base_policy) = prepare_from_complete_disk_proofs(
        base.scope,
        base.anchor.0,
        records,
        history,
        reverse,
        base.policy.clone(),
        &transaction,
    )?;
    Ok((
        PackedPreparedGraph {
            prepared,
            base_anchor: base.anchor,
            base_digest: base.publication_claims().state_digest,
            base_counts: base.counts,
            base_policy,
        },
        report,
        reader.report,
    ))
}
