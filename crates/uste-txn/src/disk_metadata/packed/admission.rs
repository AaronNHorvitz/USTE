//! Read-only correspondence admission; all provisional state stays private until exhaustion.
use super::*;
use uste_storage::journal::{
    CertificateAnchorReadReport, CertifiedPackedRoot, JournalRangeReadReport,
};
use uste_storage::packed_page_cache::{PackedCacheReport, PackedPageCache};
use uste_storage::packed_tree_validation::{TreeValidationLimits, TreeValidationReport};

#[derive(Clone, Copy)]
pub struct PackedCoordinatorAdmissionLimits {
    pub certificates: CertificateAnchorReadLimits,
    pub family: TreeValidationLimits,
    pub lookup: TreeLookupLimits,
    pub maximum_groups: u64,
    pub maximum_journal_bytes: u64,
    pub maximum_references: usize,
    pub maximum_owners: u64,
    pub maximum_lookup_pages: u64,
    pub maximum_lookup_bytes: u64,
}
/// Logical proof work, including cache hits when buffered admission is selected.
pub struct PackedCoordinatorAdmissionReport {
    pub certificate: CertificateAnchorReadReport,
    pub families: [TreeValidationReport; 4],
    pub journal: JournalRangeReadReport,
    pub lookup_pages: u64,
    pub lookup_bytes: u64,
    pub first_owners: u64,
}
/// Sequential caches are dropped after each phase; these are not simultaneous residencies.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackedCoordinatorAdmissionCacheReport {
    pub canonical: [PackedCacheReport; 4],
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

pub fn admit_packed_coordinator_prefix<F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    fs: &mut F,
    root: &CertifiedPackedRoot,
    limits: PackedCoordinatorAdmissionLimits,
) -> Result<(PackedCoordinatorPrefix, PackedCoordinatorAdmissionReport), TransactionError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    admit_inner(recovery, fs, root, limits, None).map(|(prefix, report, _)| (prefix, report))
}

/// Full correspondence admission using fresh bounded operation-local caches only.
pub fn admit_packed_coordinator_prefix_buffered<F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    fs: &mut F,
    root: &CertifiedPackedRoot,
    limits: PackedCoordinatorAdmissionLimits,
    cache_bytes: usize,
) -> Result<
    (
        PackedCoordinatorPrefix,
        PackedCoordinatorAdmissionReport,
        PackedCoordinatorAdmissionCacheReport,
    ),
    TransactionError,
>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    admit_inner(recovery, fs, root, limits, Some(cache_bytes))
}

