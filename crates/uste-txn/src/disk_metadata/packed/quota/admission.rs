//! Bounded independent quota-to-primary bijection and aggregate validation.
use super::*;
use uste_storage::{
    journal::{CertificateAnchorReadReport, CertifiedPackedRoot},
    packed_tree_cursor::{TreeCursorLimits, TreeCursorReport},
    packed_tree_validation::{TreeValidationLimits, TreeValidationReport},
};

#[derive(Clone, Copy)]
pub struct PackedQuotaAdmissionLimits {
    pub certificates: CertificateAnchorReadLimits,
    pub family: TreeValidationLimits,
    pub cursor: TreeCursorLimits,
    pub lookup: TreeLookupLimits,
    pub maximum_owners: u64,
    pub maximum_lookup_pages: u64,
    pub maximum_lookup_bytes: u64,
}
pub struct PackedQuotaAdmissionReport {
    pub certificate: CertificateAnchorReadReport,
    pub families: [TreeValidationReport; 3],
    pub cursor: TreeCursorReport,
    pub lookup_pages: u64,
    pub lookup_bytes: u64,
}
struct LookupBudget {
    pages: u64,
    bytes: u64,
}
impl LookupBudget {
    fn limits(&self, mut limits: TreeLookupLimits) -> TreeLookupLimits {
        limits.maximum_pages = limits.maximum_pages.min(self.pages);
        limits.maximum_encoded_bytes = limits.maximum_encoded_bytes.min(self.bytes);
        limits
    }
    fn debit(&mut self, report: TreeLookupReport) -> Result<(), TransactionError> {
        self.pages = self
            .pages
            .checked_sub(report.pages)
            .ok_or(TransactionError::ResourceLimit)?;
        self.bytes = self
            .bytes
            .checked_sub(report.encoded_bytes)
            .ok_or(TransactionError::ResourceLimit)?;
        Ok(())
    }
}

