//! Opt-in packed engine/oracle equivalence; never a performance qualification campaign.
use super::*;
pub(crate) mod limits;
pub(crate) mod prefix;
pub mod recovery;
use limits::Limits;
use uste_graph::{
    GRAPH_PACKED_PROFILE_V1, GraphPackedLiveState, GraphStateDeltaLimits,
    PackedGraphAdmissionLimits, PackedGraphExpansionLimits, PackedGraphGenesisLimits,
    PackedGraphOriginRecoveryLimits, PackedGraphPreparationLimits, PackedGraphReadLimits,
    PackedGraphStageLimits, PackedGraphSuffixRecoveryLimits, PackedGraphWritePreparationLimits,
    PackedGraphWritePublicationLimits, admit_packed_graph_base_buffered,
    recover_packed_graph_origin, recover_packed_graph_suffix,
};
use uste_storage::{
    PageCache,
    journal::{CertifiedPackedRoot, PackedRootDiscoveryLimits},
    packed_page_cache::PackedCacheReport,
    packed_tree_cursor::TreeCursorLimits,
    packed_tree_lookup::TreeLookupLimits,
    packed_tree_validation::TreeValidationLimits,
};
use uste_txn::{
    AuthenticatedIndexRecovery, AuthorizedPackedReader, AuthorizedPackedWriter,
    COORDINATOR_PACKED_PROFILE_V1, COORDINATOR_PACKED_USAGE_PROFILE_V1, CoordinatorRecoveryLimits,
    PackedCommitCoordinator, PackedCoordinatorAdmissionLimits, PackedCoordinatorLimits,
    PackedMetadataRebaseLimits, PackedQuotaAdmissionLimits, admit_packed_coordinator_prefix,
    admit_packed_quota_prefix,
};
pub(crate) type RecoveryEngine<F, W, E, I> = AuthenticatedIndexRecovery<F, W, E, I>;
pub(crate) type PackedEngine<F, W, E, I> =
    PackedCommitCoordinator<GraphPackedLiveState, F, W, E, I>;
type Recovery = RecoveryEngine<MemoryFileSystem, TestEnvelope, CounterEntropy, CounterEntropy>;
type Packed = PackedEngine<MemoryFileSystem, TestEnvelope, CounterEntropy, CounterEntropy>;

// Sequential operation-local caches, not retained by the coordinator or query reader.
pub(crate) const GRAPH_ADMISSION_CACHE_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const GRAPH_ADMISSION_CACHE_SCOPE: &str =
    "fresh-per-canonical-family-then-fresh-semantic";

#[derive(Debug)]
pub struct PackedDevelopmentVerification {
    pub entities: u64,
    pub relationships: u64,
    pub recovered_revision: u64,
    pub queries: usize,
    pub output_digest: [u8; 32],
    pub v1_state_digest: [u8; 32],
    pub cache_report: PackedCacheReport,
    pub origin_suffix_groups: u64,
}
fn open(
    fs: &mut MemoryFileSystem,
    name: &EntryName,
    limits: Limits,
    entropy: u64,
) -> Result<Recovery, String> {
    fs.restart().map_err(debug)?;
    let (recovery, _, _) = AuthenticatedIndexRecovery::open_with_disk_blob_metadata(
        fs,
        name,
        scope(),
        CounterEntropy(entropy),
        CounterEntropy(entropy + 100_000),
        &mut TestKeyAdapter,
        limits.legacy.blob_recovery,
        &mut PageCache::new(64 * 1024 * 1024).map_err(debug)?,
    )
    .map_err(debug)?;
    Ok(recovery)
}
fn root<F: OwnershipFileSystem, W: DurableKeyEnvelope, E: EntropySource, I: EntropySource>(
    fs: &mut F,
    recovery: &RecoveryEngine<F, W, E, I>,
    revision: uste_types::CommitRevision,
    profile: [u8; 32],
    limits: Limits,
) -> Result<CertifiedPackedRoot, String> {
    optional_root(fs, recovery, revision, profile, limits)?
        .ok_or("missing terminal packed root; origin rebuild must be explicit".into())
}
fn optional_root<
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
>(
    fs: &mut F,
    recovery: &RecoveryEngine<F, W, E, I>,
    revision: uste_types::CommitRevision,
    profile: [u8; 32],
    limits: Limits,
) -> Result<Option<CertifiedPackedRoot>, String> {
    Ok(recovery
        .discover_packed_roots_at_revision(
            fs,
            profile,
            revision,
            limits.origin.suffix.graph.certificates,
            PackedRootDiscoveryLimits::new(8, 8 * 4177).map_err(debug)?,
        )
        .map_err(debug)?
        .0
        .into_iter()
        .max_by_key(|root| root.manifest().claims().generation))
}
#[allow(clippy::type_complexity)]
pub(crate) fn admit<
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
>(
    fs: &mut F,
    recovery: RecoveryEngine<F, W, E, I>,
    limits: Limits,
) -> Result<(PackedEngine<F, W, E, I>, [u8; 32]), String> {
    let (live, digest, groups) = admit_at(fs, recovery, limits, None, limits.counts)?;
    if groups != 0 {
        return Err("terminal open unexpectedly replayed history".into());
    }
    Ok((live, digest.ok_or("missing terminal reference digest")?))
}