fn admit_inner<F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    fs: &mut F,
    root: &CertifiedPackedRoot,
    limits: PackedCoordinatorAdmissionLimits,
    cache_bytes: Option<usize>,
) -> Result<
    (
        PackedCoordinatorPrefix,
        PackedCoordinatorAdmissionReport,
        PackedCoordinatorAdmissionCacheReport,
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
    let revision = manifest.claims().revision;
    let families = manifest.families();
    if manifest.context().scope != recovery.scope()
        || manifest.context().profile != COORDINATOR_PACKED_PROFILE_V1
        || families.len() != 4
        || families
            .iter()
            .enumerate()
            .any(|(index, f)| f.family != index as u8 + 1)
        || families[0].commitment.entries() != revision.get()
        || families[1].commitment.entries() != revision.get()
        || families[2].commitment.entries() != families[3].commitment.entries()
    {
        return Err(TransactionError::IntegrityFailure);
    }
    if revision.get() > limits.maximum_groups
        || revision.get() > MAX_OUTCOMES_PER_NAMESPACE as u64
        || limits.maximum_references > MAX_BATCH_DELTAS
        || limits.maximum_owners > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64
        || families[2].commitment.entries() > limits.maximum_owners
    {
        return Err(TransactionError::ResourceLimit);
    }
    // Validate the budget before any I/O. No caller-warmed cache can hide late corruption.
    if let Some(bytes) = cache_bytes {
        PackedPageCache::new(bytes).map_err(TransactionError::Storage)?;
    }
    let mut cache_report = PackedCoordinatorAdmissionCacheReport::default();
    recovery
        .journal
        .validate_packed_root_certificate(root)
        .map_err(TransactionError::Storage)?;
    let proof = recovery
        .journal
        .authenticate_certificate_anchor(
            fs,
            revision,
            manifest.claims().certificate_digest,
            limits.certificates,
        )
        .map_err(TransactionError::Storage)?;
    let certificate = proof.report();
    let mut trees = Vec::new();
    trees
        .try_reserve_exact(4)
        .map_err(|_| TransactionError::ResourceLimit)?;
    let mut reports = Vec::new();
    reports
        .try_reserve_exact(4)
        .map_err(|_| TransactionError::ResourceLimit)?;
    {
        let maintenance =
            PackedIndexMaintenance::new(recovery.scope(), &mut recovery.journal, proof.clone())?;
        for family in 1..=4 {
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
    }
    let prefix = PackedCoordinatorPrefix {
        scope: recovery.scope(),
        anchor: proof.anchor(),
        trees: trees
            .try_into()
            .map_err(|_| TransactionError::IntegrityFailure)?,
    };
    let mut budget = LookupBudget {
        pages: limits.maximum_lookup_pages,
        bytes: limits.maximum_lookup_bytes,
    };
    let mut first_owners = 0_u64;
    let mut cache = cache_bytes
        .map(PackedPageCache::new)
        .transpose()
        .map_err(TransactionError::Storage)?;
    let mut cursor = recovery.open_transaction_cursor_with_certificate_window(
        CommitRevision::FIRST,
        revision,
        limits.maximum_groups,
        limits.maximum_journal_bytes,
        64,
    )?;
    while let Some(transaction) = recovery.next_recovered_transaction(fs, &mut cursor)? {
        let references = transaction
            .blob_inventory
            .as_ref()
            .map_or(&[][..], |i| i.references());
        if references.len() > limits.maximum_references {
            return Err(TransactionError::ResourceLimit);
        }
        let maintenance =
            PackedIndexMaintenance::new(recovery.scope(), &mut recovery.journal, proof.clone())?;
        let (retry, work) = prefix.retry_with_cache(
            &maintenance,
            fs,
            transaction.principal,
            transaction.idempotency_key,
            budget.limits(limits.lookup),
            cache.as_mut(),
        )?;
        budget.debit(work)?;
        if retry != Some(transaction.outcome) {
            return Err(TransactionError::IntegrityFailure);
        }
        let (actual, work) = prefix.transaction_with_cache(
            &maintenance,
            fs,
            transaction.outcome.transaction_id,
            budget.limits(limits.lookup),
            cache.as_mut(),
        )?;
        budget.debit(work)?;
        if actual != Some((transaction.principal, transaction.outcome)) {
            return Err(TransactionError::IntegrityFailure);
        }
        for reference in references {
            let (owner, work) = prefix.owner_with_cache(
                &maintenance,
                fs,
                reference.id(),
                budget.limits(limits.lookup),
                cache.as_mut(),
            )?;
            budget.debit(work)?;
            let (witness, work) = prefix.first_revision_with_cache(
                &maintenance,
                fs,
                reference.id(),
                budget.limits(limits.lookup),
                cache.as_mut(),
            )?;
            budget.debit(work)?;
            let (actual, principal) = owner.ok_or(TransactionError::IntegrityFailure)?;
            let witness = witness.ok_or(TransactionError::IntegrityFailure)?;
            if actual != *reference || witness > transaction.revision {
                return Err(TransactionError::IntegrityFailure);
            }
            if witness == transaction.revision {
                if principal != transaction.principal {
                    return Err(TransactionError::IntegrityFailure);
                }
                first_owners = first_owners
                    .checked_add(1)
                    .ok_or(TransactionError::ResourceLimit)?;
            }
        }
    }
    let journal = recovery.finish_transaction_cursor(cursor)?;
    if first_owners != prefix.owner_count() {
        return Err(TransactionError::IntegrityFailure);
    }
    if let Some(cache) = cache {
        cache_report.correspondence = cache.report().map_err(TransactionError::Storage)?;
    }
    Ok((
        prefix,
        PackedCoordinatorAdmissionReport {
            certificate,
            families: reports
                .try_into()
                .map_err(|_| TransactionError::IntegrityFailure)?,
            journal,
            lookup_pages: limits.maximum_lookup_pages - budget.pages,
            lookup_bytes: limits.maximum_lookup_bytes - budget.bytes,
            first_owners,
        },
        cache_report,
    ))
}
