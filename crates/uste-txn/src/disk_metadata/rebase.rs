//! Streaming overlay publication with retry-safe reuse of a partially published root set.

use super::*;
use uste_storage::{IndexDelta, IndexRunDescriptor, IndexRunMergeLimits};

/// Per-family merge and root verification limits (reuse and fallback-slot selection). Three data families
/// and one fixed metadata entry are processed, plus an optional first-reference family.
/// These bounds are not aggregate across families.
#[derive(Clone, Copy, Debug)]
pub struct CoordinatorMetadataRebaseLimits {
    pub merge: IndexRunMergeLimits,
    pub reuse: IndexRunReadLimits,
}

pub(crate) fn publish_overlay_base<S, F, W, E, I>(
    coordinator: &mut CommitCoordinator<S, F, W, E, I>,
    filesystem: &mut F,
    base: &CoordinatorDiskBase,
    limits: CoordinatorMetadataRebaseLimits,
    first_reference_limits: Option<CoordinatorFirstReferenceLimits>,
    usage_limits: Option<CoordinatorBlobUsageLimits>,
) -> Result<CoordinatorDiskBase, TransactionError>
where
    S: crate::DiskCoordinatorState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let anchor = coordinator
        .checkpoint_anchor()?
        .ok_or(TransactionError::IntegrityFailure)?;
    if (base.usage.is_some() && usage_limits.is_none())
        || (usage_limits.is_some() && base.usage.is_none() && base.owner_count() != 0)
    {
        return Err(TransactionError::InvalidRequest);
    }
    if let Some(usage) = usage_limits {
        let count = base
            .owner_count()
            .checked_add(coordinator.committed_blob_owners.len() as u64)
            .ok_or(TransactionError::ResourceLimit)?;
        if count > usage.maximum_owners
            || usage.maximum_owners > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64
        {
            return Err(TransactionError::ResourceLimit);
        }
    }
    let input = coordinator
        .state
        .metadata_publication_input(anchor)
        .map_err(crate::map_apply_error)?;
    if (base.first_references.is_some() && first_reference_limits.is_none())
        || (first_reference_limits.is_some()
            && base.first_references.is_none()
            && base.owner_count() != 0)
    {
        return Err(TransactionError::InvalidRequest);
    }
    for profile in [
        COORDINATOR_METADATA_PROFILE_V1,
        COORDINATOR_TRANSACTION_PROFILE_V1,
        COORDINATOR_FIRST_REFERENCE_PROFILE_V1,
        COORDINATOR_BLOB_USAGE_PROFILE_V1,
    ] {
        if profile == COORDINATOR_BLOB_USAGE_PROFILE_V1 && usage_limits.is_none() {
            continue;
        }
        if coordinator
            .load_index_root_manifests(filesystem, profile)?
            .iter()
            .any(|root| root.revision() > base.metadata.revision() && root.revision() != anchor.0)
        {
            // Do not rotate away the pinned older pair when a different writer path advanced
            // beyond an incomplete rebase. Explicit cache rebuild is required in this case.
            return Err(TransactionError::ResourceLimit);
        }
    }
    if input.scope != coordinator.scope
        || (input.revision, input.certificate_digest) != anchor
        || input.index_profile != COORDINATOR_METADATA_PROFILE_V1
        || input.reducer_profile != *base.metadata.reducer_profile()
        || coordinator.outcomes.len() != coordinator.transactions.len()
        || base
            .metadata
            .revision()
            .get()
            .checked_add(coordinator.outcomes.len() as u64)
            != Some(anchor.0.get())
        || coordinator.outcomes.iter().any(|(key, outcome)| {
            outcome.revision <= base.metadata.revision()
                || outcome.revision > anchor.0
                || coordinator.transactions.get(&outcome.transaction_id)
                    != Some(&(key.principal, *outcome))
        })
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let owner_count = base
        .metadata
        .runs()
        .find(|run| run.family() == FAMILY_BLOB_OWNER)
        .map_or(0, |run| run.entry_count())
        .checked_add(coordinator.committed_blob_owners.len() as u64)
        .ok_or(TransactionError::ResourceLimit)?;
    if owner_count > MAX_COMMITTED_BLOBS_PER_JOURNAL as u64 {
        return Err(TransactionError::ResourceLimit);
    }
    let first_references = first_reference_limits
        .map(|suffix| first_reference_overlay(coordinator, filesystem, base, anchor.0, suffix))
        .transpose()?;
    let retry_deltas = coordinator.outcomes.iter().map(|(key, outcome)| {
        let entry = encode_outcome(key.principal, key.key, *outcome)?;
        IndexDelta::new(entry.key, None, Some(entry.value))
    });
    let retry = coordinator
        .journal
        .merge_index_run(
            filesystem,
            coordinator.scope,
            anchor.0,
            COORDINATOR_METADATA_PROFILE_V1,
            FAMILY_OUTCOME,
            Some(&base.metadata),
            limits.merge,
            retry_deltas,
        )
        .map_err(TransactionError::Storage)?;
    let retry = exact_merged_run(retry, anchor.0.get(), coordinator.outcomes.len())?;
    let transaction_deltas = coordinator
        .transactions
        .iter()
        .map(|(id, (principal, outcome))| {
            let entry = encode_outcome(*principal, IdempotencyKey::from_bytes([0; 16]), *outcome)?;
            let mut value = Vec::with_capacity(136);
            value.extend_from_slice(&principal.as_bytes());
            value.extend_from_slice(&entry.value);
            IndexDelta::new(id.as_bytes().to_vec(), None, Some(value))
        });
    let transaction = coordinator
        .journal
        .merge_index_run(
            filesystem,
            coordinator.scope,
            anchor.0,
            COORDINATOR_TRANSACTION_PROFILE_V1,
            1,
            Some(&base.transactions.root),
            limits.merge,
            transaction_deltas,
        )
        .map_err(TransactionError::Storage)?;
    let transaction =
        exact_merged_run(transaction, anchor.0.get(), coordinator.transactions.len())?;
    let mut runs = Vec::with_capacity(3);
    let metadata = coordinator
        .journal
        .publish_index_run(
            filesystem,
            coordinator.scope,
            anchor.0,
            COORDINATOR_METADATA_PROFILE_V1,
            FAMILY_METADATA,
            [IndexEntry {
                key: METADATA_KEY.to_vec(),
                value: metadata_value(anchor.0, anchor.0.get(), owner_count),
            }],
        )
        .map_err(TransactionError::Storage)?;
    runs.push(metadata);
    runs.push(retry);
    if owner_count != 0 {
        let deltas = coordinator
            .committed_blob_owners
            .values()
            .map(|(reference, principal)| {
                if reference.scope() != coordinator.scope {
                    return Err(StorageError::IntegrityFailure);
                }
                let entry = encode_owner(*reference, *principal)?;
                IndexDelta::new(entry.key, None, Some(entry.value))
            });
        let owner_base = base
            .metadata
            .runs()
            .any(|run| run.family() == FAMILY_BLOB_OWNER)
            .then_some(&base.metadata);
        let owner = coordinator
            .journal
            .merge_index_run(
                filesystem,
                coordinator.scope,
                anchor.0,
                COORDINATOR_METADATA_PROFILE_V1,
                FAMILY_BLOB_OWNER,
                owner_base,
                limits.merge,
                deltas,
            )
            .map_err(TransactionError::Storage)?;
        runs.push(exact_merged_run(
            owner,
            owner_count,
            coordinator.committed_blob_owners.len(),
        )?);
    }
    let first_run = if let Some(first) = first_references
        && owner_count != 0
    {
        let insertions = first.len();
        let deltas = first.into_iter().map(|(id, claim)| {
            IndexDelta::new(
                id.as_bytes().to_vec(),
                None,
                Some(claim.revision.get().to_be_bytes().to_vec()),
            )
        });
        let merged = coordinator
            .journal
            .merge_index_run(
                filesystem,
                coordinator.scope,
                anchor.0,
                COORDINATOR_FIRST_REFERENCE_PROFILE_V1,
                1,
                base.first_references.as_ref(),
                limits.merge,
                deltas,
            )
            .map_err(TransactionError::Storage)?;
        Some(exact_merged_run(merged, owner_count, insertions)?)
    } else {
        None
    };
    let usage = usage_limits
        .map(|usage_limits| {
            usage::publish_overlay(coordinator, filesystem, base, input, limits, usage_limits)
        })
        .transpose()?;
    let metadata = publish_or_reuse(coordinator, filesystem, input, &runs, limits.reuse)?;
    let transactions = publish_or_reuse(
        coordinator,
        filesystem,
        IndexRootInput {
            index_profile: COORDINATOR_TRANSACTION_PROFILE_V1,
            ..input
        },
        &[transaction],
        limits.reuse,
    )?;
    let first_references = first_run
        .map(|run| {
            publish_or_reuse(
                coordinator,
                filesystem,
                IndexRootInput {
                    index_profile: COORDINATOR_FIRST_REFERENCE_PROFILE_V1,
                    ..input
                },
                &[run],
                limits.reuse,
            )
        })
        .transpose()?;
    Ok(CoordinatorDiskBase {
        metadata,
        transactions: CoordinatorTransactionIndex { root: transactions },
        first_references,
        usage,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FirstReferenceClaim {
    revision: CommitRevision,
    owner_matches: bool,
}

fn record_first_reference(
    first: &mut BTreeMap<BlobId, FirstReferenceClaim>,
    id: BlobId,
    claim: FirstReferenceClaim,
) -> Result<(), StorageError> {
    if first
        .get(&id)
        .is_some_and(|previous| previous.revision <= claim.revision)
    {
        return Err(StorageError::IntegrityFailure);
    }
    first.insert(id, claim);
    Ok(())
}

fn validate_first_references(
    first: &BTreeMap<BlobId, FirstReferenceClaim>,
    expected: usize,
) -> Result<(), TransactionError> {
    if first.len() != expected || first.values().any(|claim| !claim.owner_matches) {
        return Err(TransactionError::IntegrityFailure);
    }
    Ok(())
}

fn first_reference_overlay<S, F, W, E, I>(
    coordinator: &CommitCoordinator<S, F, W, E, I>,
    filesystem: &mut F,
    base: &CoordinatorDiskBase,
    frontier: CommitRevision,
    limits: CoordinatorFirstReferenceLimits,
) -> Result<BTreeMap<BlobId, FirstReferenceClaim>, TransactionError>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    if coordinator.committed_blob_owners.len() > limits.maximum_owners
        || limits.maximum_owners > MAX_COMMITTED_BLOBS_PER_JOURNAL
    {
        return Err(TransactionError::ResourceLimit);
    }
    let mut first = BTreeMap::new();
    if base.metadata.revision() < frontier {
        let start = CommitRevision::new(base.metadata.revision().get() + 1)
            .map_err(|_| TransactionError::IntegrityFailure)?;
        coordinator
            .journal
            .visit_committed_range_reverse_report(
                filesystem,
                start,
                frontier,
                limits.maximum_groups,
                limits.maximum_encoded_bytes,
                |_, group| {
                    if crate::sha256(group.encoded_group) != group.logical_event_digest {
                        return Err(StorageError::IntegrityFailure);
                    }
                    let decoded = crate::decode_group(
                        coordinator.scope,
                        group.encoded_group,
                        group.revision,
                        group.blob_inventory_digest,
                        group.blob_inventory,
                    )
                    .map_err(|_| StorageError::IntegrityFailure)?;
                    if let Some(inventory) = decoded.blob_inventory {
                        for reference in inventory.references() {
                            // Existing-base references were already checked at admission/commit.
                            // Only disjoint, bounded new-owner overlays need new first-revision evidence.
                            let Some(expected) = coordinator
                                .committed_blob_owners
                                .get(&(reference.scope(), reference.id()))
                            else {
                                continue;
                            };
                            if expected.0 != *reference {
                                return Err(StorageError::IntegrityFailure);
                            }
                            // Descending occurrences replace later claims. A foreign later
                            // reference is legal; only the eventual earliest principal matters.
                            record_first_reference(
                                &mut first,
                                reference.id(),
                                FirstReferenceClaim {
                                    revision: group.revision,
                                    owner_matches: expected.1 == decoded.retry_key.principal,
                                },
                            )?;
                        }
                    }
                    Ok(())
                },
            )
            .map_err(TransactionError::Storage)?;
    }
    validate_first_references(&first, coordinator.committed_blob_owners.len())?;
    Ok(first)
}

fn exact_merged_run(
    merged: uste_storage::MergedIndexRun,
    entries: u64,
    insertions: usize,
) -> Result<IndexRunDescriptor, TransactionError> {
    if merged.report.output_entries != entries
        || merged.report.insertions != insertions as u64
        || merged.report.replacements != 0
        || merged.report.deletions != 0
    {
        return Err(TransactionError::IntegrityFailure);
    }
    merged.run.ok_or(TransactionError::IntegrityFailure)
}

pub(super) fn publish_or_reuse<S, F, W, E, I>(
    coordinator: &mut CommitCoordinator<S, F, W, E, I>,
    filesystem: &mut F,
    input: IndexRootInput,
    runs: &[IndexRunDescriptor],
    limits: IndexRunReadLimits,
) -> Result<RecoveredIndexRoot, TransactionError>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    for root in coordinator.load_index_root_manifests(filesystem, input.index_profile)? {
        if root.revision() != input.revision {
            continue;
        }
        // Repeated partial-pair publication must not rotate away the old matching root. Reuse
        // only a fully authenticated current candidate matching freshly merged logical content.
        if root.scope() != input.scope
            || root.certificate_digest() != &input.certificate_digest
            || root.reducer_profile() != &input.reducer_profile
            || root.logical_state_digest() != &input.logical_state_digest
            || root.runs().len() != runs.len()
            || root.runs().zip(runs).any(|(old, new)| {
                old.family() != new.family()
                    || old.entry_count() != new.entry_count()
                    || old.logical_digest() != new.logical_digest()
            })
        {
            return Err(TransactionError::IntegrityFailure);
        }
        coordinator
            .journal
            .resync_index_root_bounded(filesystem, &root, limits)
            .map_err(TransactionError::Storage)?;
        return Ok(root);
    }
    coordinator
        .journal
        .publish_index_root_recovered_bounded(filesystem, input, runs, limits)
        .map_err(TransactionError::Storage)
}

