//! Development oracle check using disk-backed graph and coordinator state after policy bootstrap.
use super::*;
use uste_graph::{
    GraphDiskBaseAdmissionLimits, GraphDiskExpansionLimits, GraphDiskLiveState,
    GraphDiskPreparationLimits, GraphDiskReadLimits, GraphDiskWritePreparationLimits,
    GraphStateDeltaLimits, GraphStateLoadLimits, GraphStateRootMergeLimits,
};
use uste_storage::{
    IndexGetLimits, IndexPredecessorLimits, IndexRunMergeLimits, IndexRunReadLimits, PageCache,
};
use uste_txn::{
    AuthenticatedIndexRecovery, AuthorizedDiskReader, AuthorizedDiskWriter,
    CoordinatorMetadataRebaseLimits, DiskCommitCoordinator,
};

type Disk = DiskCommitCoordinator<
    GraphDiskLiveState,
    MemoryFileSystem,
    TestEnvelope,
    CounterEntropy,
    CounterEntropy,
>;

/// Privileged setup measurements, never consumer query results or complete I/O accounting.
pub(crate) struct DiskAdmissionMeasurement {
    pub graph_revision: u64,
    pub metadata_revision: u64,
    pub state_counts: [u64; 8],
    pub graph: uste_graph::GraphDiskBaseAdmissionReport,
}

type AdmittedDisk<F, W, E, I> = (
    DiskCommitCoordinator<GraphDiskLiveState, F, W, E, I>,
    DiskAdmissionMeasurement,
);

