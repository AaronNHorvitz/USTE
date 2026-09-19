//! Optional first-owner quota projection: principal ordering, exact aggregates, bounded admission.
use super::*;
use uste_storage::{IndexDelta, IndexGetLimits, PageCache};

mod rebuild;
mod recovery;
pub(crate) use rebuild::rebuild_usage;
pub use rebuild::{
    CoordinatorBlobUsageRebuildLimits, CoordinatorBlobUsageRebuildReport,
    MAX_BLOB_USAGE_REBUILD_BATCH_OWNERS,
};
pub use recovery::stage_genesis_blob_usage;
pub(super) use recovery::stage_projection;

/// SHA-256 of `USTE coordinator-blob-usage-v1`.
pub const COORDINATOR_BLOB_USAGE_PROFILE_V1: [u8; 32] = [
    0x41, 0x35, 0x99, 0x42, 0xed, 0xb3, 0x3c, 0xe8, 0x81, 0x7a, 0x23, 0x15, 0x6f, 0x04, 0x0a, 0x13,
    0x6e, 0x60, 0x9a, 0xb0, 0xa6, 0x06, 0x40, 0x88, 0x74, 0x97, 0xfd, 0x14, 0x95, 0xeb, 0x0b, 0x36,
];
const META: u8 = 1;
const PRINCIPALS: u8 = 2;
const OWNERS: u8 = 3;
const KEY: &[u8] = b"blob-usage-v1";

/// Per-run authentication and per-lookup budgets; the admitted owner count bounds all lookups.
#[derive(Clone, Copy, Debug)]
pub struct CoordinatorBlobUsageLimits {
    pub run: IndexRunReadLimits,
    pub lookup: IndexGetLimits,
    pub maximum_owners: u64,
}

#[derive(Debug)]
pub(super) struct BlobUsageIndex {
    root: RecoveredIndexRoot,
    owners: u64,
    bytes: u64,
    principals: u64,
}

impl BlobUsageIndex {
    pub(super) fn owner_count(&self) -> u64 {
        self.owners
    }
    pub(super) fn has_unpublished_root(&self) -> bool {
        self.root.generation() == 0
    }
}

fn add(a: u64, b: u64) -> Result<u64, StorageError> {
    a.checked_add(b).ok_or(StorageError::ResourceLimit)
}

fn pair(count: u64, bytes: u64) -> Vec<u8> {
    [count.to_be_bytes(), bytes.to_be_bytes()].concat()
}

fn decode_pair(value: &[u8]) -> Result<(u64, u64), StorageError> {
    if value.len() != 16 {
        return Err(StorageError::IntegrityFailure);
    }
    Ok((read_u64(&value[..8])?, read_u64(&value[8..])?))
}

impl CoordinatorDiskBase {
    pub fn has_blob_usage_index(&self) -> bool {
        self.usage.is_some()
    }

