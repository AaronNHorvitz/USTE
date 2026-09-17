use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
use uste_graph::{
    AdjacencyDirection, AssertionAction, DurablePolicyMutation, Expected, GRAPH_STATE_PROFILE_V1,
    GraphDiskError, GraphState, GraphStateLoadLimits, GraphTransaction, NewEntity, NewEvidence,
    NewRecord, NewRelationship, Operation, Record, ValidTime, disk_adjacent_ids, disk_record,
    disk_supported_ids, encode_stored_record, encode_transaction, load_current_graph_index_roots,
    load_graph_state_root_candidates, load_graph_state_root_candidates_for_recovery,
    load_graph_state_roots, publish_current_graph_index, publish_graph_state_root,
    reconstruct_graph_recovery_seed, reconstruct_graph_state_candidate, scrub_current_graph_index,
    scrub_graph_state_root,
};
use uste_policy::{NamespacePolicy, PolicyVersion, QuotaLimits};
use uste_storage::{
    ClockObservation, EntryName, INDEX_PAGE_BYTES, IndexEntry, IndexRootInput, PageCache,
    fault::ScriptedClock, journal::DurableKeyEnvelope, memory::MemoryFileSystem,
};
use uste_txn::{
    AuthenticatedIndexRecovery, CheckpointState, CommitCoordinator, CoordinatorMetadataLoadLimits,
    NeverCancel, RetentionDays, TransactionRequest,
    load_coordinator_metadata_candidates_for_recovery, publish_coordinator_metadata_root,
};
use uste_types::{
    BoundedString, CommitRevision, DatabaseId, IdempotencyKey, NamespaceId, NamespaceRef, RecordId,
    RecordRef, TransactionId, UtcInstant, Value,
};

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([0x81; 16]),
        NamespaceId::from_bytes([0x82; 16]),
    )
}

fn record(value: u8) -> RecordRef {
    RecordRef::new(
        scope().database(),
        scope().namespace(),
        RecordId::from_bytes([value; 16]),
    )
}

fn text(value: &str) -> BoundedString {
    BoundedString::new(value.to_owned()).unwrap()
}

fn clock(revision: u64) -> ScriptedClock {
    ScriptedClock::new([Ok(ClockObservation {
        wall_utc: UtcInstant::new(i64::try_from(revision).unwrap(), 0).unwrap(),
        monotonic_ticks: revision,
    })])
}

fn commit(
    coordinator: &mut CommitCoordinator<
        GraphState,
        MemoryFileSystem,
        TestEnvelope,
        CounterEntropy,
        CounterEntropy,
    >,
    filesystem: &mut MemoryFileSystem,
    revision: u8,
    transaction: GraphTransaction,
) {
    let encoded = encode_transaction(&transaction).unwrap();
    coordinator
        .commit(
            filesystem,
            TransactionRequest {
                principal: uste_policy::PrincipalDigest::from_bytes([0x90; 32]),
                idempotency_key: IdempotencyKey::from_bytes([revision; 16]),
                transaction_id: TransactionId::from_bytes([revision.wrapping_add(32); 16]),
                canonical_request: &encoded,
                blob_inventory: None,
            },
            &mut clock(u64::from(revision)),
            &NeverCancel,
        )
        .unwrap();
}

