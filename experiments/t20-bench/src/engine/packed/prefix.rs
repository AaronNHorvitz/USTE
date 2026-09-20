//! Explicit selected-prefix admission. Never silently reconstruct missing roots from origin.
use super::*;

/// Counts in the frozen three-phase BM-01 fixture at one durable revision. This performs no
/// materialization and retains no record/transaction map. Policy is always revision one.
pub(crate) fn counts(
    profile: Bm01Profile,
    revision: uste_types::CommitRevision,
) -> Result<[u64; 8], String> {
    if revision.get() > materialization_revision_count(profile) {
        return Err("packed prefix exceeds fixture frontier".into());
    }
    let batches = revision.get() - 1;
    let width = MAX_TRANSACTION_OPERATIONS as u64;
    let entities = profile.entities() + 1;
    let entity_batches = entities.div_ceil(width);
    let relationships = profile.relationships();
    let relationship_batches = relationships.div_ceil(width);
    let created_entities = (batches * width).min(entities);
    let created_relationships = (batches.saturating_sub(entity_batches) * width).min(relationships);
    let accepted =
        (batches.saturating_sub(entity_batches + relationship_batches) * width).min(relationships);
    Ok([
        created_entities + created_relationships,
        created_entities + created_relationships + accepted,
        accepted,
        accepted,
        created_relationships,
        3 * created_relationships,
        1,
        1,
    ])
}

#[allow(clippy::type_complexity)]
pub(crate) fn recover<
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
>(
    fs: &mut F,
    recovery: RecoveryEngine<F, W, E, I>,
    profile: Bm01Profile,
    revision: uste_types::CommitRevision,
) -> Result<(PackedEngine<F, W, E, I>, Option<[u8; 32]>, u64), String> {
    let limits = Limits::new(profile)?;
    admit_at(
        fs,
        recovery,
        limits,
        Some(revision),
        counts(profile, revision)?,
    )
}

/// Select only a complete same-revision triple, newest first. Missing/invalid manifest attempts
/// follow the storage discovery contract; a selected triple's semantic/page corruption is fatal.
/// At most three fixed-size candidates are retained, across a profile-bounded revision search.
#[allow(clippy::type_complexity)]
pub(crate) fn recover_latest<
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
>(
    fs: &mut F,
    recovery: RecoveryEngine<F, W, E, I>,
    profile: Bm01Profile,
) -> Result<(PackedEngine<F, W, E, I>, u64, u64), String> {
    let limits = Limits::new(profile)?;
    let frontier = recovery
        .authenticated_frontier_anchor()
        .ok_or("missing packed frontier")?
        .0;
    if frontier.get() > limits.legacy.groups {
        return Err("packed frontier exceeds fixture bound".into());
    }
    for value in (1..=frontier.get()).rev() {
        let revision = uste_types::CommitRevision::new(value).map_err(debug)?;
        let mut complete = true;
        for family in [
            GRAPH_PACKED_PROFILE_V1,
            COORDINATOR_PACKED_PROFILE_V1,
            COORDINATOR_PACKED_USAGE_PROFILE_V1,
        ] {
            if optional_root(fs, &recovery, revision, family, limits)?.is_none() {
                complete = false;
                break;
            }
        }
        if complete {
            let (live, _, groups) = recover(fs, recovery, profile, revision)?;
            return Ok((live, value, groups));
        }
    }
    Err("missing paired packed prefix; origin rebuild must be explicit".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prefix_count_boundaries_match_frozen_plan() {
        for entities in [2, 20, 999, 1000, 9999, 10000, 20000, 100000] {
            let profile = Bm01Profile::new(entities).unwrap();
            let mut expected = [0, 0, 0, 0, 0, 0, 1, 1];
            assert_eq!(
                counts(profile, uste_types::CommitRevision::FIRST).unwrap(),
                expected
            );
            disk::visit_disk_batches(profile, |sequence, operations| {
                for operation in operations {
                    match operation {
                        Operation::Create { record, .. } => {
                            expected[0] += 1;
                            expected[1] += 1;
                            if matches!(record, NewRecord::Relationship(_)) {
                                expected[4] += 1;
                                expected[5] += 3;
                            }
                        }
                        Operation::ActOnRelationship { .. } => {
                            expected[1] += 1;
                            expected[2] += 1;
                            expected[3] += 1;
                        }
                        _ => return Err("unexpected fixture operation".into()),
                    }
                }
                assert_eq!(
                    counts(profile, uste_types::CommitRevision::new(sequence).unwrap()).unwrap(),
                    expected
                );
                Ok(())
            })
            .unwrap();
            assert_eq!(expected, disk::fixture_state_counts(profile));
            assert!(
                counts(
                    profile,
                    uste_types::CommitRevision::new(materialization_revision_count(profile) + 1)
                        .unwrap()
                )
                .is_err()
            );
        }
    }
}