// The digest belongs to the admitted base. Never expose it as the resulting state's digest
// when a suffix was replayed; obtaining that digest requires a fresh terminal admission.
#[allow(clippy::type_complexity)]
pub(crate) fn admit_at<
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
>(
    fs: &mut F,
    mut recovery: RecoveryEngine<F, W, E, I>,
    limits: Limits,
    selected: Option<uste_types::CommitRevision>,
    counts: [u64; 8],
) -> Result<(PackedEngine<F, W, E, I>, Option<[u8; 32]>, u64), String> {
    let frontier = recovery
        .authenticated_frontier_anchor()
        .ok_or("missing packed frontier")?
        .0;
    let revision = selected.unwrap_or(frontier);
    if frontier.get() > limits.legacy.groups || revision > frontier {
        return Err("packed frontier exceeds fixture bound".into());
    }
    let graph_root = root(fs, &recovery, revision, GRAPH_PACKED_PROFILE_V1, limits)?;
    let primary_root = root(
        fs,
        &recovery,
        revision,
        COORDINATOR_PACKED_PROFILE_V1,
        limits,
    )?;
    let quota_root = root(
        fs,
        &recovery,
        revision,
        COORDINATOR_PACKED_USAGE_PROFILE_V1,
        limits,
    )?;
    let mut cursor = recovery
        .open_transaction_cursor(revision, revision, 1, limits.legacy.prefix_bytes)
        .map_err(debug)?;
    let receipt = recovery
        .next_recovered_transaction(fs, &mut cursor)
        .map_err(debug)?
        .ok_or("missing terminal receipt")?;
    if recovery
        .next_recovered_transaction(fs, &mut cursor)
        .map_err(debug)?
        .is_some()
    {
        return Err("unexpected terminal suffix".into());
    }
    recovery.finish_transaction_cursor(cursor).map_err(debug)?;
    let maintenance = recovery
        .packed_indexes_with_io(fs, &receipt, limits.origin.suffix.graph.certificates)
        .map_err(debug)?;
    let (base, _, _) = admit_packed_graph_base_buffered(
        &maintenance,
        fs,
        &graph_root,
        limits.graph,
        GRAPH_ADMISSION_CACHE_BYTES,
    )
    .map_err(debug)?;
    let c = counts;
    if base.families().map(|family| family.commitment.entries())
        != [1, c[0], c[1], c[2], c[3], c[4], c[5], c[6] + c[7]]
    {
        return Err("packed admitted fixture cardinality mismatch".into());
    }
    let digest = *base
        .source_v1_digest()
        .ok_or("missing cold reference digest")?;
    let (primary, _) = admit_packed_coordinator_prefix(
        &mut recovery,
        fs,
        &primary_root,
        PackedCoordinatorAdmissionLimits {
            certificates: limits.origin.suffix.graph.certificates,
            family: limits.family,
            lookup: limits.lookup,
            maximum_groups: limits.legacy.groups,
            maximum_journal_bytes: limits.legacy.prefix_bytes,
            maximum_references: 0,
            maximum_owners: 0,
            maximum_lookup_pages: limits.legacy.groups * 4 * 516,
            maximum_lookup_bytes: limits.legacy.groups * 4 * 516 * 20545,
        },
    )
    .map_err(debug)?;
    let (quota, _) = admit_packed_quota_prefix(
        &mut recovery,
        fs,
        &primary,
        &quota_root,
        PackedQuotaAdmissionLimits {
            certificates: limits.origin.suffix.graph.certificates,
            family: limits.family,
            cursor: limits.cursor,
            lookup: limits.lookup,
            maximum_owners: 0,
            maximum_lookup_pages: 516,
            maximum_lookup_bytes: 516 * 20545,
        },
    )
    .map_err(debug)?;
    let (live, suffix) = recover_packed_graph_suffix(
        recovery,
        fs,
        base,
        graph_root,
        primary,
        quota,
        &primary_root,
        &quota_root,
        RetentionDays::new(30).map_err(debug)?,
        CoordinatorRecoveryLimits::new(1, 0).map_err(debug)?,
        limits.origin.suffix,
    )
    .map_err(debug)?;
    let groups = suffix.map_or(0, |report| report.journal.groups);
    if groups != frontier.get() - revision.get() || live.overlay_counts() != (0, 0) {
        return Err("packed prefix recovery frontier/overlay mismatch".into());
    }
    Ok((live, (groups == 0).then_some(digest), groups))
}
pub(crate) fn commit_batch<
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
>(
    live: &mut PackedEngine<F, W, E, I>,
    fs: &mut F,
    kernel: &mut PolicyKernel,
    principal: &AuthenticatedPrincipal,
    batch: disk::DiskBatch,
    clock: &mut impl uste_storage::Clock,
    limits: Limits,
) -> Result<(uste_txn::TransactionOutcome, Vec<u8>), String> {
    commit_batch_observed(
        live,
        fs,
        kernel,
        principal,
        batch,
        clock,
        limits,
        &mut |_| Ok(()),
    )
}