#[test]
fn current_graph_disk_projection_matches_reference_and_rejects_stale_roots() {
    let left = record(1);
    let right = record(2);
    let evidence = record(3);
    let relationship = record(4);
    let mut filesystem = MemoryFileSystem::new(16 * 1024 * 1024);
    let name = EntryName::new("graph-index").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 1_000),
        CounterEntropy(2_000),
        GraphState::new(scope()),
    )
    .unwrap();
    commit(
        &mut coordinator,
        &mut filesystem,
        1,
        GraphTransaction::with_policy_mutation(
            scope(),
            vec![
                Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Entity(NewEntity {
                        id: left,
                        entity_type: text("node"),
                        schema_version: 1,
                        properties: Value::Null,
                    }),
                },
                Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Entity(NewEntity {
                        id: right,
                        entity_type: text("node"),
                        schema_version: 1,
                        properties: Value::Null,
                    }),
                },
                Operation::Create {
                    expected: Expected::Absent,
                    record: NewRecord::Evidence(NewEvidence {
                        id: evidence,
                        digest: [0x83; 32],
                        locator: text("fixture://graph-index"),
                    }),
                },
            ],
            DurablePolicyMutation::Install {
                policy: NamespacePolicy::new(
                    scope(),
                    PolicyVersion::new(1).unwrap(),
                    QuotaLimits::new(100, 1024 * 1024, 1024 * 1024, 8, 1024).unwrap(),
                ),
            },
        ),
    );
    commit(
        &mut coordinator,
        &mut filesystem,
        2,
        GraphTransaction::new(
            scope(),
            vec![Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Relationship(NewRelationship {
                    id: relationship,
                    from: left,
                    to: right,
                    relationship_type: text("edge"),
                    properties: Value::Null,
                    evidence: vec![evidence],
                    valid_time: ValidTime::Unknown,
                }),
            }],
        ),
    );
    commit(
        &mut coordinator,
        &mut filesystem,
        3,
        GraphTransaction::new(
            scope(),
            vec![Operation::ActOnRelationship {
                target: relationship,
                expected: Expected::Version(uste_graph::RecordVersion::FIRST),
                action: AssertionAction::Accept,
                correction: None,
                correction_expected: None,
            }],
        ),
    );

    let snapshot = coordinator.read_view().unwrap().state().clone();
    let foreign_id = record(9);
    let mut foreign_filesystem = MemoryFileSystem::new(4 * 1024 * 1024);
    let mut foreign = CommitCoordinator::create(
        &mut foreign_filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        EntryName::new("foreign-graph-index").unwrap(),
        create_vault(scope().database(), 8_000),
        CounterEntropy(9_000),
        GraphState::new(scope()),
    )
    .unwrap();
    commit(
        &mut foreign,
        &mut foreign_filesystem,
        1,
        GraphTransaction::new(
            scope(),
            vec![Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: foreign_id,
                    entity_type: text("foreign"),
                    schema_version: 1,
                    properties: Value::Null,
                }),
            }],
        ),
    );
    for (revision, expected, properties) in [
        (2, uste_graph::RecordVersion::FIRST, Value::Bool(false)),
        (
            3,
            uste_graph::RecordVersion::new(2).unwrap(),
            Value::Bool(true),
        ),
    ] {
        commit(
            &mut foreign,
            &mut foreign_filesystem,
            revision,
            GraphTransaction::new(
                scope(),
                vec![Operation::ReplaceEntity {
                    target: foreign_id,
                    expected: Expected::Version(expected),
                    properties,
                }],
            ),
        );
    }
    let foreign_snapshot = foreign.read_view().unwrap().state().clone();
    assert_eq!(foreign_snapshot.revision(), snapshot.revision());
    assert!(matches!(
        publish_current_graph_index(&mut coordinator, &mut filesystem, &foreign_snapshot),
        Err(GraphDiskError::RootStateMismatch)
    ));
    assert!(matches!(
        load_current_graph_index_roots(&coordinator, &mut filesystem, &foreign_snapshot),
        Err(GraphDiskError::RootStateMismatch)
    ));

    publish_current_graph_index(&mut coordinator, &mut filesystem, &snapshot).unwrap();
    let mut state_publication =
        publish_graph_state_root(&mut coordinator, &mut filesystem, &snapshot).unwrap();
    let state_roots = load_graph_state_roots(&coordinator, &mut filesystem, &snapshot).unwrap();
    assert_eq!(state_roots.len(), 1);
    assert_eq!(state_roots[0].generation(), state_publication.generation);
    let state_candidates = load_graph_state_root_candidates(&coordinator, &mut filesystem).unwrap();
    assert_eq!(state_candidates.len(), 1);
    let reconstruction_limits =
        GraphStateLoadLimits::new(10, 20, 10, 100, 100, 1024 * 1024).unwrap();
    let other_scope = NamespaceRef::new(scope().database(), NamespaceId::from_bytes([0x85; 16]));
    let mut other_filesystem = MemoryFileSystem::new(4 * 1024 * 1024);
    let other_coordinator = CommitCoordinator::create(
        &mut other_filesystem,
        other_scope,
        RetentionDays::new(30).unwrap(),
        EntryName::new("other-state-root").unwrap(),
        create_vault(other_scope.database(), 12_000),
        CounterEntropy(13_000),
        GraphState::new(other_scope),
    )
    .unwrap();
    assert!(matches!(
        reconstruct_graph_state_candidate(
            &other_coordinator,
            &mut other_filesystem,
            &state_candidates[0],
            reconstruction_limits,
        ),
        Err(GraphDiskError::RootStateMismatch)
    ));
    let (reconstructed, reconstruction) = reconstruct_graph_state_candidate(
        &coordinator,
        &mut filesystem,
        &state_candidates[0],
        reconstruction_limits,
    )
    .unwrap();
    assert_eq!(reconstructed.snapshot(), snapshot);
    assert_eq!(reconstruction.runs, 8);
    assert_eq!(reconstruction.entries, 18);
    assert!(reconstruction.pages_read >= reconstruction.runs);
    assert!(reconstruction.logical_bytes > 0);
    let too_few_records = GraphStateLoadLimits::new(3, 20, 10, 100, 100, 1024 * 1024).unwrap();
    assert!(matches!(
        reconstruct_graph_state_candidate(
            &coordinator,
            &mut filesystem,
            &state_candidates[0],
            too_few_records,
        ),
        Err(GraphDiskError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
    let too_few_pages = GraphStateLoadLimits::new(10, 20, 10, 100, 1, 1024 * 1024).unwrap();
    assert!(matches!(
        reconstruct_graph_state_candidate(
            &coordinator,
            &mut filesystem,
            &state_candidates[0],
            too_few_pages,
        ),
        Err(GraphDiskError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));

    // Storage authentication and descriptor digests are necessary but not sufficient. Build a
    // self-consistent run whose current relationship is the valid prior version while its history
    // still ends at the accepted version. Reconstruction must reject the semantic mismatch.
    let (state_revision, state_certificate_digest) =
        coordinator.checkpoint_anchor().unwrap().unwrap();
    let recovered_state_root = coordinator
        .load_index_roots(&mut filesystem, GRAPH_STATE_PROFILE_V1)
        .unwrap()
        .into_iter()
        .find(|root| root.generation() == state_publication.generation)
        .unwrap();
    let canonical_runs = recovered_state_root.runs().copied().collect::<Vec<_>>();
    let prior_relationship = snapshot
        .record_at(CommitRevision::new(2).unwrap(), relationship)
        .unwrap()
        .unwrap();
    let malformed_current = snapshot
        .records()
        .map(|(id, record)| IndexEntry {
            key: id.record().as_bytes().to_vec(),
            value: encode_stored_record(if *id == relationship {
                prior_relationship
            } else {
                record
            })
            .unwrap(),
        })
        .collect::<Vec<_>>();
    let malformed_current_run = coordinator
        .publish_index_run(
            &mut filesystem,
            state_revision,
            GRAPH_STATE_PROFILE_V1,
            2,
            malformed_current,
        )
        .unwrap();
    let mut malformed_current_runs = canonical_runs.clone();
    *malformed_current_runs
        .iter_mut()
        .find(|run| run.family() == 2)
        .unwrap() = malformed_current_run;
    let malformed_current_root = coordinator
        .publish_index_root(
            &mut filesystem,
            IndexRootInput {
                scope: scope(),
                revision: state_revision,
                certificate_digest: state_certificate_digest,
                reducer_profile: GraphState::REDUCER_PROFILE,
                logical_state_digest: GraphState::logical_state_digest(&snapshot).unwrap(),
                index_profile: GRAPH_STATE_PROFILE_V1,
            },
            &malformed_current_runs,
        )
        .unwrap();

    // Likewise, a fully authenticated outgoing run with the right shape but the wrong neighbor
    // must fail the comparison against the canonical indexes rebuilt from validated records.
    let mut outgoing_key = Vec::with_capacity(32);
    outgoing_key.extend_from_slice(left.record().as_bytes());
    outgoing_key.extend_from_slice(relationship.record().as_bytes());
    let malformed_outgoing_run = coordinator
        .publish_index_run(
            &mut filesystem,
            state_revision,
            GRAPH_STATE_PROFILE_V1,
            4,
            [IndexEntry {
                key: outgoing_key,
                value: foreign_id.record().as_bytes().to_vec(),
            }],
        )
        .unwrap();
    let mut malformed_derived_runs = canonical_runs;
    *malformed_derived_runs
        .iter_mut()
        .find(|run| run.family() == 4)
        .unwrap() = malformed_outgoing_run;
    let malformed_derived_root = coordinator
        .publish_index_root(
            &mut filesystem,
            IndexRootInput {
                scope: scope(),
                revision: state_revision,
                certificate_digest: state_certificate_digest,
                reducer_profile: GraphState::REDUCER_PROFILE,
                logical_state_digest: GraphState::logical_state_digest(&snapshot).unwrap(),
                index_profile: GRAPH_STATE_PROFILE_V1,
            },
            &malformed_derived_runs,
        )
        .unwrap();
    let semantic_candidates =
        load_graph_state_root_candidates(&coordinator, &mut filesystem).unwrap();
    let malformed_current_candidate = semantic_candidates
        .iter()
        .find(|candidate| candidate.generation() == malformed_current_root.generation)
        .unwrap();
    assert_eq!(
        reconstruct_graph_state_candidate(
            &coordinator,
            &mut filesystem,
            malformed_current_candidate,
            reconstruction_limits,
        ),
        Err(GraphDiskError::IndexCorrupt)
    );
    let malformed_derived_candidate = semantic_candidates
        .iter()
        .find(|candidate| candidate.generation() == malformed_derived_root.generation)
        .unwrap();
    assert!(matches!(
        reconstruct_graph_state_candidate(
            &coordinator,
            &mut filesystem,
            malformed_derived_candidate,
            reconstruction_limits,
        ),
        Err(GraphDiskError::Transaction(
            uste_txn::TransactionError::Storage(
                uste_storage::journal::StorageError::IntegrityFailure
            )
        ))
    ));
    // The root carrier intentionally retains two alternating manifests. Restore a current valid
    // candidate after the two negative generations so later restart/history checks exercise it.
    state_publication =
        publish_graph_state_root(&mut coordinator, &mut filesystem, &snapshot).unwrap();
    let mut state_cache = PageCache::new(INDEX_PAGE_BYTES * 2).unwrap();
    let state_scrub = scrub_graph_state_root(
        &coordinator,
        &mut filesystem,
        &state_roots[0],
        &mut state_cache,
    )
    .unwrap();
    assert_eq!(state_scrub.runs, 8);
    let wrong_state_run = coordinator
        .publish_index_run(
            &mut filesystem,
            state_revision,
            GRAPH_STATE_PROFILE_V1,
            1,
            [IndexEntry {
                key: b"graph-state-v1".to_vec(),
                value: b"wrong".to_vec(),
            }],
        )
        .unwrap();
    let wrong_metadata_root = coordinator
        .publish_index_root(
            &mut filesystem,
            IndexRootInput {
                scope: scope(),
                revision: state_revision,
                certificate_digest: state_certificate_digest,
                reducer_profile: GraphState::REDUCER_PROFILE,
                logical_state_digest: GraphState::logical_state_digest(&snapshot).unwrap(),
                index_profile: GRAPH_STATE_PROFILE_V1,
            },
            &[wrong_state_run],
        )
        .unwrap();
    let candidates = load_graph_state_root_candidates(&coordinator, &mut filesystem).unwrap();
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.generation() == state_publication.generation)
    );
    let wrong_candidate = candidates
        .iter()
        .find(|candidate| candidate.generation() == wrong_metadata_root.generation)
        .unwrap();
    assert!(
        reconstruct_graph_state_candidate(
            &coordinator,
            &mut filesystem,
            wrong_candidate,
            reconstruction_limits,
        )
        .is_err()
    );
    let state_roots = load_graph_state_roots(&coordinator, &mut filesystem, &snapshot).unwrap();
    assert_eq!(state_roots.len(), 1);
    assert_eq!(state_roots[0].generation(), state_publication.generation);
    // A self-consistent encrypted root may still be semantically wrong. Publish one with the
    // exact journal/state anchors but an incorrect mandatory metadata entry; graph admission must
    // independently retain only the projection that matches the frozen reference snapshot.
    let (revision, certificate_digest) = coordinator.checkpoint_anchor().unwrap().unwrap();
    let wrong_run = coordinator
        .publish_index_run(
            &mut filesystem,
            revision,
            uste_graph::GRAPH_INDEX_PROFILE_V1,
            1,
            [IndexEntry {
                key: b"current-graph-v1".to_vec(),
                value: b"wrong".to_vec(),
            }],
        )
        .unwrap();
    coordinator
        .publish_index_root(
            &mut filesystem,
            IndexRootInput {
                scope: scope(),
                revision,
                certificate_digest,
                reducer_profile: GraphState::REDUCER_PROFILE,
                logical_state_digest: GraphState::logical_state_digest(&snapshot).unwrap(),
                index_profile: uste_graph::GRAPH_INDEX_PROFILE_V1,
            },
            &[wrong_run],
        )
        .unwrap();
    let roots = load_current_graph_index_roots(&coordinator, &mut filesystem, &snapshot).unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].generation(), 1);
    let root = &roots[0];
    let mut cache = PageCache::new(INDEX_PAGE_BYTES * 2).unwrap();
    let scrub = scrub_current_graph_index(&coordinator, &mut filesystem, root, &mut cache).unwrap();
    assert_eq!(scrub.runs, 5);

    let (stored, _) = disk_record(
        &coordinator,
        &mut filesystem,
        root,
        relationship,
        &mut cache,
    )
    .unwrap();
    assert_eq!(stored.as_ref(), snapshot.record(relationship));
    let (adjacent, _) = disk_adjacent_ids(
        &coordinator,
        &mut filesystem,
        root,
        left,
        AdjacencyDirection::Outgoing,
        10,
        &mut cache,
    )
    .unwrap();
    assert_eq!(adjacent, vec![(relationship, right)]);
    let (incoming, _) = disk_adjacent_ids(
        &coordinator,
        &mut filesystem,
        root,
        right,
        AdjacencyDirection::Incoming,
        10,
        &mut cache,
    )
    .unwrap();
    assert_eq!(incoming, vec![(relationship, left)]);
    let (supported, _) = disk_supported_ids(
        &coordinator,
        &mut filesystem,
        root,
        evidence,
        10,
        &mut cache,
    )
    .unwrap();
    assert_eq!(supported, vec![relationship]);
    assert!(cache.accounted_bytes() <= cache.budget());

    drop(coordinator);
    filesystem.restart().unwrap();
    let (reopened, report) = CommitCoordinator::open(
        &mut filesystem,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(3_000),
        CounterEntropy(4_000),
        &mut TestKeyAdapter,
        GraphState::new(scope()),
    )
    .unwrap();
    assert_eq!(report.frontier.unwrap().get(), 3);
    coordinator = reopened;
    let restarted = coordinator.read_view().unwrap().state().clone();
    assert_eq!(restarted, snapshot);
    let restarted_roots =
        load_current_graph_index_roots(&coordinator, &mut filesystem, &restarted).unwrap();
    assert_eq!(restarted_roots.len(), 1);
    let restarted_state_roots =
        load_graph_state_roots(&coordinator, &mut filesystem, &restarted).unwrap();
    assert_eq!(restarted_state_roots.len(), 1);
    let restarted_candidates =
        load_graph_state_root_candidates(&coordinator, &mut filesystem).unwrap();
    let restarted_candidate = restarted_candidates
        .iter()
        .find(|candidate| candidate.generation() == state_publication.generation)
        .unwrap();
    let (reconstructed, _) = reconstruct_graph_state_candidate(
        &coordinator,
        &mut filesystem,
        restarted_candidate,
        reconstruction_limits,
    )
    .unwrap();
    assert_eq!(reconstructed.snapshot(), restarted);
    cache.clear();
    assert_eq!(
        disk_record(
            &coordinator,
            &mut filesystem,
            &restarted_roots[0],
            relationship,
            &mut cache,
        )
        .unwrap()
        .0
        .as_ref(),
        restarted.record(relationship)
    );

    commit(
        &mut coordinator,
        &mut filesystem,
        4,
        GraphTransaction::new(
            scope(),
            vec![Operation::ReplaceEntity {
                target: left,
                expected: Expected::Version(uste_graph::RecordVersion::FIRST),
                properties: Value::Bool(true),
            }],
        ),
    );
    let newer = coordinator.read_view().unwrap().state().clone();
    let historical_candidates =
        load_graph_state_root_candidates(&coordinator, &mut filesystem).unwrap();
    let historical_candidate = historical_candidates
        .iter()
        .find(|candidate| candidate.generation() == state_publication.generation)
        .unwrap();
    let (historical, _) = reconstruct_graph_state_candidate(
        &coordinator,
        &mut filesystem,
        historical_candidate,
        reconstruction_limits,
    )
    .unwrap();
    assert_eq!(historical.snapshot(), snapshot);
    assert!(
        load_current_graph_index_roots(&coordinator, &mut filesystem, &newer)
            .unwrap()
            .is_empty()
    );
    assert!(
        load_graph_state_roots(&coordinator, &mut filesystem, &newer)
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        scrub_graph_state_root(
            &coordinator,
            &mut filesystem,
            &restarted_state_roots[0],
            &mut state_cache,
        ),
        Err(GraphDiskError::RootStateMismatch)
    ));
    assert!(matches!(
        disk_record(
            &coordinator,
            &mut filesystem,
            &restarted_roots[0],
            relationship,
            &mut cache,
        ),
        Err(GraphDiskError::RootStateMismatch)
    ));
    assert!(matches!(
        snapshot.record(relationship),
        Some(Record::Relationship(_))
    ));
}