pub fn admit_packed_quota_prefix<F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    fs: &mut F,
    primary: &PackedCoordinatorPrefix,
    root: &CertifiedPackedRoot,
    limits: PackedQuotaAdmissionLimits,
) -> Result<(PackedQuotaPrefix, PackedQuotaAdmissionReport), TransactionError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let manifest = root.manifest();
    let families = manifest.families();
    if primary.scope != recovery.scope()
        || manifest.context().scope != primary.scope
        || manifest.context().profile != COORDINATOR_PACKED_USAGE_PROFILE_V1
        || (
            manifest.claims().revision,
            manifest.claims().certificate_digest,
        ) != primary.anchor
        || families.len() != 3
        || families
            .iter()
            .enumerate()
            .any(|(i, f)| f.family != i as u8 + 1)
        || families[0].commitment.entries() != 1
        || families[2].commitment.entries() != primary.owner_count()
    {
        return Err(TransactionError::IntegrityFailure);
    }
    if primary.owner_count() > limits.maximum_owners
        || limits.maximum_owners > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64
    {
        return Err(TransactionError::ResourceLimit);
    }
    for tree in &primary.trees {
        recovery
            .journal
            .validate_packed_tree_binding(tree)
            .map_err(TransactionError::Storage)?;
    }
    recovery
        .journal
        .validate_packed_root_certificate(root)
        .map_err(TransactionError::Storage)?;
    let proof = recovery
        .journal
        .authenticate_certificate_anchor(
            fs,
            primary.anchor.0,
            primary.anchor.1,
            limits.certificates,
        )
        .map_err(TransactionError::Storage)?;
    let certificate = proof.report();
    let maintenance = PackedIndexMaintenance::new(primary.scope, &mut recovery.journal, proof)?;
    let mut trees = Vec::new();
    trees
        .try_reserve_exact(3)
        .map_err(|_| TransactionError::ResourceLimit)?;
    let mut reports = Vec::new();
    reports
        .try_reserve_exact(3)
        .map_err(|_| TransactionError::ResourceLimit)?;
    for family in 1..=3 {
        let (tree, report) = maintenance.admit(fs, root, family, limits.family)?;
        trees.push(tree);
        reports.push(report);
    }
    let mut budget = LookupBudget {
        pages: limits.maximum_lookup_pages,
        bytes: limits.maximum_lookup_bytes,
    };
    let metadata = maintenance.get(fs, &trees[0], KEY, budget.limits(limits.lookup))?;
    budget.debit(metadata.report)?;
    let metadata = metadata.value.ok_or(TransactionError::IntegrityFailure)?;
    let metadata = metadata.as_slice();
    if metadata.len() != 24 {
        return Err(TransactionError::IntegrityFailure);
    }
    let owners = read_u64(&metadata[..8]).map_err(TransactionError::Storage)?;
    let bytes = read_u64(&metadata[8..16]).map_err(TransactionError::Storage)?;
    let principals = read_u64(&metadata[16..]).map_err(TransactionError::Storage)?;
    if owners != primary.owner_count()
        || principals > owners
        || (owners == 0) != (principals == 0)
        || principals != families[1].commitment.entries()
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let mut cursor = maintenance.cursor(&trees[2], b"", None, limits.cursor)?;
    let mut current = None;
    let mut actual = (0_u64, 0_u64);
    let mut expected = (0_u64, 0_u64);
    let mut seen_principals = 0_u64;
    let mut total_bytes = 0_u64;
    while let Some(entry) = maintenance.next(fs, &mut cursor)? {
        if entry.key().len() != 48 {
            return Err(TransactionError::IntegrityFailure);
        }
        let (reference, principal) = decode_owner(primary.scope, &entry.key()[32..], entry.value())
            .map_err(TransactionError::Storage)?;
        if entry.key()[..32] != principal.as_bytes() {
            return Err(TransactionError::IntegrityFailure);
        }
        let (owner, work) = primary.owner(
            &maintenance,
            fs,
            reference.id(),
            budget.limits(limits.lookup),
        )?;
        budget.debit(work)?;
        if owner != Some((reference, principal)) {
            return Err(TransactionError::IntegrityFailure);
        }
        if current != Some(principal) {
            if current.is_some() && actual != expected {
                return Err(TransactionError::IntegrityFailure);
            }
            let value = maintenance.get(
                fs,
                &trees[1],
                &principal.as_bytes(),
                budget.limits(limits.lookup),
            )?;
            budget.debit(value.report)?;
            expected = decode_pair(
                value
                    .value
                    .ok_or(TransactionError::IntegrityFailure)?
                    .as_slice(),
            )?;
            if expected.0 == 0 {
                return Err(TransactionError::IntegrityFailure);
            }
            actual = (0, 0);
            current = Some(principal);
            seen_principals = add(seen_principals, 1)?;
        }
        actual.0 = add(actual.0, 1)?;
        actual.1 = add(actual.1, reference.byte_len())?;
        total_bytes = add(total_bytes, reference.byte_len())?;
    }
    if actual != expected
        || seen_principals != principals
        || total_bytes != bytes
        || cursor.report().returned_entries != owners
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let report = PackedQuotaAdmissionReport {
        certificate,
        families: reports
            .try_into()
            .map_err(|_| TransactionError::IntegrityFailure)?,
        cursor: cursor.report(),
        lookup_pages: limits.maximum_lookup_pages - budget.pages,
        lookup_bytes: limits.maximum_lookup_bytes - budget.bytes,
    };
    Ok((
        PackedQuotaPrefix {
            scope: primary.scope,
            anchor: primary.anchor,
            primary: primary.families().map(|f| f.commitment),
            trees: trees
                .try_into()
                .map_err(|_| TransactionError::IntegrityFailure)?,
            owners,
            bytes,
            principals,
        },
        report,
    ))
}