/// Exact final state cardinalities for this fixture, not general graph admission ceilings.
pub(crate) fn fixture_state_counts(profile: Bm01Profile) -> [u64; 8] {
    let entities = profile.entities();
    let relationships = profile.relationships();
    // Bm01Profile validates entities <= 100,000 and relationships = 10 * entities.
    [
        entities + relationships + 1,
        entities + 2 * relationships + 1,
        relationships,
        relationships,
        relationships,
        3 * relationships,
        1,
        1,
    ]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiskDevelopmentVerification {
    pub entities: u64,
    pub relationships: u64,
    pub recovered_revision: u64,
    pub queries: usize,
    pub output_digest: [u8; 32],
    pub cache_report: uste_txn::AuthorizedDiskCacheReport,
}

/// Nonqualifying memory-adapter correctness check. The independent oracle is deliberately
/// separate from the engine's disk state; neither this test adapter nor its timings qualify BM-01.
pub fn verify_disk_development_profile(
    profile: Bm01Profile,
) -> Result<DiskDevelopmentVerification, String> {
    if profile.entities() > MAX_DEVELOPMENT_ENTITIES {
        return Err(format!(
            "disk development verifier accepts at most {MAX_DEVELOPMENT_ENTITIES} entities"
        ));
    }
    let policy = benchmark_policy(scope())?;
    let install = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        Vec::new(),
        uste_graph::DurablePolicyMutation::Install {
            policy: policy.clone(),
        },
    ))
    .map_err(debug)?;
    let mut fs = MemoryFileSystem::default();
    let name = EntryName::new("bm01-disk-development").map_err(debug)?;
    let vault = KeyVault::create(scope().database(), &mut TestKeyAdapter, CounterEntropy(1))
        .map_err(debug)?;
    let mut bootstrap = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).map_err(debug)?,
        name.clone(),
        vault,
        CounterEntropy(100),
        GraphState::new(scope()),
    )
    .map_err(debug)?;
    bootstrap
        .commit(
            &mut fs,
            TransactionRequest {
                principal: PRINCIPAL,
                idempotency_key: identity(1, IdempotencyKey::from_bytes),
                transaction_id: identity(1, TransactionId::from_bytes),
                canonical_request: &install,
                blob_inventory: None,
            },
            &mut clock(1),
            &NeverCancel,
        )
        .map_err(debug)?;
    // Exercise restart before any derived roots exist. Only this one-revision, owner-free
    // bootstrap may use full replay; a larger prefix must never fall back to GraphState.
    drop(bootstrap);
    fs.restart().map_err(debug)?;
    let (recovery, _) = AuthenticatedIndexRecovery::open(
        &mut fs,
        &name,
        scope(),
        CounterEntropy(200),
        CounterEntropy(300),
        &mut TestKeyAdapter,
    )
    .map_err(debug)?;
    let mut bootstrap = recovery
        .into_bounded_coordinator(
            &mut fs,
            GraphState::new(scope()),
            RetentionDays::new(30).map_err(debug)?,
            uste_txn::CoordinatorRecoveryLimits::new(1, 0).map_err(debug)?,
            1_048_576,
        )
        .map_err(debug)?;
    // The only full graph snapshot contains policy, before any fixture records are admitted.
    let policy_snapshot = bootstrap.read_view().map_err(debug)?.state().clone();
    uste_graph::publish_graph_state_root(&mut bootstrap, &mut fs, &policy_snapshot)
        .map_err(debug)?;
    uste_txn::publish_coordinator_metadata_root(&mut bootstrap, &mut fs).map_err(debug)?;
    uste_txn::publish_coordinator_transaction_index(&mut bootstrap, &mut fs).map_err(debug)?;
    drop(policy_snapshot);
    drop(bootstrap);
    fs.restart().map_err(debug)?;
    let mut disk = open_disk(&mut fs, &name)?;
    let mut policy_kernel = kernel(policy.clone())?;
    let principal = policy_kernel
        .authenticate(&mut AuthAdapter, &())
        .map_err(debug)?;
    let materializer = Materializer::new(profile);
    let expected_revision = visit_development_batches(profile, |sequence, operations| {
        commit_batch(
            &mut disk,
            &mut fs,
            &mut policy_kernel,
            &principal,
            DiskBatchIdentity {
                sequence,
                idempotency_key: identity(sequence, IdempotencyKey::from_bytes),
                transaction_id: identity(sequence, TransactionId::from_bytes),
            },
            operations,
            &mut clock(sequence),
        )
    })?;
    drop(disk);
    fs.restart().map_err(debug)?;
    let disk = open_disk(&mut fs, &name)?;
    if disk
        .state()
        .map_err(debug)?
        .current_base()
        .ok_or("pending final base")?
        .state_counts()
        != fixture_state_counts(profile)
    {
        return Err("disk fixture cardinality mismatch".into());
    }
    let recovered_revision = disk
        .checkpoint_anchor()
        .map_err(debug)?
        .ok_or("missing disk frontier")?
        .0
        .get();
    if recovered_revision != expected_revision {
        return Err("disk frontier mismatch".into());
    }
    if disk.overlay_counts() != (0, 0) {
        return Err("disk reopen unexpectedly retained coordinator overlays".into());
    }
    let reader = AuthorizedDiskReader::new_with_cache_budget(
        &disk,
        &policy_kernel,
        GraphDiskReadLimits {
            current: IndexGetLimits::new(64, 16 * 1024).map_err(debug)?,
            historical: IndexPredecessorLimits::new(64, 16 * 1024).map_err(debug)?,
            expansion: Some(
                GraphDiskExpansionLimits::new(1_000_000, 1_000_000, 64 * 1024 * 1024, 2_000_000)
                    .map_err(debug)?,
            ),
        },
        64 * 1024 * 1024,
    )
    .map_err(debug)?;
    let oracle = Oracle::build(profile).map_err(debug)?;
    let queries = measured_queries(profile);
    let mut outputs = blake3::Hasher::new_derive_key("USTE BM-01 engine-equivalence-v1");
    for query in &queries {
        let expected = oracle
            .expand(*query, OracleLimits::default())
            .map_err(debug)?;
        let actual = execute_query_with(materializer, *query, |request| {
            reader
                .read(&mut fs, &principal, request, &NeverCancel)
                .map_err(|error| EngineQueryError::Engine(debug(error)))
        })
        .map_err(|error| error.to_string())?;
        if actual != expected {
            return Err(format!("disk engine/oracle mismatch for query {query:?}"));
        }
        outputs.update(&[query.class.code(), query.direction.code(), query.depth]);
        outputs.update(&query.ordinal.to_be_bytes());
        outputs.update(&actual.digest());
    }
    Ok(DiskDevelopmentVerification {
        entities: profile.entities(),
        relationships: profile.relationships(),
        recovered_revision,
        queries: queries.len(),
        output_digest: *outputs.finalize().as_bytes(),
        cache_report: reader.cache_report(&principal).map_err(debug)?,
    })
}