// Fixture-only observation point: graph publication has succeeded, metadata rebase has not.
#[allow(clippy::too_many_arguments)]
pub(crate) fn commit_batch_observed<
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
>(
    live: &mut PackedEngine<F, W, E, I>,
    fs: &mut F,
    kernel: &mut PolicyKernel,
    principal: &AuthenticatedPrincipal,
    batch: disk::DiskBatch,
    clock: &mut impl uste_storage::Clock,
    limits: Limits,
    observer: &mut impl FnMut(u64) -> Result<(), String>,
) -> Result<(uste_txn::TransactionOutcome, Vec<u8>), String> {
    let bytes =
        encode_transaction(&GraphTransaction::new(scope(), batch.operations)).map_err(debug)?;
    let mut writer =
        AuthorizedPackedWriter::new(live, kernel, limits.preparation, limits.publication)
            .map_err(debug)?;
    let outcome = writer
        .commit(
            fs,
            principal,
            AuthorizedTransactionRequest {
                idempotency_key: batch.idempotency_key,
                transaction_id: batch.transaction_id,
                canonical_request: &bytes,
                blob_inventory: None,
            },
            clock,
            &NeverCancel,
        )
        .map_err(debug)?;
    if outcome.revision.get() != batch.sequence {
        return Err("packed write frontier mismatch".into());
    }
    drop(writer);
    observer(outcome.revision.get())?;
    live.rebase_metadata(fs, limits.origin.suffix.metadata)
        .map_err(debug)?;
    Ok((outcome, bytes))
}

