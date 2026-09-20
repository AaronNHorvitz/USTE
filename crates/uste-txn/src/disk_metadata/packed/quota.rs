//! Private first-owner accounting; no consumer policy or publication authority.
mod admission;
mod live_reads;
mod rebuild;
use super::*;
pub use admission::{
    PackedQuotaAdmissionCacheReport, PackedQuotaAdmissionLimits, PackedQuotaAdmissionReport,
    admit_packed_quota_prefix, admit_packed_quota_prefix_buffered,
};
pub use rebuild::{
    PackedQuotaRebuildLimits, PackedQuotaRebuildReport, rebuild_packed_quota_prefix,
};
use uste_storage::journal::JournalStore;
use uste_storage::ordered_commitment::OrderedCommitment;

pub const COORDINATOR_PACKED_USAGE_PROFILE_V1: [u8; 32] = [
    0xd0, 0xfe, 0x0c, 0x60, 0x7d, 0xf2, 0x2b, 0xcb, 0x3f, 0xfa, 0xda, 0xca, 0xc4, 0x42, 0xed, 0x0f,
    0x17, 0xd3, 0x91, 0xec, 0x59, 0x51, 0xe1, 0x46, 0x7a, 0x2f, 0x3f, 0xaf, 0xb3, 0x25, 0x44, 0x01,
];
const KEY: &[u8] = b"packed-usage-v1";
pub struct PackedQuotaPrefix {
    scope: NamespaceRef,
    anchor: (CommitRevision, [u8; 32]),
    primary: [OrderedCommitment; 4],
    trees: [CanonicalPackedTree; 3],
    owners: u64,
    bytes: u64,
    principals: u64,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PackedQuotaReport {
    pub staging_caches: [Option<uste_storage::packed_page_cache::PackedCacheReport>; 3],
    pub primary_lookup_pages: u64,
    pub primary_lookup_bytes: u64,
    pub principal_lookup: Option<TreeLookupReport>,
    pub new_owners: u64,
    pub charged_bytes: u64,
    pub batches: [TreeBatchReport; 3],
}
fn add(a: u64, b: u64) -> Result<u64, TransactionError> {
    a.checked_add(b).ok_or(TransactionError::ResourceLimit)
}
fn pair(count: u64, bytes: u64) -> Vec<u8> {
    [count.to_be_bytes(), bytes.to_be_bytes()].concat()
}
fn decode_pair(value: &[u8]) -> Result<(u64, u64), TransactionError> {
    if value.len() != 16 {
        return Err(TransactionError::IntegrityFailure);
    }
    Ok((
        read_u64(&value[..8]).map_err(TransactionError::Storage)?,
        read_u64(&value[8..]).map_err(TransactionError::Storage)?,
    ))
}
fn head(owners: u64, bytes: u64, principals: u64) -> Vec<u8> {
    [
        owners.to_be_bytes(),
        bytes.to_be_bytes(),
        principals.to_be_bytes(),
    ]
    .concat()
}

impl PackedQuotaPrefix {
    pub(crate) fn validate_live_pair<F, W, E, I>(
        &self,
        journal: &JournalStore<F, W, E, I>,
        primary: &PackedCoordinatorPrefix,
    ) -> Result<(), TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        if !self.paired(primary) {
            return Err(TransactionError::IntegrityFailure);
        }
        for tree in primary.trees.iter().chain(self.trees.iter()) {
            journal
                .validate_packed_tree_binding(tree)
                .map_err(TransactionError::Storage)?;
        }
        Ok(())
    }
    pub fn anchor(&self) -> (CommitRevision, [u8; 32]) {
        self.anchor
    }
    pub fn families(&self) -> [PackedRootFamily; 3] {
        core::array::from_fn(|i| self.trees[i].family_descriptor())
    }
    fn paired(&self, primary: &PackedCoordinatorPrefix) -> bool {
        self.scope == primary.scope
            && self.anchor == primary.anchor
            && self.primary == primary.families().map(|f| f.commitment)
    }
    /// Raw maintenance accounting only. A facade must apply current InspectQuota authority first.
    pub fn usage<F, W, E, I>(
        &self,
        maintenance: &PackedIndexMaintenance<'_, F, W, E, I>,
        fs: &mut F,
        primary: &PackedCoordinatorPrefix,
        principal: PrincipalDigest,
        limits: TreeLookupLimits,
    ) -> Result<(crate::CommittedBlobUsage, [TreeLookupReport; 2]), TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        if !self.paired(primary) {
            return Err(TransactionError::IntegrityFailure);
        }
        for tree in &primary.trees {
            maintenance.validate_tree_binding(tree)?;
        }
        self.read_usage(principal, |tree, key| {
            maintenance.get(fs, tree, key, limits)
        })
    }
    fn read_usage(
        &self,
        principal: PrincipalDigest,
        mut get: impl FnMut(
            &CanonicalPackedTree,
            &[u8],
        ) -> Result<
            uste_storage::packed_tree_lookup::TreeLookupResult,
            TransactionError,
        >,
    ) -> Result<(crate::CommittedBlobUsage, [TreeLookupReport; 2]), TransactionError> {
        let metadata = get(&self.trees[0], KEY)?;
        if metadata.value.as_ref().map(|v| v.as_slice())
            != Some(head(self.owners, self.bytes, self.principals).as_slice())
        {
            return Err(TransactionError::IntegrityFailure);
        }
        let result = get(&self.trees[1], &principal.as_bytes())?;
        let principal_bytes = result
            .value
            .as_ref()
            .map(|v| decode_pair(v.as_slice()).map(|p| p.1))
            .transpose()?
            .unwrap_or(0);
        Ok((
            crate::CommittedBlobUsage {
                namespace_bytes: self.bytes,
                principal_bytes,
                owners: self.owners,
            },
            [metadata.report, result.report],
        ))
    }
}