#[test]
fn cold_root_pair_reconstructs_seed_and_replays_graph_suffix() {
    let entity = record(0x21);
    let mut filesystem = MemoryFileSystem::new(16 * 1024 * 1024);
    let name = EntryName::new("cold-root-pair").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 30_000),
        CounterEntropy(31_000),
        GraphState::new(scope()),
    )
    .unwrap();
    commit(
        &mut coordinator,
        &mut filesystem,
        1,
        GraphTransaction::with_policy_mutation(
            scope(),
            vec![Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: entity,
                    entity_type: text("tracked-item"),
                    schema_version: 1,
                    properties: Value::Null,
                }),
            }],
            DurablePolicyMutation::Install {
                policy: NamespacePolicy::new(
                    scope(),
                    PolicyVersion::new(1).unwrap(),
                    QuotaLimits::new(100, 1024 * 1024, 1024 * 1024, 8, 1024).unwrap(),
                ),
            },
        ),
    );
    let checkpoint_snapshot = coordinator.read_view().unwrap().state().clone();
    let graph_publication =
        publish_graph_state_root(&mut coordinator, &mut filesystem, &checkpoint_snapshot).unwrap();
    let metadata_publication =
        publish_coordinator_metadata_root(&mut coordinator, &mut filesystem).unwrap();
    assert_eq!(graph_publication.revision, metadata_publication.revision);

    commit(
        &mut coordinator,
        &mut filesystem,
        2,
        GraphTransaction::new(
            scope(),
            vec![Operation::ReplaceEntity {
                target: entity,
                expected: Expected::Version(uste_graph::RecordVersion::FIRST),
                properties: Value::Bool(true),
            }],
        ),
    );
    let expected = coordinator.read_view().unwrap().state().clone();
    let newer_metadata_publication =
        publish_coordinator_metadata_root(&mut coordinator, &mut filesystem).unwrap();
    drop(coordinator);
    filesystem.restart().unwrap();

    let (recovery, storage_report) = AuthenticatedIndexRecovery::open(
        &mut filesystem,
        &name,
        scope(),
        CounterEntropy(32_000),
        CounterEntropy(33_000),
        &mut TestKeyAdapter,
    )
    .unwrap();
    assert_eq!(storage_report.frontier.unwrap().get(), 2);
    let graph_candidates =
        load_graph_state_root_candidates_for_recovery(&recovery, &mut filesystem).unwrap();
    let graph_candidate = graph_candidates
        .iter()
        .find(|candidate| candidate.generation() == graph_publication.generation)
        .unwrap();
    let metadata_candidates =
        load_coordinator_metadata_candidates_for_recovery::<GraphState, _, _, _, _>(
            &recovery,
            &mut filesystem,
        )
        .unwrap();
    let newer_metadata_candidate = metadata_candidates
        .iter()
        .find(|candidate| candidate.generation() == newer_metadata_publication.generation)
        .unwrap();
    assert!(matches!(
        reconstruct_graph_recovery_seed(
            &recovery,
            &mut filesystem,
            graph_candidate,
            GraphStateLoadLimits::new(10, 20, 10, 100, 100, 1024 * 1024).unwrap(),
            newer_metadata_candidate,
            CoordinatorMetadataLoadLimits::new(10, 10, 21, 20, 1024 * 1024).unwrap(),
        ),
        Err(GraphDiskError::RootStateMismatch)
    ));
    let metadata_candidate = metadata_candidates
        .iter()
        .find(|candidate| {
            candidate.generation() == metadata_publication.generation
                && candidate.anchor() == graph_candidate.anchor()
        })
        .unwrap();
    let (seed, recovery_report) = reconstruct_graph_recovery_seed(
        &recovery,
        &mut filesystem,
        graph_candidate,
        GraphStateLoadLimits::new(10, 20, 10, 100, 100, 1024 * 1024).unwrap(),
        metadata_candidate,
        CoordinatorMetadataLoadLimits::new(10, 10, 21, 20, 1024 * 1024).unwrap(),
    )
    .unwrap();
    assert!(recovery_report.graph_state.runs >= 4);
    assert_eq!(recovery_report.coordinator_metadata.runs, 2);
    assert_eq!(recovery_report.coordinator_metadata.entries, 2);
    drop(recovery);

    let (recovered, seeded_report) = CommitCoordinator::open_seeded(
        &mut filesystem,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(34_000),
        CounterEntropy(35_000),
        &mut TestKeyAdapter,
        seed,
    )
    .unwrap();
    assert_eq!(seeded_report.frontier.unwrap().get(), 2);
    assert_eq!(recovered.read_view().unwrap().state(), &expected);
    assert_eq!(recovered.checkpoint_outcomes().len(), 2);
}