fn verify_queries(
    fs: &mut MemoryFileSystem,
    live: &Packed,
    policy: &PolicyKernel,
    principal: &AuthenticatedPrincipal,
    profile: Bm01Profile,
    limits: Limits,
) -> Result<([u8; 32], PackedCacheReport), String> {
    let reader = AuthorizedPackedReader::new_with_cache_budget(
        live,
        policy,
        limits.read()?,
        64 * 1024 * 1024,
    )
    .map_err(debug)?;
    let oracle = Oracle::build(profile).map_err(debug)?;
    let materializer = Materializer::new(profile);
    let mut outputs = blake3::Hasher::new_derive_key("USTE BM-01 engine-equivalence-v1");
    for query in measured_queries(profile) {
        let expected = oracle
            .expand(query, OracleLimits::default())
            .map_err(debug)?;
        let actual = execute_query_with(materializer, query, |request| {
            reader
                .read(fs, principal, request, &NeverCancel)
                .map_err(|error| EngineQueryError::Engine(debug(error)))
        })
        .map_err(|error| error.to_string())?;
        if actual != expected {
            return Err(format!("packed engine/oracle mismatch for query {query:?}"));
        }
        outputs.update(&[query.class.code(), query.direction.code(), query.depth]);
        outputs.update(&query.ordinal.to_be_bytes());
        outputs.update(&actual.digest());
    }
    Ok((
        *outputs.finalize().as_bytes(),
        reader
            .cache_report(principal)
            .map_err(debug)?
            .ok_or("missing packed cache")?,
    ))
}

fn exact_retry(
    fs: &mut MemoryFileSystem,
    live: &mut Packed,
    kernel: &mut PolicyKernel,
    principal: &AuthenticatedPrincipal,
    bytes: &[u8],
    expected: uste_txn::TransactionOutcome,
    limits: Limits,
) -> Result<(), String> {
    let sequence = expected.revision.get();
    let mut writer =
        AuthorizedPackedWriter::new(live, kernel, limits.preparation, limits.publication)
            .map_err(debug)?;
    let actual = writer
        .commit(
            fs,
            principal,
            AuthorizedTransactionRequest {
                idempotency_key: identity(sequence, IdempotencyKey::from_bytes),
                transaction_id: identity(sequence, TransactionId::from_bytes),
                canonical_request: bytes,
                blob_inventory: None,
            },
            &mut clock(sequence),
            &NeverCancel,
        )
        .map_err(debug)?;
    if actual != expected {
        return Err("packed cold/origin exact retry mismatch".into());
    }
    drop(writer);
    if live.overlay_counts() != (0, 0)
        || live
            .rebase_metadata(fs, limits.origin.suffix.metadata)
            .map_err(debug)?
            .is_some()
    {
        return Err("packed retry changed durable metadata".into());
    }
    Ok(())
}