fn open_disk(fs: &mut MemoryFileSystem, name: &EntryName) -> Result<Disk, String> {
    static ENTROPY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1_000_000);
    let entropy = ENTROPY.fetch_add(1_000_000, std::sync::atomic::Ordering::Relaxed);
    let (recovery, _, frontier) = AuthenticatedIndexRecovery::open_with_frontier_transaction(
        fs,
        name,
        scope(),
        CounterEntropy(entropy),
        CounterEntropy(entropy + 500_000),
        &mut TestKeyAdapter,
    )
    .map_err(debug)?;
    admit_development_disk(fs, recovery, frontier.ok_or("missing disk frontier")?)
        .map(|(disk, _)| disk)
}

/// Shared adapter-independent development recovery. No full graph/coordinator fallback is allowed.
/// The explicit development budgets are not qualification-profile admission.
pub(crate) fn admit_development_disk<F, W, E, I>(
    fs: &mut F,
    recovery: AuthenticatedIndexRecovery<F, W, E, I>,
    frontier: uste_txn::RecoveredFrontierTransaction,
) -> Result<AdmittedDisk<F, W, E, I>, String>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let mut cache = PageCache::new(64 * 1024).map_err(debug)?;
    let lookup = IndexGetLimits::new(64, 136).map_err(debug)?;
    let graph_candidate = uste_graph::load_graph_state_root_candidates_for_recovery(&recovery, fs)
        .map_err(debug)?
        .into_iter()
        .filter(|root| root.revision() <= frontier.revision())
        .max_by_key(|root| root.revision())
        .ok_or("missing graph base")?;
    if frontier.revision().get() - graph_candidate.revision().get() > 1 {
        return Err("disk graph suffix exceeds one revision".into());
    }
    let transaction_roots = recovery
        .load_index_root_manifests(fs, uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1)
        .map_err(debug)?;
    let candidate =
        uste_txn::load_coordinator_metadata_candidates_for_recovery::<GraphState, _, _, _, _>(
            &recovery, fs,
        )
        .map_err(debug)?
        .into_iter()
        .filter(|root| {
            root.revision() <= graph_candidate.revision()
                && transaction_roots
                    .iter()
                    .any(|tx| tx.revision() == root.revision())
        })
        .max_by_key(|root| root.revision())
        .ok_or("missing paired metadata base")?;
    let transaction_root = transaction_roots
        .into_iter()
        .find(|root| root.revision() == candidate.revision())
        .ok_or("missing paired transaction root")?;
    let transactions = uste_txn::admit_coordinator_transaction_index_for_recovery(
        &recovery,
        fs,
        transaction_root,
        uste_txn::CoordinatorTransactionAdmissionLimits {
            run: IndexRunReadLimits::new(1000, 1000, 1024 * 1024).map_err(debug)?,
            lookup,
            maximum_groups: 1000,
            maximum_encoded_bytes: 128 * 1024 * 1024,
        },
        &mut cache,
    )
    .map_err(debug)?;
    let metadata_revision = candidate.revision().get();
    let metadata = uste_txn::admit_coordinator_disk_base(
        &recovery,
        fs,
        candidate,
        transactions,
        uste_txn::CoordinatorDiskAdmissionLimits {
            metadata: uste_txn::CoordinatorMetadataLoadLimits::new(
                1000,
                0,
                1001,
                1000,
                1024 * 1024,
            )
            .map_err(debug)?,
            lookup,
            maximum_total_journal_groups: 1000,
            maximum_encoded_bytes_per_pass: 128 * 1024 * 1024,
        },
        &mut cache,
    )
    .map_err(debug)?;
    let limits = GraphDiskBaseAdmissionLimits::new(
        GraphStateLoadLimits::new(
            100_000,
            200_000,
            1000,
            1_000_000,
            100_000,
            256 * 1024 * 1024,
        )
        .map_err(debug)?,
        4,
        64 * 1024,
        1_000_000,
        1_000_000,
        1_000_000,
        256 * 1024 * 1024,
        IndexPredecessorLimits::new(64, 16 * 1024).map_err(debug)?,
    )
    .map_err(debug)?;
    let (base, graph_report) = uste_graph::admit_graph_disk_base_candidate_for_recovery(
        &recovery,
        fs,
        &graph_candidate,
        limits,
        &mut cache,
    )
    .map_err(debug)?;
    let measurement = DiskAdmissionMeasurement {
        graph_revision: base.revision().get(),
        metadata_revision,
        state_counts: base.state_counts(),
        graph: graph_report,
    };
    let suffix = if base.revision() == frontier.revision() {
        None
    } else {
        let prepared = uste_graph::load_graph_disk_recovery_preparation_view(
            &recovery,
            fs,
            &base,
            &frontier,
            GraphDiskPreparationLimits::new(20_000, 1_000_000, 100_000, 100_000, 32 * 1024 * 1024)
                .map_err(debug)?,
            &mut cache,
        )
        .map_err(debug)?
        .prepare()
        .map_err(debug)?;
        Some(
            frontier.bind_prepared(
                uste_graph::prepare_graph_disk_commit(
                    prepared,
                    GraphStateDeltaLimits::new(1_000_000, 64 * 1024 * 1024).map_err(debug)?,
                )
                .map_err(debug)?,
            ),
        )
    };
    DiskCommitCoordinator::recover_with_prepared_suffix(
        recovery,
        fs,
        metadata,
        GraphDiskLiveState::new(base),
        suffix,
        RetentionDays::new(30).map_err(debug)?,
        uste_txn::DiskCoordinatorRecoveryLimits {
            overlay: uste_txn::CoordinatorRecoveryLimits::new(2, 0).map_err(debug)?,
            lookup,
            maximum_encoded_bytes: 128 * 1024 * 1024,
        },
        &mut cache,
    )
    .map(|disk| (disk, measurement))
    .map_err(debug)
}

