//! Packed BM-06 development history equivalence, never a native recovery timing campaign.
use super::*;
use crate::recovery_materialization::{Bm06Profile, PAYLOAD_BYTES, VERSIONS};

#[derive(Debug)]
pub struct PackedRecoveryVerification {
    pub records: u64,
    pub verified_versions: u64,
    pub verified_payload_bytes: u64,
    pub base_revision: u64,
    pub recovered_revision: u64,
    pub origin_suffix_groups: u64,
    pub v1_state_digest: [u8; 32],
}

/// Same literal payload/version oracle as the v1 runner, through authorized packed historical reads.
pub(crate) fn verify_history<
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
>(
    live: &PackedEngine<F, W, E, I>,
    fs: &mut F,
    kernel: &PolicyKernel,
    principal: &AuthenticatedPrincipal,
    profile: Bm06Profile,
    versions: u64,
) -> Result<u64, String> {
    if versions == 0 || versions > VERSIONS {
        return Err("BM-06 unsupported generation frontier".into());
    }
    verify_prefix_history(
        live,
        fs,
        kernel,
        principal,
        profile,
        1 + versions * profile.batches_per_version(),
    )
}

/// Stream all and only the events committed at an arbitrary (possibly partial-generation) prefix.
pub(crate) fn verify_prefix_history<
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
>(
    live: &PackedEngine<F, W, E, I>,
    fs: &mut F,
    kernel: &PolicyKernel,
    principal: &AuthenticatedPrincipal,
    profile: Bm06Profile,
    frontier: u64,
) -> Result<u64, String> {
    let counts = profile.prefix_state_counts(frontier)?;
    let base = live
        .state()
        .map_err(debug)?
        .current_base()
        .ok_or("BM-06 pending packed base")?;
    if base.anchor().0.get() != frontier
        || base.families().map(|family| family.commitment.entries())
            != [1, counts[0], counts[1], 0, 0, 0, 0, 2]
    {
        return Err("BM-06 packed profile/frontier mismatch".into());
    }
    let mut read = Limits::recovery(profile)?.read()?;
    read.expansion = None;
    let reader =
        AuthorizedPackedReader::new_with_cache_budget(live, kernel, read, 64 * 1024 * 1024)
            .map_err(debug)?;
    let mut verified = 0;
    for ordinal in 0..counts[1] {
        let generation = ordinal / profile.records();
        let record = ordinal % profile.records();
        let revision =
            uste_types::CommitRevision::new(profile.event_revision(ordinal)?).map_err(debug)?;
        let actual = reader
            .read(
                fs,
                principal,
                &GraphReadRequest::RecordAt {
                    id: profile.record_ref(scope(), record)?,
                    revision,
                },
                &NeverCancel,
            )
            .map_err(debug)?;
        let GraphReadOutput::Record(Some(actual)) = actual else {
            return Err("BM-06 missing packed history".into());
        };
        let uste_graph::Record::Entity(actual) = *actual else {
            return Err("BM-06 wrong packed record kind".into());
        };
        if actual.version.get() != generation + 1
            || actual.modified_revision != revision
            || actual.id != profile.record_ref(scope(), record)?
            || actual.created_revision.get() != profile.event_revision(record)?
            || actual.lifecycle != uste_graph::EntityLifecycle::Active
            || actual.entity_type.as_str() != crate::recovery_materialization::PROFILE
            || actual.schema_version != 1
            || actual.properties
                != Value::bytes(profile.payload(ordinal)?.to_vec()).map_err(debug)?
        {
            return Err("BM-06 packed historical oracle mismatch".into());
        }
        verified += 1;
    }
    Ok(verified)
}

fn batch(profile: Bm06Profile, sequence: u64) -> Result<disk::DiskBatch, String> {
    Ok(disk::DiskBatch {
        sequence,
        idempotency_key: identity(sequence, IdempotencyKey::from_bytes),
        transaction_id: identity(sequence, TransactionId::from_bytes),
        operations: profile.batch(scope(), sequence)?,
    })
}