/// Test-model filesystem and credential wrapper, with the same pre-I/O cap as the v1 verifier.
pub fn verify_packed_development_profile(
    profile: Bm01Profile,
) -> Result<PackedDevelopmentVerification, String> {
    if profile.entities() > MAX_DEVELOPMENT_ENTITIES {
        return Err(format!(
            "packed development verifier accepts at most {MAX_DEVELOPMENT_ENTITIES} entities"
        ));
    }
    let limits = Limits::new(profile)?;
    let policy = benchmark_policy(scope())?;
    let name = EntryName::new("bm01-packed-development").map_err(debug)?;
    let mut fs = MemoryFileSystem::default();
    let vault = KeyVault::create(
        scope().database(),
        &mut TestKeyAdapter,
        CounterEntropy(30_000_001),
    )
    .map_err(debug)?;
    let mut raw = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).map_err(debug)?,
        name.clone(),
        vault,
        CounterEntropy(30_100_000),
        GraphState::new(scope()),
    )
    .map_err(debug)?;
    let bytes = encode_transaction(&GraphTransaction::with_policy_mutation(
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
            canonical_request: &bytes,
            blob_inventory: None,
        },
        &mut clock(1),
        &NeverCancel,
    )
    .map_err(debug)?;
    drop(raw);
    let recovery = open(&mut fs, &name, limits, 31_000_000)?;
    let (mut live, report) = recover_packed_graph_origin(
        recovery,
        &mut fs,
        RetentionDays::new(30).map_err(debug)?,
        CoordinatorRecoveryLimits::new(1, 0).map_err(debug)?,
        limits.origin,
    )
    .map_err(debug)?;
    if report.suffix.journal.groups != 0 {
        return Err("bootstrap unexpectedly replayed graph history".into());
    }
    let mut kernel = kernel(policy)?;
    let principal = kernel.authenticate(&mut AuthAdapter, &()).map_err(debug)?;
    let mut last = None;
    let revision = disk::visit_disk_batches(profile, |sequence, operations| {
        let (outcome, bytes) = commit_batch(
            &mut live,
            &mut fs,
            &mut kernel,
            &principal,
            disk::DiskBatch {
                sequence,
                idempotency_key: identity(sequence, IdempotencyKey::from_bytes),
                transaction_id: identity(sequence, TransactionId::from_bytes),
                operations,
            },
            &mut clock(sequence),
            limits,
        )?;
        // One bounded request, never an outcome/request map for the fixture history.
        last = Some((bytes, outcome));
        Ok(())
    })?;
    let (last_bytes, last_outcome) = last.ok_or("missing packed final request")?;
    drop(live);
    let recovery = open(&mut fs, &name, limits, 32_000_000)?;
    let (mut live, v1_state_digest) = admit(&mut fs, recovery, limits)?;
    if live.state().map_err(debug)?.revision().get() != revision {
        return Err("packed cold frontier mismatch".into());
    }
    exact_retry(
        &mut fs,
        &mut live,
        &mut kernel,
        &principal,
        &last_bytes,
        last_outcome,
        limits,
    )?;
    let (output_digest, cache_report) =
        verify_queries(&mut fs, &live, &kernel, &principal, profile, limits)?;
    drop(live);
    let recovery = open(&mut fs, &name, limits, 33_000_000)?;
    let (mut live, origin_report) = recover_packed_graph_origin(
        recovery,
        &mut fs,
        RetentionDays::new(30).map_err(debug)?,
        CoordinatorRecoveryLimits::new(0, 0).map_err(debug)?,
        limits.origin,
    )
    .map_err(debug)?;
    if live.overlay_counts() != (0, 0) || origin_report.suffix.journal.groups != revision - 1 {
        return Err("packed origin suffix mismatch".into());
    }
    exact_retry(
        &mut fs,
        &mut live,
        &mut kernel,
        &principal,
        &last_bytes,
        last_outcome,
        limits,
    )?;
    let (rebuilt, _) = verify_queries(&mut fs, &live, &kernel, &principal, profile, limits)?;
    if rebuilt != output_digest {
        return Err("packed origin query digest mismatch".into());
    }
    drop(live);
    let recovery = open(&mut fs, &name, limits, 34_000_000)?;
    let (_, rebuilt) = admit(&mut fs, recovery, limits)?;
    if rebuilt != v1_state_digest {
        return Err("packed origin logical state mismatch".into());
    }
    Ok(PackedDevelopmentVerification {
        entities: profile.entities(),
        relationships: profile.relationships(),
        recovered_revision: revision,
        queries: measured_queries(profile).len(),
        output_digest,
        v1_state_digest,
        cache_report,
        origin_suffix_groups: origin_report.suffix.journal.groups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packed_engine_cold_and_origin_rebuild_match_independent_oracle() {
        let profile = Bm01Profile::new(20).unwrap();
        let report = verify_packed_development_profile(profile).unwrap();
        assert_eq!(report.queries, 384);
        assert_eq!(
            report.output_digest,
            *blake3::Hash::from_hex(
                "46f1bdb3138d6325e4c0f56b5fd3bbf5ff092d816e8a0f6c23acd15687b910b5"
            )
            .unwrap()
            .as_bytes()
        );
        assert_eq!(
            report.recovered_revision,
            materialization_revision_count(profile)
        );
        assert_eq!(report.origin_suffix_groups, report.recovered_revision - 1);
        assert!(report.cache_report.hits > 0);
        assert!(report.cache_report.misses > 0);
        assert!(report.cache_report.accounted_bytes <= report.cache_report.budget_bytes);
        assert_eq!(
            report.output_digest,
            verify_development_profile(profile).unwrap().output_digest
        );
    }
    #[test]
    fn packed_engine_retains_the_pre_io_development_cap() {
        assert!(
            verify_packed_development_profile(Bm01Profile::qualifying())
                .unwrap_err()
                .contains("at most 1000")
        );
    }
}