    /// Attach only after complete independent correspondence with this already admitted base.
    /// The principal ordering is a bijection of primary owners, not a new ownership authority.
    pub fn admit_blob_usage_index<F, W, E, I>(
        &mut self,
        recovery: &AuthenticatedIndexRecovery<F, W, E, I>,
        filesystem: &mut F,
        root: RecoveredIndexRoot,
        limits: CoordinatorBlobUsageLimits,
        cache: &mut PageCache,
    ) -> Result<(), TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        if recovery.scope() != self.metadata.scope() {
            return Err(TransactionError::IntegrityFailure);
        }
        let admitted =
            self.validate_blob_usage_index(&recovery.journal, filesystem, root, limits, cache)?;
        self.usage = Some(admitted);
        Ok(())
    }

    fn validate_blob_usage_index<F, W, E, I>(
        &self,
        journal: &uste_storage::journal::JournalStore<F, W, E, I>,
        filesystem: &mut F,
        root: RecoveredIndexRoot,
        limits: CoordinatorBlobUsageLimits,
        cache: &mut PageCache,
    ) -> Result<BlobUsageIndex, TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        if root.anchor() != self.anchor()
            || root.index_profile() != &COORDINATOR_BLOB_USAGE_PROFILE_V1
        {
            return Err(TransactionError::IntegrityFailure);
        }
        if self.owner_count() > limits.maximum_owners
            || limits.maximum_owners > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64
        {
            return Err(TransactionError::ResourceLimit);
        }
        let mut totals = None;
        journal
            .visit_index_run(filesystem, &root, META, limits.run, &mut |key, value| {
                if key != KEY || value.len() != 24 || totals.is_some() {
                    return Err(StorageError::IntegrityFailure);
                }
                totals = Some((
                    read_u64(&value[..8])?,
                    read_u64(&value[8..16])?,
                    read_u64(&value[16..])?,
                ));
                Ok(())
            })
            .map_err(TransactionError::Storage)?;
        let (owners, bytes, principals) = totals.ok_or(TransactionError::IntegrityFailure)?;
        if owners != self.owner_count() || principals > owners || (owners == 0) != (principals == 0)
        {
            return Err(TransactionError::IntegrityFailure);
        }
        let expected = [(META, 1), (PRINCIPALS, principals), (OWNERS, owners)];
        if root
            .runs()
            .map(|run| (run.family(), run.entry_count()))
            .ne(expected.into_iter().filter(|(_, count)| *count != 0))
        {
            return Err(TransactionError::IntegrityFailure);
        }
        let mut total_bytes = 0;
        let mut seen_principals = 0;
        if owners != 0 {
            // Authenticate all aggregate entries, including otherwise unqueried extras.
            journal
                .visit_index_run(
                    filesystem,
                    &root,
                    PRINCIPALS,
                    limits.run,
                    &mut |key, value| {
                        if key.len() != 32 || decode_pair(value)?.0 == 0 {
                            return Err(StorageError::IntegrityFailure);
                        }
                        Ok(())
                    },
                )
                .map_err(TransactionError::Storage)?;
            let mut cursor = journal
                .open_index_run_cursor(filesystem, &root, OWNERS, limits.run)
                .map_err(TransactionError::Storage)?;
            let mut current: Option<PrincipalDigest> = None;
            let mut actual = (0_u64, 0_u64);
            let mut expected = (0_u64, 0_u64);
            while let Some(entry) = journal
                .next_index_run_entry(filesystem, &mut cursor)
                .map_err(TransactionError::Storage)?
            {
                if entry.key.len() != 48 || entry.value.len() != OWNER_VALUE_BYTES {
                    return Err(TransactionError::IntegrityFailure);
                }
                let (reference, principal) =
                    decode_owner(root.scope(), &entry.key[32..], &entry.value)
                        .map_err(TransactionError::Storage)?;
                if entry.key[..32] != principal.as_bytes()
                    || self.owner_from_journal(
                        journal,
                        filesystem,
                        reference.id(),
                        limits.lookup,
                        cache,
                    )? != Some((reference, principal))
                {
                    return Err(TransactionError::IntegrityFailure);
                }
                if current != Some(principal) {
                    if current.is_some() && actual != expected {
                        return Err(TransactionError::IntegrityFailure);
                    }
                    let (value, _) = journal
                        .index_get_bounded(
                            filesystem,
                            &root,
                            PRINCIPALS,
                            &principal.as_bytes(),
                            limits.lookup,
                            cache,
                        )
                        .map_err(TransactionError::Storage)?;
                    expected = decode_pair(&value.ok_or(TransactionError::IntegrityFailure)?)
                        .map_err(TransactionError::Storage)?;
                    actual = (0, 0);
                    current = Some(principal);
                    seen_principals = add(seen_principals, 1).map_err(TransactionError::Storage)?;
                }
                actual.0 = add(actual.0, 1).map_err(TransactionError::Storage)?;
                actual.1 =
                    add(actual.1, reference.byte_len()).map_err(TransactionError::Storage)?;
                total_bytes =
                    add(total_bytes, reference.byte_len()).map_err(TransactionError::Storage)?;
            }
            if actual != expected
                || journal
                    .finish_index_run_cursor(cursor)
                    .map_err(TransactionError::Storage)?
                    .entries
                    != owners
            {
                return Err(TransactionError::IntegrityFailure);
            }
        }
        if seen_principals != principals || total_bytes != bytes {
            return Err(TransactionError::IntegrityFailure);
        }
        Ok(BlobUsageIndex {
            root,
            owners,
            bytes,
            principals,
        })
    }

    pub(crate) fn indexed_usage<F, W, E, I>(
        &self,
        journal: &uste_storage::journal::JournalStore<F, W, E, I>,
        filesystem: &mut F,
        principal: PrincipalDigest,
        limits: IndexGetLimits,
        cache: &mut PageCache,
    ) -> Result<Option<crate::CommittedBlobUsage>, TransactionError>
    where
        F: OwnershipFileSystem,
        W: DurableKeyEnvelope,
        E: EntropySource,
        I: EntropySource,
    {
        let Some(index) = &self.usage else {
            return Ok(None);
        };
        // Even the empty index authenticates a live root-bound page on each uncached call.
        let (value, _) = journal
            .index_get_bounded(filesystem, &index.root, META, KEY, limits, cache)
            .map_err(TransactionError::Storage)?;
        let mut expected = pair(index.owners, index.bytes);
        expected.extend_from_slice(&index.principals.to_be_bytes());
        if value.as_deref() != Some(expected.as_slice()) {
            return Err(TransactionError::IntegrityFailure);
        }
        let principal_bytes = if index.principals == 0 {
            0
        } else {
            let (value, _) = journal
                .index_get_bounded(
                    filesystem,
                    &index.root,
                    PRINCIPALS,
                    &principal.as_bytes(),
                    limits,
                    cache,
                )
                .map_err(TransactionError::Storage)?;
            value
                .map(|value| decode_pair(&value).map(|(_, bytes)| bytes))
                .transpose()
                .map_err(TransactionError::Storage)?
                .unwrap_or(0)
        };
        Ok(Some(crate::CommittedBlobUsage {
            namespace_bytes: index.bytes,
            principal_bytes,
            owners: index.owners,
        }))
    }
}