pub fn verify(profile: Bm06Profile) -> Result<PackedRecoveryVerification, String> {
    if profile.records() > crate::engine::recovery::MAX_DEVELOPMENT_RECORDS {
        return Err("BM-06 packed memory-adapter verifier accepts at most 2 records".into());
    }
    let limits = Limits::recovery(profile)?;
    let policy = crate::engine::recovery::recovery_policy()?;
    let mut fs = MemoryFileSystem::default();
    let name = EntryName::new("bm06-packed-development").map_err(debug)?;
    let vault = KeyVault::create(
        scope().database(),
        &mut TestKeyAdapter,
        CounterEntropy(80_000_001),
    )
    .map_err(debug)?;
    let mut raw = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).map_err(debug)?,
        name.clone(),
        vault,
        CounterEntropy(81_000_001),
        GraphState::new(scope()),
    )
    .map_err(debug)?;
    let encoded = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        vec![],
        uste_graph::DurablePolicyMutation::Install {
            policy: policy.clone(),
        },
    ))
    .map_err(debug)?;
    raw.commit(
        &mut fs,
        TransactionRequest {
            principal: PRINCIPAL,
            idempotency_key: identity(1, IdempotencyKey::from_bytes),
            transaction_id: identity(1, TransactionId::from_bytes),
            canonical_request: &encoded,
            blob_inventory: None,
        },
        &mut clock(1),
        &NeverCancel,
    )
    .map_err(debug)?;
    drop(raw);
    let recovery = open(&mut fs, &name, limits, 82_000_000)?;
    let (mut live, _) = recover_packed_graph_origin(
        recovery,
        &mut fs,
        RetentionDays::new(30).map_err(debug)?,
        CoordinatorRecoveryLimits::new(1, 0).map_err(debug)?,
        limits.origin,
    )
    .map_err(debug)?;
    let mut kernel = kernel(policy)?;
    let principal = kernel.authenticate(&mut AuthAdapter, &()).map_err(debug)?;
    for sequence in 2..=profile.checkpoint_revision() {
        commit_batch(
            &mut live,
            &mut fs,
            &mut kernel,
            &principal,
            batch(profile, sequence)?,
            &mut clock(sequence),
            limits,
        )?;
    }
    let sequence = profile.frontier();
    let encoded = encode_transaction(&GraphTransaction::new(
        scope(),
        profile.batch(scope(), sequence)?,
    ))
    .map_err(debug)?;
    let mut publication = limits.publication;
    publication.stage.maximum_batches = 1;
    let mut writer =
        AuthorizedPackedWriter::new(&mut live, &mut kernel, limits.preparation, publication)
            .map_err(debug)?;
    let outcome = match writer.commit(
        &mut fs,
        &principal,
        AuthorizedTransactionRequest {
            idempotency_key: identity(sequence, IdempotencyKey::from_bytes),
            transaction_id: identity(sequence, TransactionId::from_bytes),
            canonical_request: &encoded,
            blob_inventory: None,
        },
        &mut clock(sequence),
        &NeverCancel,
    ) {
        Err(uste_txn::AuthorizedDiskWriteError::CommittedPublication {
            outcome,
            error:
                uste_txn::TransactionError::Storage(uste_storage::journal::StorageError::ResourceLimit),
        }) => outcome,
        _ => return Err("BM-06 expected certified packed publication refusal".into()),
    };
    if outcome.revision.get() != sequence {
        return Err("BM-06 packed tail revision mismatch".into());
    }
    drop(writer);
    drop(live);
    fs.restart().map_err(debug)?;
    let recovery = open(&mut fs, &name, limits, 83_000_000)?;
    let (mut live, digest, groups) = admit_at(
        &mut fs,
        recovery,
        limits,
        Some(uste_types::CommitRevision::new(profile.checkpoint_revision()).map_err(debug)?),
        [
            profile.records(),
            profile.records() * (VERSIONS - 1),
            0,
            0,
            0,
            0,
            1,
            1,
        ],
    )?;
    if digest.is_some() || groups != profile.frontier() - profile.checkpoint_revision() {
        return Err("BM-06 packed checkpoint/suffix mismatch".into());
    }
    let retried = commit_batch(
        &mut live,
        &mut fs,
        &mut kernel,
        &principal,
        batch(profile, sequence)?,
        &mut clock(sequence),
        limits,
    )?
    .0;
    if retried != outcome {
        return Err("BM-06 packed retry changed outcome".into());
    }
    drop(live);
    fs.restart().map_err(debug)?;
    let recovery = open(&mut fs, &name, limits, 84_000_000)?;
    let (live, v1_state_digest) = admit(&mut fs, recovery, limits)?;
    let verified_versions = verify_history(&live, &mut fs, &kernel, &principal, profile, VERSIONS)?;
    drop(live);
    let recovery = open(&mut fs, &name, limits, 85_000_000)?;
    let (mut live, origin) = recover_packed_graph_origin(
        recovery,
        &mut fs,
        RetentionDays::new(30).map_err(debug)?,
        CoordinatorRecoveryLimits::new(0, 0).map_err(debug)?,
        limits.origin,
    )
    .map_err(debug)?;
    if origin.suffix.journal.groups != sequence - 1
        || live.overlay_counts() != (0, 0)
        || verify_history(&live, &mut fs, &kernel, &principal, profile, VERSIONS)?
            != verified_versions
    {
        return Err("BM-06 packed origin mismatch".into());
    }
    // Stream every original batch once more at the terminal clock, retaining no outcome map.
    // Old versions must be exact retries, not attempts to overwrite the current generation.
    for retry in 2..=sequence {
        commit_batch(
            &mut live,
            &mut fs,
            &mut kernel,
            &principal,
            batch(profile, retry)?,
            &mut clock(sequence),
            limits,
        )?;
        if live.overlay_counts() != (0, 0) {
            return Err("BM-06 packed origin retry created an overlay".into());
        }
    }
    drop(live);
    let recovery = open(&mut fs, &name, limits, 86_000_000)?;
    if admit(&mut fs, recovery, limits)?.1 != v1_state_digest {
        return Err("BM-06 packed origin digest mismatch".into());
    }
    Ok(PackedRecoveryVerification {
        records: profile.records(),
        verified_versions,
        verified_payload_bytes: verified_versions * PAYLOAD_BYTES as u64,
        base_revision: profile.checkpoint_revision(),
        recovered_revision: sequence,
        origin_suffix_groups: origin.suffix.journal.groups,
        v1_state_digest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    mod prefix;
    #[test]
    fn packed_bm06_preserves_every_version_after_suffix_and_origin_recovery() {
        for records in [1, 2] {
            let report = verify(Bm06Profile::new(records).unwrap()).unwrap();
            assert_eq!(
                (
                    report.base_revision,
                    report.recovered_revision,
                    report.origin_suffix_groups
                ),
                (100, 101, 100)
            );
            assert_eq!(
                (report.verified_versions, report.verified_payload_bytes),
                (100 * records, 409600 * records)
            );
        }
    }
    #[test]
    fn packed_bm06_model_cap_precedes_allocation() {
        for records in [3, 100000] {
            assert!(
                verify(Bm06Profile::new(records).unwrap())
                    .unwrap_err()
                    .contains("at most 2")
            );
        }
    }
}
