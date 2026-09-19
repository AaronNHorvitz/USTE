//! Cold semantic admission of canonical packed families; no graph-wide maps.
use super::*;
use uste_storage::{
    journal::CertifiedPackedRoot,
    packed_tree_cursor::TreeCursorReport,
    packed_tree_lookup::TreeLookupLimits,
    packed_tree_validation::{TreeValidationLimits, TreeValidationReport},
};
use uste_txn::{PackedIndexMaintenance, ScopedPackedCursor};

#[derive(Clone, Copy)]
pub struct PackedGraphAdmissionLimits {
    /// Aggregate over all eight full canonical validations; path depth remains per traversal.
    pub canonical: TreeValidationLimits,
    pub semantic: GraphDiskBaseAdmissionLimits,
    /// Aggregate across all sequential family scans; separate from interleaved exact proofs.
    pub scan: TreeCursorLimits,
    pub lookup: TreeLookupLimits,
    pub maximum_lookup_encoded_bytes: u64,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackedGraphAdmissionReport {
    pub canonical: [TreeValidationReport; 8],
    /// Logical semantic counters plus actual packed scan/lookup pages, not legacy cache hits.
    pub semantic: GraphDiskBaseAdmissionReport,
    pub scan_encoded_bytes: u64,
    pub scan_candidates: u64,
    pub lookup_encoded_bytes: u64,
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
    trees: &'a [CanonicalPackedTree; 8],
    scope: NamespaceRef,
    revision: CommitRevision,
    limits: PackedGraphAdmissionLimits,
    scan_remaining: TreeCursorLimits,
    report: PackedGraphAdmissionReport,
}
impl<F: OwnershipFileSystem, W: DurableKeyEnvelope, E: EntropySource, I: EntropySource>
    Reader<'_, '_, F, W, E, I>
{
    fn scan(&self, family: u8) -> Result<ScopedPackedCursor, GraphDiskError> {
        Ok(self.maintenance.cursor(
            &self.trees[usize::from(family - 1)],
            b"",
            None,
            self.scan_remaining,
        )?)
    }
    fn finish_scan(
        &mut self,
        cursor: ScopedPackedCursor,
        expected: u64,
    ) -> Result<(), GraphDiskError> {
        let work = cursor.report();
        if work.returned_entries != expected {
            return Err(GraphDiskError::IndexCorrupt);
        }
        self.scan_remaining.maximum_candidates -= work.candidates;
        self.scan_remaining.maximum_returned_bytes -= work.returned_bytes;
        self.scan_remaining.maximum_pages -= work.pages;
        self.scan_remaining.maximum_encoded_bytes -= work.encoded_bytes;
        self.report.scan_encoded_bytes =
            checked_sum(self.report.scan_encoded_bytes, work.encoded_bytes)?;
        self.report.scan_candidates = checked_sum(self.report.scan_candidates, work.candidates)?;
        let scan = &mut self.report.semantic.scan;
        scan.runs += 1;
        scan.entries = checked_sum(scan.entries, expected)?;
        scan.logical_bytes = checked_sum(scan.logical_bytes, work.returned_bytes)?;
        scan.pages_read = checked_sum(scan.pages_read, work.pages)?;
        Ok(())
    }
    fn lookup_limits(&self) -> Result<TreeLookupLimits, GraphDiskError> {
        let mut limits = self.limits.lookup;
        let semantic = self.limits.semantic;
        limits.maximum_pages = limits.maximum_pages.min(
            semantic
                .maximum_lookup_page_visits
                .checked_sub(self.report.semantic.lookup_page_visits)
                .ok_or(GraphDiskError::IndexCorrupt)?,
        );
        limits.maximum_value_bytes = limits.maximum_value_bytes.min(
            semantic
                .maximum_lookup_result_bytes
                .checked_sub(self.report.semantic.lookup_result_bytes)
                .ok_or(GraphDiskError::IndexCorrupt)?,
        );
        limits.maximum_encoded_bytes = limits.maximum_encoded_bytes.min(
            self.limits
                .maximum_lookup_encoded_bytes
                .checked_sub(self.report.lookup_encoded_bytes)
                .ok_or(GraphDiskError::IndexCorrupt)?,
        );
        Ok(limits)
    }
    fn charge_lookup(
        &mut self,
        pages: u64,
        bytes: u64,
        returned: u64,
    ) -> Result<(), GraphDiskError> {
        self.report.semantic.lookup_page_visits =
            checked_sum(self.report.semantic.lookup_page_visits, pages)?;
        self.report.semantic.lookup_result_bytes =
            checked_sum(self.report.semantic.lookup_result_bytes, returned)?;
        self.report.lookup_encoded_bytes = checked_sum(self.report.lookup_encoded_bytes, bytes)?;
        Ok(())
    }
    fn current(&mut self, id: RecordRef) -> Result<Option<Record>, GraphDiskError> {
        charge_lookup_operation(&mut self.report.semantic, self.limits.semantic, true)?;
        let limits = self.lookup_limits()?;
        let result =
            self.maintenance
                .get(self.fs, &self.trees[1], id.record().as_bytes(), limits)?;
        let returned = result
            .value
            .as_ref()
            .map_or(0, |value| value.as_slice().len() as u64);
        self.charge_lookup(result.report.pages, result.report.encoded_bytes, returned)?;
        result
            .value
            .map(|value| {
                let record = decode_stored_record(value.as_slice())?;
                if record.id() != id || record.modified_revision() > self.revision {
                    return Err(GraphDiskError::IndexCorrupt);
                }
                Ok(record)
            })
            .transpose()
    }
    fn requirement(
        &mut self,
        requirement: ReferenceRequirement,
        historical: bool,
    ) -> Result<(), GraphDiskError> {
        charge_semantic_reference_visits(&mut self.report.semantic, self.limits.semantic, 1)?;
        let record = if historical {
            charge_lookup_operation(&mut self.report.semantic, self.limits.semantic, false)?;
            let limits = self.lookup_limits()?;
            let prefix = *requirement.target.record().as_bytes();
            let upper = history_key(requirement.target, requirement.revision);
            let mut cursor = self.maintenance.reverse_cursor(
                &self.trees[2],
                &prefix,
                Some(&upper),
                TreeCursorLimits {
                    maximum_path_branches: limits.maximum_path_branches,
                    maximum_candidates: 2,
                    maximum_returned_bytes: limits
                        .maximum_value_bytes
                        .min(self.limits.semantic.predecessor.maximum_result_bytes() as u64),
                    maximum_pages: limits
                        .maximum_pages
                        .min(self.limits.semantic.predecessor.maximum_page_visits()),
                    maximum_encoded_bytes: limits.maximum_encoded_bytes,
                },
            )?;
            let entry = self.maintenance.next(self.fs, &mut cursor)?;
            let TreeCursorReport {
                pages,
                encoded_bytes,
                returned_bytes,
                ..
            } = cursor.report();
            self.charge_lookup(pages, encoded_bytes, returned_bytes)?;
            entry
                .map(|entry| {
                    if entry.key().len() != 24 || entry.key()[..16] != prefix {
                        return Err(GraphDiskError::IndexCorrupt);
                    }
                    let revision = CommitRevision::new(
                        read_u64_be(&entry.key()[16..]).map_err(GraphDiskError::Storage)?,
                    )
                    .map_err(|_| GraphDiskError::IndexCorrupt)?;
                    let record = decode_stored_record(entry.value())?;
                    if record.id() != requirement.target
                        || record.modified_revision() != revision
                        || revision > requirement.revision
                        || revision > self.revision
                    {
                        return Err(GraphDiskError::IndexCorrupt);
                    }
                    Ok(record)
                })
                .transpose()?
        } else {
            self.current(requirement.target)?
        };
        if !reference_requirement_matches(requirement, record.as_ref()) {
            return Err(GraphDiskError::IndexCorrupt);
        }
        Ok(())
    }
    fn finish_group(&mut self, group: HistoryAdmissionGroup) -> Result<(), GraphDiskError> {
        let current = self
            .current(group.id)?
            .ok_or(GraphDiskError::IndexCorrupt)?;
        if group.previous.as_ref() != Some(&current) {
            return Err(GraphDiskError::IndexCorrupt);
        }
        visit_current_reference_requirements(&current, self.revision, &mut |r| {
            self.requirement(r, false)
        })
        .map_err(graph_requirement_error)
    }
}

pub fn admit_packed_graph_base<F, W, E, I>(
    maintenance: &PackedIndexMaintenance<'_, F, W, E, I>,
    fs: &mut F,
    root: &CertifiedPackedRoot,
    limits: PackedGraphAdmissionLimits,
) -> Result<(PackedGraphBase, PackedGraphAdmissionReport), GraphDiskError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let manifest = root.manifest();
    let context = manifest.context();
    let claims = manifest.claims();
    if context.profile != GRAPH_PACKED_PROFILE_V1
        || claims.reducer_profile != GraphState::REDUCER_PROFILE
        || claims.state_commitment_profile != GRAPH_ORDERED_STATE_PROFILE_V1
        || maintenance.anchor() != (claims.revision, claims.certificate_digest)
        || manifest.families().len() != 8
        || !manifest.families().iter().map(|f| f.family).eq(1..=8)
    {
        return Err(GraphDiskError::RootStateMismatch);
    }
    maintenance.validate_root_binding(root)?;
    let commitments = core::array::from_fn(|i| manifest.families()[i].commitment);
    if ordered_state_digest(
        context.scope,
        claims.revision,
        claims.reducer_profile,
        commitments,
    ) != claims.state_digest
    {
        return Err(GraphDiskError::RootStateMismatch);
    }
    let entries = commitments
        .iter()
        .try_fold(0, |sum, c| checked_sum(sum, c.entries()))?;
    let logical_bytes = commitments
        .iter()
        .try_fold(0, |sum, c| checked_sum(sum, c.logical_bytes()))?;
    if entries > limits.semantic.scan.maximum_total_entries
        || logical_bytes > limits.semantic.scan.maximum_logical_bytes
    {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    let mut report = PackedGraphAdmissionReport::default();
    let mut remaining = limits.canonical;
    let mut trees = Vec::with_capacity(8);
    for family in 1..=8 {
        let (tree, work) = maintenance.admit(fs, root, family, remaining)?;
        remaining.maximum_nodes -= work.nodes;
        remaining.maximum_logical_bytes -= work.logical_bytes;
        remaining.maximum_pages -= work.pages;
        remaining.maximum_encoded_bytes -= work.encoded_bytes;
        report.canonical[usize::from(family - 1)] = work;
        trees.push(tree);
    }
    let trees: [CanonicalPackedTree; 8] =
        trees.try_into().map_err(|_| GraphDiskError::IndexCorrupt)?;
    let mut scan_remaining = limits.scan;
    scan_remaining.maximum_pages = scan_remaining
        .maximum_pages
        .min(limits.semantic.scan.maximum_total_pages);
    scan_remaining.maximum_returned_bytes = scan_remaining
        .maximum_returned_bytes
        .min(limits.semantic.scan.maximum_logical_bytes);
    let mut reader = Reader {
        maintenance,
        fs,
        trees: &trees,
        scope: context.scope,
        revision: claims.revision,
        limits,
        scan_remaining,
        report,
    };
    let mut cursor = reader.scan(FAMILY_METADATA)?;
    let metadata = maintenance
        .next(reader.fs, &mut cursor)?
        .ok_or(GraphDiskError::IndexCorrupt)?;
    if metadata.key() != b"graph-state-v1" || maintenance.next(reader.fs, &mut cursor)?.is_some() {
        return Err(GraphDiskError::IndexCorrupt);
    }
    reader.finish_scan(cursor, 1)?;
    let parsed = parse_metadata(metadata.value(), claims.revision)?;
    let counts = parsed.state_counts();
    let family_counts = parsed.family_counts()?;
    if commitments.iter().map(|c| c.entries()).ne(family_counts) || parsed.history < parsed.current
    {
        return Err(GraphDiskError::IndexCorrupt);
    }
    if parsed.current > limits.semantic.scan.maximum_records
        || parsed.history > limits.semantic.scan.maximum_versions
        || parsed.policy_history > limits.semantic.scan.maximum_policy_history
    {
        return Err(GraphDiskError::Storage(StorageError::ResourceLimit));
    }
    let mut validator = MergedGraphStateValidator::new(
        context.scope,
        claims.revision,
        counts,
        limits.semantic.maximum_history_group_logical_bytes,
    )?;
    validator
        .observe(FAMILY_METADATA, metadata.key(), metadata.value())
        .map_err(GraphDiskError::Storage)?;
    validator.finish_family(FAMILY_METADATA)?;
    let mut derived = [0_u64; 4];
    let mut group: Option<HistoryAdmissionGroup> = None;
    let mut current_policy = None;
    let mut terminal_policy = None;
    let mut prior_policy_revision = None;
    let mut prior_policy_version = None;
    for family in FAMILY_CURRENT_RECORD..=FAMILY_POLICY {
        let mut cursor = reader.scan(family)?;
        while let Some(entry) = maintenance.next(reader.fs, &mut cursor)? {
            validator
                .observe(family, entry.key(), entry.value())
                .map_err(GraphDiskError::Storage)?;
            match family {
                FAMILY_CURRENT_RECORD => {
                    let record = decode_stored_record(entry.value())?;
                    for (i, count) in record_secondary_counts(
                        &record,
                        limits.semantic,
                        &mut reader.report.semantic,
                    )?
                    .into_iter()
                    .enumerate()
                    {
                        derived[i] = checked_sum(derived[i], count)?;
                    }
                }
                FAMILY_RECORD_HISTORY => {
                    let record = decode_stored_record(entry.value())?;
                    let id = record.id();
                    if group.as_ref().is_some_and(|g| g.id != id) {
                        reader.finish_group(group.take().ok_or(GraphDiskError::IndexCorrupt)?)?;
                    }
                    let group = group.get_or_insert_with(|| HistoryAdmissionGroup::new(id));
                    group.observe(
                        context.scope,
                        record,
                        entry.value().len() as u64,
                        limits.semantic,
                    )?;
                    let record = group
                        .previous
                        .as_ref()
                        .ok_or(GraphDiskError::IndexCorrupt)?;
                    if group.versions == 1 {
                        visit_history_first_reference_requirements(record, &mut |r| {
                            reader.requirement(r, true)
                        })
                    } else {
                        visit_history_successor_reference_requirements(record, &mut |r| {
                            reader.requirement(r, true)
                        })
                    }
                    .map_err(graph_requirement_error)?;
                    reader.report.semantic.peak_history_group_logical_bytes = reader
                        .report
                        .semantic
                        .peak_history_group_logical_bytes
                        .max(group.logical_bytes);
                }
                FAMILY_OUTGOING..=FAMILY_REVERSE => {
                    let owner = record_key(reader.scope, &entry.key()[16..])
                        .map_err(GraphDiskError::Storage)?;
                    let record = reader.current(owner)?.ok_or(GraphDiskError::IndexCorrupt)?;
                    let entry = IndexEntry {
                        key: entry.key().to_vec(),
                        value: entry.value().to_vec(),
                    };
                    if !record_contributes_secondary_entry(
                        reader.scope,
                        &record,
                        family,
                        &entry,
                        limits.semantic,
                        &mut reader.report.semantic,
                    )? {
                        return Err(GraphDiskError::IndexCorrupt);
                    }
                }
                FAMILY_POLICY => {
                    let policy =
                        decode_result_policy(entry.value())?.ok_or(GraphDiskError::IndexCorrupt)?;
                    if policy.scope() != context.scope {
                        return Err(GraphDiskError::IndexCorrupt);
                    }
                    match entry.key() {
                        [0] if current_policy.is_none() => current_policy = Some(policy),
                        [1, bytes @ ..] if bytes.len() == 8 && current_policy.is_some() => {
                            let revision = CommitRevision::new(
                                read_u64_be(bytes).map_err(GraphDiskError::Storage)?,
                            )
                            .map_err(|_| GraphDiskError::IndexCorrupt)?;
                            if revision > claims.revision
                                || prior_policy_revision.is_some_and(|p| p >= revision)
                                || prior_policy_version.is_some_and(|p| p >= policy.version())
                            {
                                return Err(GraphDiskError::IndexCorrupt);
                            }
                            prior_policy_revision = Some(revision);
                            prior_policy_version = Some(policy.version());
                            terminal_policy = Some(policy);
                        }
                        _ => return Err(GraphDiskError::IndexCorrupt),
                    }
                }
                _ => return Err(GraphDiskError::IndexCorrupt),
            }
        }
        reader.finish_scan(cursor, family_counts[usize::from(family - 1)])?;
        if family == FAMILY_CURRENT_RECORD
            && derived != [counts[2], counts[3], counts[4], counts[5]]
        {
            return Err(GraphDiskError::IndexCorrupt);
        }
        if family == FAMILY_RECORD_HISTORY
            && let Some(group) = group.take()
        {
            reader.finish_group(group)?;
        }
        validator.finish_family(family)?;
    }
    if (parsed.policy_history == 0 && current_policy.is_some()) || terminal_policy != current_policy
    {
        return Err(GraphDiskError::IndexCorrupt);
    }
    let digest = validator.finish()?;
    let report = reader.report;
    Ok((
        PackedGraphBase {
            scope: context.scope,
            anchor: (claims.revision, claims.certificate_digest),
            reducer: claims.reducer_profile,
            trees,
            counts,
            policy: current_policy,
            source_v1_digest: Some(digest),
        },
        report,
    ))
}