pub fn stage_packed_quota_prefix<F, W, E, I>(
    recovery: &mut AuthenticatedIndexRecovery<F, W, E, I>,
    fs: &mut F,
    before: Option<&PackedQuotaPrefix>,
    primary: &PackedCoordinatorPrefix,
    transaction: &RecoveredFrontierTransaction,
    limits: PackedCoordinatorLimits,
) -> Result<(PackedQuotaPrefix, PackedQuotaReport), TransactionError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    stage_packed_quota_prefix_on_journal(
        recovery.scope(),
        &mut recovery.journal,
        fs,
        before,
        primary,
        transaction,
        limits,
    )
}

pub(crate) fn stage_packed_quota_prefix_on_journal<F, W, E, I>(
    scope: NamespaceRef,
    journal: &mut JournalStore<F, W, E, I>,
    fs: &mut F,
    before: Option<&PackedQuotaPrefix>,
    primary: &PackedCoordinatorPrefix,
    transaction: &RecoveredFrontierTransaction,
    limits: PackedCoordinatorLimits,
) -> Result<(PackedQuotaPrefix, PackedQuotaReport), TransactionError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if primary.scope != scope
        || transaction.scope != primary.scope
        || primary.anchor != (transaction.revision, transaction.certificate_digest)
        || before.is_none() && transaction.revision != CommitRevision::FIRST
        || before.is_some_and(|b| {
            b.scope != primary.scope || b.anchor.0.checked_next().ok() != Some(transaction.revision)
        })
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let references = transaction
        .blob_inventory
        .as_ref()
        .map_or(&[][..], |i| i.references());
    if limits.maximum_references > MAX_BATCH_DELTAS
        || references.len() > limits.maximum_references
        || limits.maximum_owners > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64
        || primary.owner_count() > limits.maximum_owners
    {
        return Err(TransactionError::ResourceLimit);
    }
    validate_staging_cache(limits.staging_cache_bytes)?;
    let mut maintenance =
        transaction.packed_maintenance(scope, journal, fs, limits.certificates)?;
    let mut report = PackedQuotaReport::default();
    // Bind the supplied primary capability even when the inventory is empty.
    let (outcome, work) = primary.retry(
        &maintenance,
        fs,
        transaction.principal,
        transaction.idempotency_key,
        limits.lookup,
    )?;
    if outcome != Some(transaction.outcome) {
        return Err(TransactionError::IntegrityFailure);
    }
    report.primary_lookup_pages = work.pages;
    report.primary_lookup_bytes = work.encoded_bytes;
    let mut owners = Vec::new();
    owners
        .try_reserve_exact(references.len())
        .map_err(|_| TransactionError::ResourceLimit)?;
    for reference in references {
        let (owner, work) = primary.owner(&maintenance, fs, reference.id(), limits.lookup)?;
        report.primary_lookup_pages = add(report.primary_lookup_pages, work.pages)?;
        report.primary_lookup_bytes = add(report.primary_lookup_bytes, work.encoded_bytes)?;
        let (witness, work) =
            primary.first_revision(&maintenance, fs, reference.id(), limits.lookup)?;
        report.primary_lookup_pages = add(report.primary_lookup_pages, work.pages)?;
        report.primary_lookup_bytes = add(report.primary_lookup_bytes, work.encoded_bytes)?;
        let (actual, principal) = owner.ok_or(TransactionError::IntegrityFailure)?;
        let witness = witness.ok_or(TransactionError::IntegrityFailure)?;
        if actual != *reference || witness > transaction.revision {
            return Err(TransactionError::IntegrityFailure);
        }
        if witness != transaction.revision {
            continue;
        }
        if principal != transaction.principal {
            return Err(TransactionError::IntegrityFailure);
        }
        report.new_owners = add(report.new_owners, 1)?;
        report.charged_bytes = add(report.charged_bytes, reference.byte_len())?;
        let entry = encode_owner(*reference, principal).map_err(TransactionError::Storage)?;
        let mut key = principal.as_bytes().to_vec();
        key.extend_from_slice(&entry.key);
        owners.push(
            IndexDelta::new(key, None, Some(entry.value)).map_err(TransactionError::Storage)?,
        );
    }
    let owner_count = add(before.map_or(0, |b| b.owners), report.new_owners)?;
    if owner_count != primary.owner_count() {
        return Err(TransactionError::IntegrityFailure);
    }
    let bytes = add(before.map_or(0, |b| b.bytes), report.charged_bytes)?;
    let mut principal_count = before.map_or(0, |b| b.principals);
    let mut principal_delta = None;
    if report.new_owners != 0 {
        let old = if let Some(before) = before {
            let result = maintenance.get(
                fs,
                &before.trees[1],
                &transaction.principal.as_bytes(),
                limits.lookup,
            )?;
            report.principal_lookup = Some(result.report);
            result.value.map(|v| v.as_slice().to_vec())
        } else {
            None
        };
        let (count, previous_bytes) = old
            .as_deref()
            .map(decode_pair)
            .transpose()?
            .unwrap_or((0, 0));
        if old.is_none() {
            principal_count = add(principal_count, 1)?;
        }
        principal_delta = Some(
            IndexDelta::new(
                transaction.principal.as_bytes().to_vec(),
                old,
                Some(pair(
                    add(count, report.new_owners)?,
                    add(previous_bytes, report.charged_bytes)?,
                )),
            )
            .map_err(TransactionError::Storage)?,
        );
    }
    let metadata = [IndexDelta::new(
        KEY.to_vec(),
        before.map(|b| head(b.owners, b.bytes, b.principals)),
        Some(head(owner_count, bytes, principal_count)),
    )
    .map_err(TransactionError::Storage)?];
    let mut trees = Vec::new();
    trees
        .try_reserve_exact(3)
        .map_err(|_| TransactionError::ResourceLimit)?;
    for (index, deltas) in [
        metadata.as_slice(),
        principal_delta.as_slice(),
        owners.as_slice(),
    ]
    .into_iter()
    .enumerate()
    {
        let stage = if let Some(bytes) = limits.staging_cache_bytes {
            let (stage, cache) = maintenance.stage_buffered(
                fs,
                COORDINATOR_PACKED_USAGE_PROFILE_V1,
                index as u8 + 1,
                before.map(|b| &b.trees[index]),
                deltas,
                limits.batch,
                bytes,
            )?;
            report.staging_caches[index] = Some(cache);
            stage
        } else {
            maintenance.stage(
                fs,
                COORDINATOR_PACKED_USAGE_PROFILE_V1,
                index as u8 + 1,
                before.map(|b| &b.trees[index]),
                deltas,
                limits.batch,
            )?
        };
        report.batches[index] = stage.report();
        trees.push(stage.tree().clone());
    }
    let trees: [CanonicalPackedTree; 3] = trees
        .try_into()
        .map_err(|_| TransactionError::IntegrityFailure)?;
    if trees[0].family_descriptor().commitment.entries() != 1
        || trees[1].family_descriptor().commitment.entries() != principal_count
        || trees[2].family_descriptor().commitment.entries() != owner_count
    {
        return Err(TransactionError::IntegrityFailure);
    }
    Ok((
        PackedQuotaPrefix {
            scope: primary.scope,
            anchor: primary.anchor,
            primary: primary.families().map(|f| f.commitment),
            trees,
            owners: owner_count,
            bytes,
            principals: principal_count,
        },
        report,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packed_quota_checked_counts_and_exact_pair_framing() {
        assert_eq!(add(u64::MAX, 0), Ok(u64::MAX));
        assert_eq!(add(u64::MAX, 1), Err(TransactionError::ResourceLimit));
        assert_eq!(decode_pair(&pair(1, 0)), Ok((1, 0)));
        for length in [0, 15, 17] {
            assert!(decode_pair(&vec![0; length]).is_err());
        }
        assert_eq!(head(1, 0, 1).len(), 24);
    }
}