#[derive(Debug)]
struct TestEnvelope([u8; 32]);

impl DurableKeyEnvelope for TestEnvelope {
    fn encode_durable(&self) -> Result<Vec<u8>, CryptoError> {
        Ok(self.0.to_vec())
    }

    fn decode_durable(encoded: &[u8]) -> Result<Self, CryptoError> {
        Ok(Self(
            encoded
                .try_into()
                .map_err(|_| CryptoError::InvalidEnvelope)?,
        ))
    }
}

struct TestKeyAdapter;

impl KeyAdapter for TestKeyAdapter {
    type Envelope = TestEnvelope;

    fn wrap(
        &mut self,
        _database: DatabaseId,
        key: &SecretKeyMaterial,
        _entropy: &mut dyn EntropySource,
    ) -> Result<Self::Envelope, CryptoError> {
        Ok(TestEnvelope(*key.expose_to_adapter()))
    }

    fn unwrap(
        &mut self,
        _database: DatabaseId,
        envelope: &Self::Envelope,
    ) -> Result<SecretKeyMaterial, CryptoError> {
        Ok(SecretKeyMaterial::from_adapter_bytes(envelope.0))
    }
}

#[derive(Debug)]
struct CounterEntropy(u64);

impl EntropySource for CounterEntropy {
    fn fill(&mut self, output: &mut [u8]) -> Result<(), EntropyFailure> {
        self.0 = self.0.checked_add(1).ok_or(EntropyFailure)?;
        for (index, chunk) in output.chunks_mut(8).enumerate() {
            let value = self
                .0
                .checked_add(u64::try_from(index).map_err(|_| EntropyFailure)?)
                .ok_or(EntropyFailure)?;
            chunk.copy_from_slice(&value.to_be_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}

fn create_vault(database: DatabaseId, seed: u64) -> KeyVault<TestEnvelope, CounterEntropy> {
    KeyVault::create(database, &mut TestKeyAdapter, CounterEntropy(seed)).unwrap()
}