pub(super) fn publish_overlay<S, F, W, E, I>(
    coordinator: &mut CommitCoordinator<S, F, W, E, I>,
    filesystem: &mut F,
    base: &CoordinatorDiskBase,
    input: IndexRootInput,
    merge: CoordinatorMetadataRebaseLimits,
    limits: CoordinatorBlobUsageLimits,
) -> Result<BlobUsageIndex, TransactionError>
where
    S: crate::DiskCoordinatorState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if base.usage.as_ref().map_or(0, |index| index.owners) != base.owner_count() {
        return Err(TransactionError::InvalidRequest);
    }
    let merged = merge_projection(
        &mut coordinator.journal,
        filesystem,
        base.usage.as_ref(),
        input,
        merge.merge,
        limits,
        coordinator.committed_blob_owners.values(),
        u64::MAX,
    )?;
    let root = rebase::publish_or_reuse(
        coordinator,
        filesystem,
        IndexRootInput {
            index_profile: COORDINATOR_BLOB_USAGE_PROFILE_V1,
            ..input
        },
        &merged.runs,
        merge.reuse,
    )?;
    Ok(merged.into_index(root))
}

struct MergedUsage {
    owners: u64,
    bytes: u64,
    principals: u64,
    runs: Vec<uste_storage::IndexRunDescriptor>,
    output_bytes: u64,
}