/// Stream the unchanged fixture plan in bounded batches to either disk adapter.
pub(crate) fn visit_development_batches(
    profile: Bm01Profile,
    mut visitor: impl FnMut(u64, Vec<Operation>) -> Result<(), String>,
) -> Result<u64, String> {
    if profile.entities() > MAX_DEVELOPMENT_ENTITIES {
        return Err("development disk profile exceeded".into());
    }
    let materializer = Materializer::new(profile);
    let mut sequence = 2;
    emit_batches(
        &mut sequence,
        &mut visitor,
        core::iter::once(Ok(Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Evidence(NewEvidence {
                id: evidence_ref(scope()),
                digest: engine_mapping_digest(profile),
                locator: text("bm01-uste-graph-v1")?,
            }),
        }))
        .chain((0..profile.entities()).map(|ordinal| {
            Ok(Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: entity_ref(scope(), materializer, ordinal),
                    entity_type: text("bm01-entity-v1")?,
                    schema_version: 1,
                    properties: Value::Null,
                }),
            })
        })),
    )?;
    emit_batches(
        &mut sequence,
        &mut visitor,
        (0..profile.relationships()).map(|ordinal| {
            let edge = materializer.edge(ordinal);
            Ok(Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Relationship(NewRelationship {
                    id: relationship_ref(scope(), materializer, ordinal),
                    from: entity_ref(scope(), materializer, edge.source),
                    to: entity_ref(scope(), materializer, edge.destination),
                    relationship_type: text(match edge.topology {
                        crate::Topology::Uniform => "bm01-uniform-v1",
                        crate::Topology::DistributedHub => "bm01-hub-v1",
                        crate::Topology::RingCycle => "bm01-ring-v1",
                    })?,
                    properties: Value::Null,
                    evidence: vec![evidence_ref(scope())],
                    valid_time: ValidTime::Unknown,
                }),
            })
        }),
    )?;
    emit_batches(
        &mut sequence,
        &mut visitor,
        (0..profile.relationships()).map(|ordinal| {
            Ok(Operation::ActOnRelationship {
                target: relationship_ref(scope(), materializer, ordinal),
                expected: Expected::Version(RecordVersion::FIRST),
                action: AssertionAction::Accept,
                correction: None,
                correction_expected: None,
            })
        }),
    )?;
    let frontier = sequence - 1;
    if frontier != materialization_revision_count(profile) {
        return Err("disk materialization plan mismatch".into());
    }
    Ok(frontier)
}