#[cfg(test)]
mod reverse_first_reference_tests {
    use super::*;

    #[test]
    fn reverse_first_reference_claims_match_independent_earliest_owner_selection() {
        assert!(size_of::<FirstReferenceClaim>() <= 2 * size_of::<CommitRevision>());
        // Enumerate all principal-match histories: a late correct principal cannot repair an
        // incorrect earliest owner, and a late foreign reference cannot displace a correct one.
        for bits in 0_u16..256 {
            let id = BlobId::from_bytes([1; 16]);
            let mut first = BTreeMap::new();
            for number in (1..=8).rev() {
                record_first_reference(
                    &mut first,
                    id,
                    FirstReferenceClaim {
                        revision: CommitRevision::new(number).unwrap(),
                        owner_matches: bits & (1 << (number - 1)) != 0,
                    },
                )
                .unwrap();
            }
            assert_eq!(first[&id].revision, CommitRevision::FIRST);
            assert_eq!(validate_first_references(&first, 1).is_ok(), bits & 1 != 0);
            assert!(validate_first_references(&first, 0).is_err());
            assert!(validate_first_references(&first, 2).is_err());
            let before = first.clone();
            for number in [1, 2] {
                assert!(
                    record_first_reference(
                        &mut first,
                        id,
                        FirstReferenceClaim {
                            revision: CommitRevision::new(number).unwrap(),
                            owner_matches: true,
                        }
                    )
                    .is_err()
                );
                assert_eq!(first, before);
            }
        }
    }
}
