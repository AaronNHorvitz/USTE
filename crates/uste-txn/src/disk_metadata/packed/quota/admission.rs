//! Bounded independent quota-to-primary bijection and aggregate validation.
use super::*;
use uste_storage::{
    journal::{CertificateAnchorReadReport, CertifiedPackedRoot},
    packed_page_cache::{PackedCacheReport, PackedPageCache},
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
/// Logical proof work, including buffered hits; not physical I/O.
pub struct PackedQuotaAdmissionReport {
    pub certificate: CertificateAnchorReadReport,
    pub families: [TreeValidationReport; 3],
    pub cursor: TreeCursorReport,
    pub lookup_pages: u64,
    pub lookup_bytes: u64,
}
/// Sequential canonical caches followed by a fresh correspondence cache.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackedQuotaAdmissionCacheReport {
    pub canonical: [PackedCacheReport; 3],
    pub correspondence: PackedCacheReport,
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
    admit_inner(recovery, fs, primary, root, limits, None)
        .map(|(prefix, report, _)| (prefix, report))
}

/// Independently admit quota correspondence using fresh bounded caches, never caller-warmed pages.
pub fn admit_packed_quota_prefix_buffered<F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    fs: &mut F,
    primary: &PackedCoordinatorPrefix,
    root: &CertifiedPackedRoot,
    limits: PackedQuotaAdmissionLimits,
    cache_bytes: usize,
) -> Result<
    (
        PackedQuotaPrefix,
        PackedQuotaAdmissionReport,
        PackedQuotaAdmissionCacheReport,
    ),
    TransactionError,
>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    admit_inner(recovery, fs, primary, root, limits, Some(cache_bytes))
}

fn admit_inner<F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    fs: &mut F,
    primary: &PackedCoordinatorPrefix,
    root: &CertifiedPackedRoot,
    limits: PackedQuotaAdmissionLimits,
    cache_bytes: Option<usize>,
) -> Result<
    (
        PackedQuotaPrefix,
        PackedQuotaAdmissionReport,
        PackedQuotaAdmissionCacheReport,
    ),
    TransactionError,
>
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
    if let Some(bytes) = cache_bytes {
        PackedPageCache::new(bytes).map_err(TransactionError::Storage)?;
    }
    let mut cache_report = PackedQuotaAdmissionCacheReport::default();
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
        let (tree, report) = if let Some(bytes) = cache_bytes {
            let (tree, report, cache) =
                maintenance.admit_buffered(fs, root, family, limits.family, bytes)?;
            cache_report.canonical[usize::from(family - 1)] = cache;
            (tree, report)
        } else {
            maintenance.admit(fs, root, family, limits.family)?
        };
        trees.push(tree);
        reports.push(report);
    }
    let mut budget = LookupBudget {
        pages: limits.maximum_lookup_pages,
        bytes: limits.maximum_lookup_bytes,
    };
    let mut cache = cache_bytes
        .map(PackedPageCache::new)
        .transpose()
        .map_err(TransactionError::Storage)?;
    let metadata = metadata_get(
        &maintenance,
        fs,
        &trees[0],
        KEY,
        budget.limits(limits.lookup),
        cache.as_mut(),
    )?;
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
    loop {
        let next = match cache.as_mut() {
            Some(cache) => maintenance
                .as_reader()
                .next_cached(fs, &mut cursor, cache)?,
            None => maintenance.next(fs, &mut cursor)?,
        };
        let Some(entry) = next else {
            break;
        };
        if entry.key().len() != 48 {
            return Err(TransactionError::IntegrityFailure);
        }
        let (reference, principal) = decode_owner(primary.scope, &entry.key()[32..], entry.value())
            .map_err(TransactionError::Storage)?;
        if entry.key()[..32] != principal.as_bytes() {
            return Err(TransactionError::IntegrityFailure);
        }
        let (owner, work) = primary.owner_with_cache(
            &maintenance,
            fs,
            reference.id(),
            budget.limits(limits.lookup),
            cache.as_mut(),
        )?;
        budget.debit(work)?;
        if owner != Some((reference, principal)) {
            return Err(TransactionError::IntegrityFailure);
        }
        if current != Some(principal) {
            if current.is_some() && actual != expected {
                return Err(TransactionError::IntegrityFailure);
            }
            let value = metadata_get(
                &maintenance,
                fs,
                &trees[1],
                &principal.as_bytes(),
                budget.limits(limits.lookup),
                cache.as_mut(),
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
    if let Some(cache) = cache {
        cache_report.correspondence = cache.report().map_err(TransactionError::Storage)?;
    }
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
        cache_report,
    ))
}
