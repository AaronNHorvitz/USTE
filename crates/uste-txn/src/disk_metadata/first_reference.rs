//! Optional first-reference evidence. Publication here is a legacy full-coordinator bridge.
use super::*;

/// SHA-256 of `USTE coordinator-first-reference-v1`.
pub const COORDINATOR_FIRST_REFERENCE_PROFILE_V1: [u8; 32] = [
    0x9a, 0x09, 0xb5, 0xa3, 0x0b, 0x98, 0x47, 0x2a, 0x5f, 0x95, 0xf1, 0xf2, 0x33, 0xf8, 0x46, 0xb6,
    0xdc, 0x97, 0x59, 0xea, 0x2a, 0x11, 0xec, 0xa0, 0xbf, 0xd3, 0xfb, 0x79, 0x15, 0x15, 0xf2, 0x1d,
];

/// Bounds for a temporary first-reference map and single journal pass. The legacy publisher
/// scans the full prefix; disk rebase scans only the post-base suffix and retains new owners.
#[derive(Clone, Copy, Debug)]
pub struct CoordinatorFirstReferenceLimits {
    pub maximum_owners: usize,
    pub maximum_groups: u64,
    pub maximum_encoded_bytes: u64,
}

/// Publish first-reference evidence for a nonempty owner set from a full legacy coordinator.
/// This bridge retains up to `maximum_owners` ID/revision entries; it is not disk-backed
/// incremental construction or larger-than-memory qualification. Empty-owner bases need no proof.
pub fn publish_coordinator_first_reference_index<S, F, W, E, I>(
    coordinator: &mut CommitCoordinator<S, F, W, E, I>,
    filesystem: &mut F,
    limits: CoordinatorFirstReferenceLimits,
) -> Result<DurableIndexRoot, TransactionError>
where
    S: CheckpointState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let (revision, certificate_digest) = coordinator
        .checkpoint_anchor()?
        .ok_or(TransactionError::InvalidRequest)?;
    if coordinator.committed_blob_owners.is_empty() {
        return Err(TransactionError::InvalidRequest);
    }
    if coordinator.committed_blob_owners.len() > limits.maximum_owners
        || limits.maximum_owners > MAX_COMMITTED_BLOBS_PER_JOURNAL
    {
        return Err(TransactionError::ResourceLimit);
    }
    if coordinator.state.current_checkpoint_revision() != Some(revision)
        || coordinator.state.current_checkpoint_scope() != coordinator.scope
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let mut first = BTreeMap::new();
    coordinator
        .journal
        .visit_committed_range(
            filesystem,
            CommitRevision::FIRST,
            revision,
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
                        let expected = coordinator
                            .committed_blob_owners
                            .get(&(reference.scope(), reference.id()))
                            .ok_or(StorageError::IntegrityFailure)?;
                        if expected.0 != *reference {
                            return Err(StorageError::IntegrityFailure);
                        }
                        if let std::collections::btree_map::Entry::Vacant(entry) =
                            first.entry(reference.id())
                        {
                            if expected.1 != decoded.retry_key.principal {
                                return Err(StorageError::IntegrityFailure);
                            }
                            entry.insert(group.revision);
                        }
                    }
                }
                Ok(())
            },
        )
        .map_err(TransactionError::Storage)?;
    if first.len() != coordinator.committed_blob_owners.len() {
        return Err(TransactionError::IntegrityFailure);
    }
    let run = coordinator
        .journal
        .publish_index_run(
            filesystem,
            coordinator.scope,
            revision,
            COORDINATOR_FIRST_REFERENCE_PROFILE_V1,
            1,
            first.into_iter().map(|(id, revision)| IndexEntry {
                key: id.as_bytes().to_vec(),
                value: revision.get().to_be_bytes().to_vec(),
            }),
        )
        .map_err(TransactionError::Storage)?;
    let logical_state_digest = coordinator
        .state
        .current_logical_state_digest()
        .map_err(checkpoint_error)?;
    coordinator
        .journal
        .publish_index_root(
            filesystem,
            IndexRootInput {
                scope: coordinator.scope,
                revision,
                certificate_digest,
                reducer_profile: S::REDUCER_PROFILE,
                logical_state_digest,
                index_profile: COORDINATOR_FIRST_REFERENCE_PROFILE_V1,
            },
            &[run],
        )
        .map_err(TransactionError::Storage)
}

pub(super) fn decode_first_reference(
    key: &[u8],
    value: &[u8],
    frontier: CommitRevision,
) -> Result<CommitRevision, StorageError> {
    if key.len() != 16 || value.len() != 8 {
        return Err(StorageError::IntegrityFailure);
    }
    let revision =
        CommitRevision::new(read_u64(value)?).map_err(|_| StorageError::IntegrityFailure)?;
    if revision > frontier {
        return Err(StorageError::IntegrityFailure);
    }
    Ok(revision)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_first_reference_profile_and_revision_encoding() {
        assert_eq!(
            crate::sha256(b"USTE coordinator-first-reference-v1"),
            COORDINATOR_FIRST_REFERENCE_PROFILE_V1
        );
        let frontier = CommitRevision::new(2).unwrap();
        assert_eq!(
            decode_first_reference(&[7; 16], &1_u64.to_be_bytes(), frontier).unwrap(),
            CommitRevision::FIRST
        );
        for (key, value) in [
            (vec![7; 15], 1_u64.to_be_bytes().to_vec()),
            (vec![7; 17], 1_u64.to_be_bytes().to_vec()),
            (vec![7; 16], vec![0; 7]),
            (vec![7; 16], 0_u64.to_be_bytes().to_vec()),
            (vec![7; 16], 3_u64.to_be_bytes().to_vec()),
        ] {
            assert_eq!(
                decode_first_reference(&key, &value, frontier),
                Err(StorageError::IntegrityFailure)
            );
        }
    }
}