impl MergedUsage {
    fn into_index(self, root: RecoveredIndexRoot) -> BlobUsageIndex {
        BlobUsageIndex {
            root,
            owners: self.owners,
            bytes: self.bytes,
            principals: self.principals,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn merge_projection<'a, F, W, E, I>(
    journal: &mut uste_storage::journal::JournalStore<F, W, E, I>,
    filesystem: &mut F,
    before: Option<&BlobUsageIndex>,
    input: IndexRootInput,
    merge: uste_storage::IndexRunMergeLimits,
    limits: CoordinatorBlobUsageLimits,
    additions: impl ExactSizeIterator<Item = &'a (BlobReference, PrincipalDigest)>,
    maximum_output_bytes: u64,
) -> Result<MergedUsage, TransactionError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let owners = add(
        before.map_or(0, |index| index.owners),
        additions.len() as u64,
    )
    .map_err(TransactionError::Storage)?;
    if owners > limits.maximum_owners {
        return Err(TransactionError::ResourceLimit);
    }
    let mut bytes = before.map_or(0, |index| index.bytes);
    let mut principals = before.map_or(0, |index| index.principals);
    let mut charges: BTreeMap<PrincipalDigest, (u64, u64)> = BTreeMap::new();
    let mut owner_deltas = BTreeMap::new();
    for &(reference, principal) in additions {
        if reference.scope() != input.scope {
            return Err(TransactionError::IntegrityFailure);
        }
        bytes = add(bytes, reference.byte_len()).map_err(TransactionError::Storage)?;
        let charge = charges.entry(principal).or_default();
        charge.0 = add(charge.0, 1).map_err(TransactionError::Storage)?;
        charge.1 = add(charge.1, reference.byte_len()).map_err(TransactionError::Storage)?;
        let encoded = encode_owner(reference, principal).map_err(TransactionError::Storage)?;
        let mut key = principal.as_bytes().to_vec();
        key.extend_from_slice(&encoded.key);
        if owner_deltas.insert(key, encoded.value).is_some() {
            return Err(TransactionError::IntegrityFailure);
        }
    }
    let mut cache = PageCache::new(64 * 1024).map_err(TransactionError::Storage)?;
    let mut deltas = Vec::new();
    for (principal, (count, charged)) in charges {
        let previous = if let Some(index) = before.filter(|index| index.principals != 0) {
            journal
                .index_get_bounded(
                    filesystem,
                    &index.root,
                    PRINCIPALS,
                    &principal.as_bytes(),
                    limits.lookup,
                    &mut cache,
                )
                .map_err(TransactionError::Storage)?
                .0
        } else {
            None
        };
        let (old_count, old_bytes) = previous
            .as_deref()
            .map(decode_pair)
            .transpose()
            .map_err(TransactionError::Storage)?
            .unwrap_or((0, 0));
        if previous.is_none() {
            principals = add(principals, 1).map_err(TransactionError::Storage)?;
        }
        deltas.push(
            IndexDelta::new(
                principal.as_bytes().to_vec(),
                previous,
                Some(pair(
                    add(old_count, count).map_err(TransactionError::Storage)?,
                    add(old_bytes, charged).map_err(TransactionError::Storage)?,
                )),
            )
            .map_err(TransactionError::Storage)?,
        );
    }
    let mut metadata = pair(owners, bytes);
    metadata.extend_from_slice(&principals.to_be_bytes());
    let output_bytes = owners
        .checked_mul(128)
        .and_then(|bytes| {
            principals
                .checked_mul(48)
                .and_then(|part| bytes.checked_add(part))
        })
        .and_then(|bytes| bytes.checked_add(KEY.len() as u64 + 24))
        .filter(|bytes| *bytes <= maximum_output_bytes)
        .ok_or(TransactionError::ResourceLimit)?;
    let mut runs = vec![
        journal
            .publish_index_run(
                filesystem,
                input.scope,
                input.revision,
                COORDINATOR_BLOB_USAGE_PROFILE_V1,
                META,
                [IndexEntry {
                    key: KEY.to_vec(),
                    value: metadata,
                }],
            )
            .map_err(TransactionError::Storage)?,
    ];
    if owners != 0 {
        for (family, changes, count) in [
            (PRINCIPALS, deltas, principals),
            (
                OWNERS,
                owner_deltas
                    .into_iter()
                    .map(|(key, value)| IndexDelta::new(key, None, Some(value)))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(TransactionError::Storage)?,
                owners,
            ),
        ] {
            let merged = journal
                .merge_index_run(
                    filesystem,
                    input.scope,
                    input.revision,
                    COORDINATOR_BLOB_USAGE_PROFILE_V1,
                    family,
                    before
                        .filter(|index| index.owners != 0)
                        .map(|index| &index.root),
                    merge,
                    changes.into_iter().map(Ok),
                )
                .map_err(TransactionError::Storage)?;
            let run = merged.run.ok_or(TransactionError::IntegrityFailure)?;
            if run.entry_count() != count {
                return Err(TransactionError::IntegrityFailure);
            }
            runs.push(run);
        }
    }
    Ok(MergedUsage {
        runs,
        output_bytes,
        owners,
        bytes,
        principals,
    })
}

pub(crate) fn bootstrap_empty_usage<S, F, W, E, I>(
    coordinator: &mut CommitCoordinator<S, F, W, E, I>,
    filesystem: &mut F,
    base: &mut CoordinatorDiskBase,
    limits: CoordinatorMetadataRebaseLimits,
    usage: CoordinatorBlobUsageLimits,
) -> Result<(), TransactionError>
where
    S: crate::DiskCoordinatorState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if base.owner_count() != 0
        || !coordinator.committed_blob_owners.is_empty()
        || !coordinator.outcomes.is_empty()
        || coordinator.checkpoint_anchor()?
            != Some((
                base.metadata.revision(),
                *base.metadata.certificate_digest(),
            ))
    {
        return Err(TransactionError::InvalidRequest);
    }
    if usage.maximum_owners > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64 {
        return Err(TransactionError::ResourceLimit);
    }
    coordinator
        .state
        .validate_metadata_base(&base.metadata)
        .map_err(crate::map_apply_error)?;
    let input = IndexRootInput {
        scope: base.metadata.scope(),
        revision: base.metadata.revision(),
        certificate_digest: *base.metadata.certificate_digest(),
        reducer_profile: *base.metadata.reducer_profile(),
        logical_state_digest: *base.metadata.logical_state_digest(),
        index_profile: COORDINATOR_BLOB_USAGE_PROFILE_V1,
    };
    let next = publish_overlay(coordinator, filesystem, base, input, limits, usage)?;
    base.usage = Some(next);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_usage_profile_pairs_and_overflow_are_exact() {
        assert_eq!(
            crate::sha256(b"USTE coordinator-blob-usage-v1"),
            COORDINATOR_BLOB_USAGE_PROFILE_V1
        );
        for count in [0, 1, u64::MAX] {
            for bytes in [0, 1, u64::MAX] {
                let encoded = pair(count, bytes);
                assert_eq!(decode_pair(&encoded).unwrap(), (count, bytes));
                for size in 0..16 {
                    assert!(decode_pair(&encoded[..size]).is_err());
                }
                let mut extra = encoded;
                extra.push(0);
                assert!(decode_pair(&extra).is_err());
            }
        }
        assert_eq!(add(u64::MAX, 0).unwrap(), u64::MAX);
        assert_eq!(add(u64::MAX, 1), Err(StorageError::ResourceLimit));
    }
}