fn emit_batches(
    sequence: &mut u64,
    visitor: &mut impl FnMut(u64, Vec<Operation>) -> Result<(), String>,
    operations: impl IntoIterator<Item = Result<Operation, String>>,
) -> Result<(), String> {
    let mut batch = Vec::with_capacity(MAX_TRANSACTION_OPERATIONS);
    for operation in operations {
        batch.push(operation?);
        if batch.len() == MAX_TRANSACTION_OPERATIONS {
            visitor(*sequence, core::mem::take(&mut batch))?;
            *sequence += 1;
        }
    }
    if !batch.is_empty() {
        visitor(*sequence, batch)?;
        *sequence += 1;
    }
    Ok(())
}

pub(crate) struct DiskBatchIdentity {
    pub sequence: u64,
    pub idempotency_key: IdempotencyKey,
    pub transaction_id: TransactionId,
}

pub(crate) fn commit_batch<F, W, E, I>(
    disk: &mut DiskCommitCoordinator<GraphDiskLiveState, F, W, E, I>,
    fs: &mut F,
    kernel: &mut PolicyKernel,
    principal: &AuthenticatedPrincipal,
    identity: DiskBatchIdentity,
    operations: Vec<Operation>,
    clock: &mut impl uste_storage::Clock,
) -> Result<(), String>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    let encoded = encode_transaction(&GraphTransaction::new(scope(), operations)).map_err(debug)?;
    let (read, merge) = development_merge_limits()?;
    let mut writer = AuthorizedDiskWriter::new_with_cache_budget(
        disk,
        kernel,
        GraphDiskWritePreparationLimits {
            proof: GraphDiskPreparationLimits::new(
                20_000,
                1_000_000,
                100_000,
                100_000,
                32 * 1024 * 1024,
            )
            .map_err(debug)?,
            delta: GraphStateDeltaLimits::new(1_000_000, 64 * 1024 * 1024).map_err(debug)?,
        },
        GraphStateRootMergeLimits::uniform(merge, 64 * 1024).map_err(debug)?,
        64 * 1024 * 1024,
    )
    .map_err(debug)?;
    let outcome = writer
        .commit(
            fs,
            principal,
            AuthorizedTransactionRequest {
                idempotency_key: identity.idempotency_key,
                transaction_id: identity.transaction_id,
                canonical_request: &encoded,
                blob_inventory: None,
            },
            clock,
            &NeverCancel,
        )
        .map_err(debug)?;
    if outcome.revision.get() != identity.sequence {
        return Err("disk commit revision mismatch".into());
    }
    drop(writer);
    disk.rebase_metadata(fs, CoordinatorMetadataRebaseLimits { merge, reuse: read })
        .map_err(debug)
}

pub(crate) fn development_merge_limits() -> Result<(IndexRunReadLimits, IndexRunMergeLimits), String>
{
    let read = IndexRunReadLimits::new(100_000, 1_000_000, 256 * 1024 * 1024).map_err(debug)?;
    let merge = IndexRunMergeLimits::new(
        read,
        1_000_000,
        64 * 1024 * 1024,
        1_000_000,
        256 * 1024 * 1024,
    )
    .map_err(debug)?;
    Ok((read, merge))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_cardinalities_include_policy_and_both_relationship_versions() {
        assert_eq!(
            fixture_state_counts(Bm01Profile::new(20).unwrap()),
            [221, 421, 200, 200, 200, 600, 1, 1]
        );
        assert_eq!(
            fixture_state_counts(Bm01Profile::qualifying()),
            [
                1_100_001, 2_100_001, 1_000_000, 1_000_000, 1_000_000, 3_000_000, 1, 1
            ]
        );
    }
    #[test]
    fn disk_engine_matches_frozen_development_oracle_after_cold_admission() {
        let report = verify_disk_development_profile(Bm01Profile::new(20).unwrap()).unwrap();
        assert_eq!(report.recovered_revision, 4);
        assert_eq!(report.queries, 384);
        assert_eq!(report.cache_report.budget_bytes, 64 * 1024 * 1024);
        assert!(report.cache_report.hits > 0);
        assert!(report.cache_report.misses > 0);
        assert_eq!(
            report.output_digest,
            *blake3::Hash::from_hex(
                "46f1bdb3138d6325e4c0f56b5fd3bbf5ff092d816e8a0f6c23acd15687b910b5"
            )
            .unwrap()
            .as_bytes()
        );
        assert!(verify_disk_development_profile(Bm01Profile::qualifying()).is_err());
    }
}
