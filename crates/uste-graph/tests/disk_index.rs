use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
use uste_graph::{
    AdjacencyDirection, AssertionAction, DeletePolicy, DurablePolicyMutation, Expected,
    GRAPH_STATE_PROFILE_V1, GraphDiskBaseAdmissionLimits, GraphDiskError, GraphDiskLiveState,
    GraphDiskPreparationLimits, GraphError, GraphState, GraphStateDeltaLimits,
    GraphStateLoadLimits, GraphStateRootMergeLimits, GraphTransaction, NewAssertion, NewEntity,
    NewEvidence, NewRecord, NewRelationship, Operation, Predicate, Record, ValidTime,
    admit_graph_disk_base_candidate, admit_graph_disk_base_candidate_for_recovery,
    commit_graph_disk_live_prepared, commit_graph_disk_prepared, disk_adjacent_ids, disk_record,
    disk_supported_ids, encode_stored_record, encode_transaction, load_current_graph_index_roots,
    load_graph_disk_live_preparation_view, load_graph_disk_preparation_view,
    load_graph_disk_recovery_preparation_view, load_graph_state_root_candidates,
    load_graph_state_root_candidates_for_recovery, load_graph_state_roots,
    prepare_graph_disk_commit, prepare_graph_state_root_delta,
    prepare_graph_state_root_delta_from_disk, publish_current_graph_index,
    publish_graph_disk_live_base, publish_graph_state_root, publish_graph_state_root_delta,
    reconstruct_graph_recovery_seed, reconstruct_graph_state_candidate, scrub_current_graph_index,
    scrub_graph_state_root,
};
use uste_policy::{NamespacePolicy, PolicyVersion, QuotaLimits};
use uste_storage::{
    ClockObservation, EntryName, INDEX_PAGE_BYTES, IndexEntry, IndexPredecessorLimits,
    IndexRootInput, IndexRunMergeLimits, IndexRunReadLimits, PageCache, fault::ScriptedClock,
    journal::DurableKeyEnvelope, memory::MemoryFileSystem,
};
use uste_txn::{
    AuthenticatedIndexRecovery, CheckpointState, CommitCoordinator, CoordinatorMetadataLoadLimits,
    NeverCancel, RetentionDays, TransactionOutcome, TransactionRequest,
    load_coordinator_metadata_candidates_for_recovery, publish_coordinator_metadata_root,
};
use uste_types::{
    BoundedString, CommitRevision, DatabaseId, IdempotencyKey, NamespaceId, NamespaceRef, RecordId,
    RecordRef, TransactionId, UtcInstant, Value,
};

#[path = "support/disk_recovery_faults.rs"]
mod disk_recovery_faults;

#[path = "support/disk_queries.rs"]
mod disk_queries;
#[path = "support/packed_bridge.rs"]
mod packed_bridge;

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

fn state_load_limits() -> GraphStateLoadLimits {
    GraphStateLoadLimits::new(100, 1_000, 100, 10_000, 10_000, 16 * 1024 * 1024).unwrap()
}

fn admission_limits() -> GraphDiskBaseAdmissionLimits {
    GraphDiskBaseAdmissionLimits::new(
        state_load_limits(),
        100,
        4 * 1024 * 1024,
        100_000,
        10_000,
        100_000,
        64 * 1024 * 1024,
        IndexPredecessorLimits::new(1_000, 16 * 1024 * 1024).unwrap(),
    )
    .unwrap()
}

fn commit<F: uste_storage::OwnershipFileSystem>(
    coordinator: &mut CommitCoordinator<
        GraphState,
        F,
        TestEnvelope,
        CounterEntropy,
        CounterEntropy,
    >,
    filesystem: &mut F,
    revision: u8,
    transaction: GraphTransaction,
) -> TransactionOutcome {
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
        .unwrap()
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
    let mut admission_cache = PageCache::new(INDEX_PAGE_BYTES * 4).unwrap();
    let (disk_base, admission) = admit_graph_disk_base_candidate(
        &coordinator,
        &mut filesystem,
        &state_candidates[0],
        admission_limits(),
        &mut admission_cache,
    )
    .unwrap();
    assert_eq!(disk_base.scope(), scope());
    assert_eq!(disk_base.anchor(), state_candidates[0].anchor());
    assert_eq!(disk_base.revision(), snapshot.revision().unwrap());
    assert_eq!(disk_base.generation(), state_candidates[0].generation());
    assert_eq!(disk_base.namespace_policy(), snapshot.namespace_policy());
    assert_eq!(disk_base.state_counts(), [4, 5, 1, 1, 1, 3, 1, 1]);
    assert_eq!(admission.scan.runs, 8);
    assert_eq!(admission.scan.entries, 18);
    assert!(admission.predecessor_lookups > 0);
    assert!(admission.exact_lookups > 0);
    assert!(admission.semantic_reference_visits > 1);
    assert!(admission.lookup_page_visits > 1);
    assert!(admission.lookup_result_bytes > 1);
    assert!(format!("{disk_base:?}").contains("[REDACTED]"));
    assert_eq!(
        scrub_graph_state_root(
            &coordinator,
            &mut filesystem,
            disk_base.admitted_root(),
            &mut admission_cache,
        )
        .unwrap()
        .runs,
        8
    );
    let handoff = load_graph_disk_preparation_view(
        &coordinator,
        &mut filesystem,
        disk_base.admitted_root(),
        GraphTransaction::new(
            scope(),
            vec![Operation::ReplaceEntity {
                target: left,
                expected: Expected::Version(uste_graph::RecordVersion::FIRST),
                properties: Value::Bool(true),
            }],
        ),
        GraphDiskPreparationLimits::new(10, 100, 10, 10, 1024 * 1024).unwrap(),
        &mut admission_cache,
    )
    .unwrap()
    .prepare()
    .unwrap();
    assert_eq!(handoff.base_revision(), snapshot.revision().unwrap());
    assert_eq!(
        handoff.revision().get(),
        snapshot.revision().unwrap().get() + 1
    );
    let mut limit_baseline_cache = PageCache::new(INDEX_PAGE_BYTES * 4).unwrap();
    let (_, bounded_admission) = admit_graph_disk_base_candidate(
        &coordinator,
        &mut filesystem,
        &state_candidates[0],
        admission_limits(),
        &mut limit_baseline_cache,
    )
    .unwrap();
    let one_history_version = GraphDiskBaseAdmissionLimits::new(
        state_load_limits(),
        1,
        4 * 1024 * 1024,
        100_000,
        10_000,
        100_000,
        64 * 1024 * 1024,
        IndexPredecessorLimits::new(1_000, 16 * 1024 * 1024).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        admit_graph_disk_base_candidate(
            &coordinator,
            &mut filesystem,
            &state_candidates[0],
            one_history_version,
            &mut admission_cache,
        ),
        Err(GraphDiskError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
    let lookup_count = admission.exact_lookups + admission.predecessor_lookups;
    let one_lookup_short = GraphDiskBaseAdmissionLimits::new(
        state_load_limits(),
        100,
        4 * 1024 * 1024,
        100_000,
        lookup_count - 1,
        100_000,
        64 * 1024 * 1024,
        IndexPredecessorLimits::new(1_000, 16 * 1024 * 1024).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        admit_graph_disk_base_candidate(
            &coordinator,
            &mut filesystem,
            &state_candidates[0],
            one_lookup_short,
            &mut admission_cache,
        ),
        Err(GraphDiskError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
    let one_semantic_visit_short = GraphDiskBaseAdmissionLimits::new(
        state_load_limits(),
        100,
        4 * 1024 * 1024,
        bounded_admission.semantic_reference_visits - 1,
        10_000,
        100_000,
        64 * 1024 * 1024,
        IndexPredecessorLimits::new(1_000, 16 * 1024 * 1024).unwrap(),
    )
    .unwrap();
    let mut semantic_limit_cache = PageCache::new(INDEX_PAGE_BYTES * 4).unwrap();
    assert!(matches!(
        admit_graph_disk_base_candidate(
            &coordinator,
            &mut filesystem,
            &state_candidates[0],
            one_semantic_visit_short,
            &mut semantic_limit_cache,
        ),
        Err(GraphDiskError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
    let one_lookup_page_short = GraphDiskBaseAdmissionLimits::new(
        state_load_limits(),
        100,
        4 * 1024 * 1024,
        100_000,
        10_000,
        bounded_admission.lookup_page_visits - 1,
        64 * 1024 * 1024,
        IndexPredecessorLimits::new(1_000, 16 * 1024 * 1024).unwrap(),
    )
    .unwrap();
    let mut page_limit_cache = PageCache::new(INDEX_PAGE_BYTES * 4).unwrap();
    let page_limit_result = admit_graph_disk_base_candidate(
        &coordinator,
        &mut filesystem,
        &state_candidates[0],
        one_lookup_page_short,
        &mut page_limit_cache,
    );
    assert!(
        matches!(
            page_limit_result,
            Err(GraphDiskError::Storage(
                uste_storage::journal::StorageError::ResourceLimit
            ))
        ),
        "baseline={bounded_admission:?}; limited={page_limit_result:?}"
    );
    let one_lookup_byte_short = GraphDiskBaseAdmissionLimits::new(
        state_load_limits(),
        100,
        4 * 1024 * 1024,
        100_000,
        10_000,
        100_000,
        bounded_admission.lookup_result_bytes - 1,
        IndexPredecessorLimits::new(1_000, 16 * 1024 * 1024).unwrap(),
    )
    .unwrap();
    let mut byte_limit_cache = PageCache::new(INDEX_PAGE_BYTES * 4).unwrap();
    assert!(matches!(
        admit_graph_disk_base_candidate(
            &coordinator,
            &mut filesystem,
            &state_candidates[0],
            one_lookup_byte_short,
            &mut byte_limit_cache,
        ),
        Err(GraphDiskError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
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
    assert!(
        admit_graph_disk_base_candidate(
            &coordinator,
            &mut filesystem,
            malformed_current_candidate,
            admission_limits(),
            &mut admission_cache,
        )
        .is_err()
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
    assert!(
        admit_graph_disk_base_candidate(
            &coordinator,
            &mut filesystem,
            malformed_derived_candidate,
            admission_limits(),
            &mut admission_cache,
        )
        .is_err()
    );
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
fn bounded_disk_preparation_supports_current_history_reverse_and_stale_roots() {
    let left = record(0x21);
    let right = record(0x22);
    let evidence = record(0x23);
    let assertion = record(0x24);
    let mut filesystem = MemoryFileSystem::new(16 * 1024 * 1024);
    let name = EntryName::new("graph-disk-prepare").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 20_000),
        CounterEntropy(21_000),
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
                        digest: [0x25; 32],
                        locator: text("fixture://disk-prepare"),
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
    let snapshot = coordinator.read_view().unwrap().state().clone();
    publish_graph_state_root(&mut coordinator, &mut filesystem, &snapshot).unwrap();
    let roots = load_graph_state_roots(&coordinator, &mut filesystem, &snapshot).unwrap();
    let transaction = GraphTransaction::with_policy_mutation(
        scope(),
        vec![Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Assertion(NewAssertion {
                id: assertion,
                subject: left,
                predicate: text("linked"),
                object: Value::RecordRef(right),
                evidence: vec![evidence],
                valid_time: ValidTime::Unknown,
            }),
        }],
        DurablePolicyMutation::Replace {
            expected: PolicyVersion::new(1).unwrap(),
            policy: NamespacePolicy::new(
                scope(),
                PolicyVersion::new(2).unwrap(),
                QuotaLimits::new(120, 1024 * 1024, 1024 * 1024, 8, 1024).unwrap(),
            ),
        },
    );
    let limits = GraphDiskPreparationLimits::new(8, 8, 8, 8, 1024 * 1024).unwrap();
    let mut cache = PageCache::new(1024 * 1024).unwrap();
    let view = load_graph_disk_preparation_view(
        &coordinator,
        &mut filesystem,
        &roots[0],
        transaction.clone(),
        limits,
        &mut cache,
    )
    .unwrap();
    assert_eq!(view.base_revision(), CommitRevision::FIRST);
    assert_eq!(view.report().record_proofs, 4);
    assert_eq!(view.report().present_records, 3);
    assert_eq!(view.report().absent_records, 1);
    assert_eq!(view.report().reference_visits, 4);
    assert_eq!(view.report().index_lookups, 6);
    let exact = GraphDiskPreparationLimits::new(
        view.report().record_proofs,
        view.report().reference_visits,
        view.report().history_versions,
        view.report().reverse_references,
        view.report().proof_logical_bytes,
    )
    .unwrap();
    let mut exact_cache = PageCache::new(1024 * 1024).unwrap();
    load_graph_disk_preparation_view(
        &coordinator,
        &mut filesystem,
        &roots[0],
        transaction.clone(),
        exact,
        &mut exact_cache,
    )
    .unwrap();
    let too_few_bytes = GraphDiskPreparationLimits::new(
        view.report().record_proofs,
        view.report().reference_visits,
        8,
        8,
        view.report().proof_logical_bytes - 1,
    )
    .unwrap();
    let mut byte_cache = PageCache::new(1024 * 1024).unwrap();
    assert!(matches!(
        load_graph_disk_preparation_view(
            &coordinator,
            &mut filesystem,
            &roots[0],
            transaction.clone(),
            too_few_bytes,
            &mut byte_cache,
        ),
        Err(GraphDiskError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
    let too_few_proofs = GraphDiskPreparationLimits::new(3, 8, 8, 8, 1024 * 1024).unwrap();
    let mut proof_cache = PageCache::new(1024 * 1024).unwrap();
    assert!(matches!(
        load_graph_disk_preparation_view(
            &coordinator,
            &mut filesystem,
            &roots[0],
            transaction.clone(),
            too_few_proofs,
            &mut proof_cache,
        ),
        Err(GraphDiskError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
    let too_few_references = GraphDiskPreparationLimits::new(8, 3, 8, 8, 1024 * 1024).unwrap();
    let mut rejected_cache = PageCache::new(1024 * 1024).unwrap();
    assert!(matches!(
        load_graph_disk_preparation_view(
            &coordinator,
            &mut filesystem,
            &roots[0],
            transaction.clone(),
            too_few_references,
            &mut rejected_cache,
        ),
        Err(GraphDiskError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));

    let prepared = view.prepare().unwrap();
    assert_eq!(prepared.revision(), CommitRevision::new(2).unwrap());
    assert_eq!(prepared.change_count(), 1);
    let memory_plan = prepare_graph_state_root_delta(
        &coordinator,
        &roots[0],
        &transaction,
        CommitRevision::new(2).unwrap(),
        GraphStateDeltaLimits::new(100, 1024 * 1024).unwrap(),
    )
    .unwrap();
    let disk_plan = prepare_graph_state_root_delta_from_disk(
        &prepared,
        GraphStateDeltaLimits::new(100, 1024 * 1024).unwrap(),
    )
    .unwrap();
    assert_eq!(disk_plan.delta_count(), memory_plan.delta_count());
    assert_eq!(disk_plan.logical_bytes(), memory_plan.logical_bytes());
    let mut stale_token_cache = PageCache::new(1024 * 1024).unwrap();
    let stale_prepared = load_graph_disk_preparation_view(
        &coordinator,
        &mut filesystem,
        &roots[0],
        transaction.clone(),
        limits,
        &mut stale_token_cache,
    )
    .unwrap()
    .prepare()
    .unwrap();
    let mut retry_token_cache = PageCache::new(1024 * 1024).unwrap();
    let retry_prepared = load_graph_disk_preparation_view(
        &coordinator,
        &mut filesystem,
        &roots[0],
        transaction.clone(),
        limits,
        &mut retry_token_cache,
    )
    .unwrap()
    .prepare()
    .unwrap();
    let mut mismatch_cache = PageCache::new(1024 * 1024).unwrap();
    let mismatch_prepared = load_graph_disk_preparation_view(
        &coordinator,
        &mut filesystem,
        &roots[0],
        transaction.clone(),
        limits,
        &mut mismatch_cache,
    )
    .unwrap()
    .prepare()
    .unwrap();
    let mismatched_transaction = GraphTransaction::new(
        scope(),
        vec![Operation::ReplaceEntity {
            target: left,
            expected: Expected::Version(uste_graph::RecordVersion::FIRST),
            properties: Value::Bool(false),
        }],
    );
    let mismatched_encoded = encode_transaction(&mismatched_transaction).unwrap();
    assert!(matches!(
        commit_graph_disk_prepared(
            &mut coordinator,
            &mut filesystem,
            TransactionRequest {
                principal: uste_policy::PrincipalDigest::from_bytes([0x90; 32]),
                idempotency_key: IdempotencyKey::from_bytes([0x72; 16]),
                transaction_id: TransactionId::from_bytes([0x73; 16]),
                canonical_request: &mismatched_encoded,
                blob_inventory: None,
            },
            mismatch_prepared,
            &mut clock(2),
            &NeverCancel,
        ),
        Err(GraphDiskError::Transaction(
            uste_txn::TransactionError::InvalidRequest
        ))
    ));
    assert_eq!(
        coordinator.read_view().unwrap().revision(),
        Some(CommitRevision::FIRST)
    );

    let encoded = encode_transaction(&transaction).unwrap();
    let outcome = commit_graph_disk_prepared(
        &mut coordinator,
        &mut filesystem,
        TransactionRequest {
            principal: uste_policy::PrincipalDigest::from_bytes([0x90; 32]),
            idempotency_key: IdempotencyKey::from_bytes([2; 16]),
            transaction_id: TransactionId::from_bytes([34; 16]),
            canonical_request: &encoded,
            blob_inventory: None,
        },
        prepared,
        &mut clock(2),
        &NeverCancel,
    )
    .unwrap();
    let retry_outcome = commit_graph_disk_prepared(
        &mut coordinator,
        &mut filesystem,
        TransactionRequest {
            principal: uste_policy::PrincipalDigest::from_bytes([0x90; 32]),
            idempotency_key: IdempotencyKey::from_bytes([2; 16]),
            transaction_id: TransactionId::from_bytes([34; 16]),
            canonical_request: &encoded,
            blob_inventory: None,
        },
        retry_prepared,
        &mut clock(2),
        &NeverCancel,
    )
    .unwrap();
    assert_eq!(retry_outcome, outcome);
    assert!(matches!(
        commit_graph_disk_prepared(
            &mut coordinator,
            &mut filesystem,
            TransactionRequest {
                principal: uste_policy::PrincipalDigest::from_bytes([0x90; 32]),
                idempotency_key: IdempotencyKey::from_bytes([0x74; 16]),
                transaction_id: TransactionId::from_bytes([0x75; 16]),
                canonical_request: &encoded,
                blob_inventory: None,
            },
            stale_prepared,
            &mut clock(3),
            &NeverCancel,
        ),
        Err(GraphDiskError::Transaction(
            uste_txn::TransactionError::Conflict
        ))
    ));
    let base_read = IndexRunReadLimits::new(100, 100, 1024 * 1024).unwrap();
    let merge = IndexRunMergeLimits::new(base_read, 100, 1024 * 1024, 100, 1024 * 1024).unwrap();
    let (delta_root, _) = publish_graph_state_root_delta(
        &mut coordinator,
        &mut filesystem,
        &roots[0],
        &disk_plan,
        outcome,
        GraphStateRootMergeLimits::uniform(merge, 2 * 1024 * 1024).unwrap(),
    )
    .unwrap();

    let revision_two = coordinator.read_view().unwrap().state().clone();
    assert_eq!(delta_root.revision(), CommitRevision::new(2).unwrap());
    drop(coordinator);
    filesystem.restart().unwrap();
    let (reopened, recovery) = CommitCoordinator::open(
        &mut filesystem,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(22_000),
        CounterEntropy(23_000),
        &mut TestKeyAdapter,
        GraphState::new(scope()),
    )
    .unwrap();
    assert_eq!(recovery.frontier, Some(CommitRevision::new(2).unwrap()));
    assert_eq!(reopened.read_view().unwrap().state(), &revision_two);
    coordinator = reopened;
    let transition = GraphTransaction::new(
        scope(),
        vec![Operation::ActOnAssertion {
            target: assertion,
            expected: Expected::Version(uste_graph::RecordVersion::FIRST),
            action: AssertionAction::Accept,
            correction: None,
            correction_expected: None,
        }],
    );
    let mut transition_limit_cache = PageCache::new(1024 * 1024).unwrap();
    assert!(matches!(
        load_graph_disk_preparation_view(
            &coordinator,
            &mut filesystem,
            &delta_root,
            transition.clone(),
            GraphDiskPreparationLimits::new(3, 8, 8, 8, 1024 * 1024).unwrap(),
            &mut transition_limit_cache,
        ),
        Err(GraphDiskError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
    let mut transition_cache = PageCache::new(1024 * 1024).unwrap();
    let transition_view = load_graph_disk_preparation_view(
        &coordinator,
        &mut filesystem,
        &delta_root,
        transition.clone(),
        limits,
        &mut transition_cache,
    )
    .unwrap();
    assert_eq!(transition_view.report().record_proofs, 4);
    assert_eq!(transition_view.report().present_records, 4);
    assert_eq!(transition_view.report().reference_visits, 4);
    let transition_prepared = transition_view.prepare().unwrap();
    let transition_outcome = commit(&mut coordinator, &mut filesystem, 3, transition);
    assert_eq!(
        transition_prepared.result_digest(),
        transition_outcome.result_digest
    );

    let revision_three = coordinator.read_view().unwrap().state().clone();
    publish_graph_state_root(&mut coordinator, &mut filesystem, &revision_three).unwrap();
    let revision_three_roots =
        load_graph_state_roots(&coordinator, &mut filesystem, &revision_three).unwrap();
    let occupied_correction = GraphTransaction::new(
        scope(),
        vec![Operation::ActOnAssertion {
            target: assertion,
            expected: Expected::Version(uste_graph::RecordVersion::new(2).unwrap()),
            action: AssertionAction::Correct,
            correction: Some(NewAssertion {
                id: right,
                subject: left,
                predicate: text("corrected"),
                object: Value::Null,
                evidence: vec![evidence],
                valid_time: ValidTime::Unknown,
            }),
            correction_expected: Some(Expected::Absent),
        }],
    );
    let mut correction_cache = PageCache::new(1024 * 1024).unwrap();
    let correction_view = load_graph_disk_preparation_view(
        &coordinator,
        &mut filesystem,
        &revision_three_roots[0],
        occupied_correction,
        limits,
        &mut correction_cache,
    )
    .unwrap();
    assert_eq!(correction_view.report().record_proofs, 4);
    assert!(matches!(
        correction_view.prepare(),
        Err(GraphDiskError::Graph(GraphError::PreconditionFailed(id))) if id == right
    ));

    let overlay_delete = GraphTransaction::new(
        scope(),
        vec![
            Operation::ActOnAssertion {
                target: assertion,
                expected: Expected::Version(uste_graph::RecordVersion::new(2).unwrap()),
                action: AssertionAction::Retract,
                correction: None,
                correction_expected: None,
            },
            Operation::DeleteEntity {
                target: right,
                expected: Expected::Version(uste_graph::RecordVersion::FIRST),
                policy: DeletePolicy::Reject,
                affected: Vec::new(),
            },
        ],
    );
    let mut reverse_limit_cache = PageCache::new(1024 * 1024).unwrap();
    assert!(matches!(
        load_graph_disk_preparation_view(
            &coordinator,
            &mut filesystem,
            &revision_three_roots[0],
            overlay_delete.clone(),
            GraphDiskPreparationLimits::new(8, 8, 8, 0, 1024 * 1024).unwrap(),
            &mut reverse_limit_cache,
        ),
        Err(GraphDiskError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
    let mut overlay_delete_cache = PageCache::new(1024 * 1024).unwrap();
    let overlay_delete_view = load_graph_disk_preparation_view(
        &coordinator,
        &mut filesystem,
        &revision_three_roots[0],
        overlay_delete.clone(),
        limits,
        &mut overlay_delete_cache,
    )
    .unwrap();
    assert_eq!(overlay_delete_view.report().reverse_references, 1);
    let overlay_delete_prepared = overlay_delete_view.prepare().unwrap();
    let overlay_delete_outcome = commit(&mut coordinator, &mut filesystem, 4, overlay_delete);
    assert_eq!(overlay_delete_prepared.change_count(), 2);
    assert_eq!(
        overlay_delete_prepared.result_digest(),
        overlay_delete_outcome.result_digest
    );

    let revision_four = coordinator.read_view().unwrap().state().clone();
    publish_graph_state_root(&mut coordinator, &mut filesystem, &revision_four).unwrap();
    let revision_four_roots =
        load_graph_state_roots(&coordinator, &mut filesystem, &revision_four).unwrap();
    let changed_predicate = GraphTransaction::new(
        scope(),
        vec![Operation::ReplaceEntity {
            target: left,
            expected: Expected::ReadView {
                revision: CommitRevision::FIRST,
                predicate: Predicate::RecordVisible(right),
            },
            properties: Value::Bool(false),
        }],
    );
    let mut changed_predicate_cache = PageCache::new(1024 * 1024).unwrap();
    let changed_predicate_view = load_graph_disk_preparation_view(
        &coordinator,
        &mut filesystem,
        &revision_four_roots[0],
        changed_predicate,
        limits,
        &mut changed_predicate_cache,
    )
    .unwrap();
    assert_eq!(changed_predicate_view.report().history_versions, 2);
    assert!(matches!(
        changed_predicate_view.prepare(),
        Err(GraphDiskError::Graph(GraphError::PredicateChanged(revision)))
            if revision == CommitRevision::FIRST
    ));
    let read_view = GraphTransaction::new(
        scope(),
        vec![Operation::ReplaceEntity {
            target: left,
            expected: Expected::ReadView {
                revision: CommitRevision::FIRST,
                predicate: Predicate::RecordVisible(left),
            },
            properties: Value::Bool(true),
        }],
    );
    let mut history_limit_cache = PageCache::new(1024 * 1024).unwrap();
    assert!(matches!(
        load_graph_disk_preparation_view(
            &coordinator,
            &mut filesystem,
            &revision_four_roots[0],
            read_view.clone(),
            GraphDiskPreparationLimits::new(8, 8, 0, 8, 1024 * 1024).unwrap(),
            &mut history_limit_cache,
        ),
        Err(GraphDiskError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
    let mut read_view_cache = PageCache::new(1024 * 1024).unwrap();
    let historical_view = load_graph_disk_preparation_view(
        &coordinator,
        &mut filesystem,
        &revision_four_roots[0],
        read_view.clone(),
        limits,
        &mut read_view_cache,
    )
    .unwrap();
    assert_eq!(historical_view.report().history_versions, 1);
    let historical_prepared = historical_view.prepare().unwrap();
    let historical_outcome = commit(&mut coordinator, &mut filesystem, 5, read_view);
    assert_eq!(
        historical_prepared.result_digest(),
        historical_outcome.result_digest
    );

    let mut stale_cache = PageCache::new(1024 * 1024).unwrap();
    assert!(matches!(
        load_graph_disk_preparation_view(
            &coordinator,
            &mut filesystem,
            &roots[0],
            transaction,
            limits,
            &mut stale_cache,
        ),
        Err(GraphDiskError::RootStateMismatch)
    ));
}

#[test]
fn warm_disk_state_blocks_stale_progress_and_repairs_failed_publication() {
    let entity = record(0x29);
    let mut filesystem = MemoryFileSystem::new(16 * 1024 * 1024);
    let name = EntryName::new("graph-disk-live").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 25_000),
        CounterEntropy(26_000),
        GraphState::new(scope()),
    )
    .unwrap();
    commit(
        &mut coordinator,
        &mut filesystem,
        1,
        GraphTransaction::new(
            scope(),
            vec![Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: entity,
                    entity_type: text("tracked"),
                    schema_version: 1,
                    properties: Value::Null,
                }),
            }],
        ),
    );
    let snapshot = coordinator.read_view().unwrap().state().clone();
    publish_graph_state_root(&mut coordinator, &mut filesystem, &snapshot).unwrap();
    let candidates = load_graph_state_root_candidates(&coordinator, &mut filesystem).unwrap();
    let mut admission_cache = PageCache::new(1024 * 1024).unwrap();
    let (base, _) = admit_graph_disk_base_candidate(
        &coordinator,
        &mut filesystem,
        &candidates[0],
        admission_limits(),
        &mut admission_cache,
    )
    .unwrap();
    let mut coordinator = match coordinator.into_equivalent_state(GraphDiskLiveState::new(base)) {
        Ok(coordinator) => coordinator,
        Err(_) => panic!("admitted root must be equivalent to its live graph state"),
    };

    let transaction = GraphTransaction::new(
        scope(),
        vec![Operation::ReplaceEntity {
            target: entity,
            expected: Expected::Version(uste_graph::RecordVersion::FIRST),
            properties: Value::Bool(true),
        }],
    );
    let encoded = encode_transaction(&transaction).unwrap();
    let proof_limits = GraphDiskPreparationLimits::new(8, 8, 8, 8, 1024 * 1024).unwrap();
    let mut proof_cache = PageCache::new(1024 * 1024).unwrap();
    let prepared = load_graph_disk_live_preparation_view(
        &coordinator,
        &mut filesystem,
        transaction.clone(),
        proof_limits,
        &mut proof_cache,
    )
    .unwrap()
    .prepare()
    .unwrap();
    let prepared = prepare_graph_disk_commit(
        prepared,
        GraphStateDeltaLimits::new(100, 1024 * 1024).unwrap(),
    )
    .unwrap();
    let request = TransactionRequest {
        principal: uste_policy::PrincipalDigest::from_bytes([0x90; 32]),
        idempotency_key: IdempotencyKey::from_bytes([2; 16]),
        transaction_id: TransactionId::from_bytes([34; 16]),
        canonical_request: &encoded,
        blob_inventory: None,
    };
    let outcome = commit_graph_disk_live_prepared(
        &mut coordinator,
        &mut filesystem,
        request,
        prepared.clone(),
        &mut clock(2),
        &NeverCancel,
    )
    .unwrap();
    assert!(
        coordinator
            .reducer_state_for_checkpoint()
            .unwrap()
            .is_pending()
    );

    drop(coordinator);
    filesystem.restart().unwrap();
    let (recovery, recovery_report, frontier) =
        AuthenticatedIndexRecovery::open_with_frontier_transaction(
            &mut filesystem,
            &name,
            scope(),
            CounterEntropy(27_000),
            CounterEntropy(28_000),
            &mut TestKeyAdapter,
        )
        .unwrap();
    assert_eq!(
        recovery_report.frontier,
        Some(CommitRevision::new(2).unwrap())
    );
    let frontier = frontier.unwrap();
    assert_eq!(frontier.outcome(), outcome);
    let recovery_transaction =
        uste_graph::decode_transaction(frontier.canonical_request()).unwrap();
    assert_eq!(recovery_transaction, transaction);
    let recovery_candidates =
        load_graph_state_root_candidates_for_recovery(&recovery, &mut filesystem).unwrap();
    let recovery_candidate = recovery_candidates
        .iter()
        .find(|candidate| candidate.revision() == CommitRevision::FIRST)
        .unwrap();
    let mut recovery_admission_cache = PageCache::new(1024 * 1024).unwrap();
    let (recovery_base, _) = admit_graph_disk_base_candidate_for_recovery(
        &recovery,
        &mut filesystem,
        recovery_candidate,
        admission_limits(),
        &mut recovery_admission_cache,
    )
    .unwrap();
    let mut recovery_proof_cache = PageCache::new(1024 * 1024).unwrap();
    let recovery_prepared = load_graph_disk_recovery_preparation_view(
        &recovery,
        &mut filesystem,
        &recovery_base,
        &frontier,
        proof_limits,
        &mut recovery_proof_cache,
    )
    .unwrap()
    .prepare()
    .unwrap();
    let recovery_prepared = prepare_graph_disk_commit(
        recovery_prepared,
        GraphStateDeltaLimits::new(100, 1024 * 1024).unwrap(),
    )
    .unwrap();
    let suffix = frontier.bind_prepared(recovery_prepared);
    drop(recovery);
    let (mut coordinator, reopened_report) = CommitCoordinator::open_journal_anchored_prepared(
        &mut filesystem,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(29_000),
        CounterEntropy(30_000),
        &mut TestKeyAdapter,
        GraphDiskLiveState::new(recovery_base),
        Some(suffix),
    )
    .unwrap();
    assert_eq!(reopened_report.frontier, Some(outcome.revision));
    assert!(
        coordinator
            .reducer_state_for_checkpoint()
            .unwrap()
            .is_pending()
    );

    let retry = commit_graph_disk_live_prepared(
        &mut coordinator,
        &mut filesystem,
        request,
        prepared.clone(),
        &mut clock(2),
        &NeverCancel,
    )
    .unwrap();
    assert_eq!(retry, outcome);
    assert!(matches!(
        commit_graph_disk_live_prepared(
            &mut coordinator,
            &mut filesystem,
            TransactionRequest {
                principal: request.principal,
                idempotency_key: IdempotencyKey::from_bytes([3; 16]),
                transaction_id: TransactionId::from_bytes([35; 16]),
                canonical_request: &encoded,
                blob_inventory: None,
            },
            prepared,
            &mut clock(3),
            &NeverCancel,
        ),
        Err(GraphDiskError::Transaction(
            uste_txn::TransactionError::Conflict
        ))
    ));
    let mut pending_cache = PageCache::new(1024 * 1024).unwrap();
    assert!(matches!(
        load_graph_disk_live_preparation_view(
            &coordinator,
            &mut filesystem,
            transaction.clone(),
            proof_limits,
            &mut pending_cache,
        ),
        Err(GraphDiskError::RootStateMismatch)
    ));

    let base_read = IndexRunReadLimits::new(100, 100, 1024 * 1024).unwrap();
    let merge = IndexRunMergeLimits::new(base_read, 100, 1024 * 1024, 100, 1024 * 1024).unwrap();
    let failed_publication = publish_graph_disk_live_base(
        &mut coordinator,
        &mut filesystem,
        outcome,
        GraphStateRootMergeLimits::uniform(merge, 1).unwrap(),
    );
    assert!(
        matches!(
            &failed_publication,
            Err(GraphDiskError::Transaction(
                uste_txn::TransactionError::Storage(
                    uste_storage::journal::StorageError::ResourceLimit
                )
            ))
        ),
        "unexpected failed-publication result: {failed_publication:?}"
    );
    assert!(
        coordinator
            .reducer_state_for_checkpoint()
            .unwrap()
            .is_pending()
    );
    let (root, report) = publish_graph_disk_live_base(
        &mut coordinator,
        &mut filesystem,
        outcome,
        GraphStateRootMergeLimits::uniform(merge, 2 * 1024 * 1024).unwrap(),
    )
    .unwrap();
    assert_eq!(root.revision(), CommitRevision::new(2).unwrap());
    assert!(report.runs > 0);
    assert!(
        !coordinator
            .reducer_state_for_checkpoint()
            .unwrap()
            .is_pending()
    );

    drop(coordinator);
    filesystem.restart().unwrap();
    let (ready_recovery, ready_report, _) =
        AuthenticatedIndexRecovery::open_with_frontier_transaction(
            &mut filesystem,
            &name,
            scope(),
            CounterEntropy(31_000),
            CounterEntropy(32_000),
            &mut TestKeyAdapter,
        )
        .unwrap();
    assert_eq!(ready_report.frontier, Some(outcome.revision));
    let ready_candidates =
        load_graph_state_root_candidates_for_recovery(&ready_recovery, &mut filesystem).unwrap();
    let ready_candidate = ready_candidates
        .iter()
        .find(|candidate| candidate.revision() == outcome.revision)
        .unwrap();
    let mut ready_admission_cache = PageCache::new(1024 * 1024).unwrap();
    let (ready_base, _) = admit_graph_disk_base_candidate_for_recovery(
        &ready_recovery,
        &mut filesystem,
        ready_candidate,
        admission_limits(),
        &mut ready_admission_cache,
    )
    .unwrap();
    drop(ready_recovery);
    let (coordinator, reopened_ready_report) = CommitCoordinator::open_journal_anchored_prepared(
        &mut filesystem,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(33_000),
        CounterEntropy(34_000),
        &mut TestKeyAdapter,
        GraphDiskLiveState::new(ready_base),
        None,
    )
    .unwrap();
    assert_eq!(reopened_ready_report.frontier, Some(outcome.revision));
    assert!(
        !coordinator
            .reducer_state_for_checkpoint()
            .unwrap()
            .is_pending()
    );

    let next = GraphTransaction::new(
        scope(),
        vec![Operation::ReplaceEntity {
            target: entity,
            expected: Expected::Version(uste_graph::RecordVersion::new(2).unwrap()),
            properties: Value::Bool(false),
        }],
    );
    let mut next_cache = PageCache::new(1024 * 1024).unwrap();
    let next_prepared = load_graph_disk_live_preparation_view(
        &coordinator,
        &mut filesystem,
        next,
        proof_limits,
        &mut next_cache,
    )
    .unwrap()
    .prepare()
    .unwrap();
    assert_eq!(next_prepared.revision(), CommitRevision::new(3).unwrap());
}

#[test]
fn graph_state_delta_root_matches_full_projection_and_reconstructs() {
    let left = record(0x31);
    let right = record(0x32);
    let evidence = record(0x33);
    let relationship = record(0x34);
    let assertion = record(0x35);
    let mut filesystem = MemoryFileSystem::new(32 * 1024 * 1024);
    let name = EntryName::new("graph-state-delta").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope().database(), 40_000),
        CounterEntropy(41_000),
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
                        digest: [0x36; 32],
                        locator: text("fixture://delta"),
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

    let base_snapshot = coordinator.read_view().unwrap().state().clone();
    publish_graph_state_root(&mut coordinator, &mut filesystem, &base_snapshot).unwrap();
    let base_roots = load_graph_state_roots(&coordinator, &mut filesystem, &base_snapshot).unwrap();
    assert_eq!(base_roots.len(), 1);
    let base = &base_roots[0];
    let target_revision = CommitRevision::new(4).unwrap();
    let transaction = GraphTransaction::with_policy_mutation(
        scope(),
        vec![
            Operation::ReplaceEntity {
                target: left,
                expected: Expected::Version(uste_graph::RecordVersion::FIRST),
                properties: Value::RecordRef(right),
            },
            Operation::ActOnRelationship {
                target: relationship,
                expected: Expected::Version(uste_graph::RecordVersion::new(2).unwrap()),
                action: AssertionAction::Retract,
                correction: None,
                correction_expected: None,
            },
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Assertion(NewAssertion {
                    id: assertion,
                    subject: left,
                    predicate: text("tracks"),
                    object: Value::RecordRef(right),
                    evidence: vec![evidence],
                    valid_time: ValidTime::Unknown,
                }),
            },
        ],
        DurablePolicyMutation::Replace {
            expected: PolicyVersion::new(1).unwrap(),
            policy: NamespacePolicy::new(
                scope(),
                PolicyVersion::new(2).unwrap(),
                QuotaLimits::new(200, 2 * 1024 * 1024, 2 * 1024 * 1024, 16, 2048).unwrap(),
            ),
        },
    );
    assert!(matches!(
        prepare_graph_state_root_delta(
            &coordinator,
            base,
            &transaction,
            target_revision,
            GraphStateDeltaLimits::new(1, 1024 * 1024).unwrap(),
        ),
        Err(GraphDiskError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
    let plan = prepare_graph_state_root_delta(
        &coordinator,
        base,
        &transaction,
        target_revision,
        GraphStateDeltaLimits::new(100, 1024 * 1024).unwrap(),
    )
    .unwrap();
    assert_eq!(plan.delta_count(), 19);
    assert!(plan.logical_bytes() > 0);
    assert!(matches!(
        prepare_graph_state_root_delta(
            &coordinator,
            base,
            &transaction,
            target_revision,
            GraphStateDeltaLimits::new(18, 1024 * 1024).unwrap(),
        ),
        Err(GraphDiskError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
    assert!(matches!(
        prepare_graph_state_root_delta(
            &coordinator,
            base,
            &transaction,
            target_revision,
            GraphStateDeltaLimits::new(100, plan.logical_bytes() - 1).unwrap(),
        ),
        Err(GraphDiskError::Storage(
            uste_storage::journal::StorageError::ResourceLimit
        ))
    ));
    let exact_plan = prepare_graph_state_root_delta(
        &coordinator,
        base,
        &transaction,
        target_revision,
        GraphStateDeltaLimits::new(plan.delta_count(), plan.logical_bytes()).unwrap(),
    )
    .unwrap();
    assert_eq!(exact_plan.delta_count(), plan.delta_count());
    assert_eq!(exact_plan.logical_bytes(), plan.logical_bytes());

    let outcome = commit(&mut coordinator, &mut filesystem, 4, transaction);
    let base_read = IndexRunReadLimits::new(100, 100, 1024 * 1024).unwrap();
    let merge = IndexRunMergeLimits::new(base_read, 100, 1024 * 1024, 100, 1024 * 1024).unwrap();
    let merge_limits = GraphStateRootMergeLimits::uniform(merge, 2 * 1024 * 1024).unwrap();
    let mut wrong_outcome = outcome;
    wrong_outcome.result_digest[0] ^= 1;
    assert!(matches!(
        publish_graph_state_root_delta(
            &mut coordinator,
            &mut filesystem,
            base,
            &exact_plan,
            wrong_outcome,
            merge_limits,
        ),
        Err(GraphDiskError::RootStateMismatch)
    ));

    let undersized_merge =
        IndexRunMergeLimits::new(base_read, 1, 1024 * 1024, 100, 1024 * 1024).unwrap();
    assert!(matches!(
        publish_graph_state_root_delta(
            &mut coordinator,
            &mut filesystem,
            base,
            &plan,
            outcome,
            GraphStateRootMergeLimits::uniform(undersized_merge, 2 * 1024 * 1024).unwrap(),
        ),
        Err(GraphDiskError::Transaction(
            uste_txn::TransactionError::Storage(uste_storage::journal::StorageError::ResourceLimit)
        ))
    ));
    let history_limited = GraphStateRootMergeLimits::uniform(merge, 1).unwrap();
    assert!(matches!(
        publish_graph_state_root_delta(
            &mut coordinator,
            &mut filesystem,
            base,
            &plan,
            outcome,
            history_limited,
        ),
        Err(GraphDiskError::Transaction(
            uste_txn::TransactionError::Storage(uste_storage::journal::StorageError::ResourceLimit)
        ))
    ));
    let committed_target = coordinator.read_view().unwrap().state().clone();
    assert_eq!(committed_target.revision(), Some(target_revision));
    assert!(
        load_graph_state_roots(&coordinator, &mut filesystem, &committed_target)
            .unwrap()
            .is_empty()
    );
    drop(coordinator);
    filesystem.restart().unwrap();
    let (reopened_after_failed_merge, recovery) = CommitCoordinator::open(
        &mut filesystem,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(42_000),
        CounterEntropy(43_000),
        &mut TestKeyAdapter,
        GraphState::new(scope()),
    )
    .unwrap();
    assert_eq!(recovery.frontier, Some(target_revision));
    assert_eq!(
        reopened_after_failed_merge.read_view().unwrap().state(),
        &committed_target
    );
    assert!(
        load_graph_state_roots(
            &reopened_after_failed_merge,
            &mut filesystem,
            &committed_target,
        )
        .unwrap()
        .is_empty()
    );
    coordinator = reopened_after_failed_merge;

    let (delta_publication, report) = publish_graph_state_root_delta(
        &mut coordinator,
        &mut filesystem,
        base,
        &plan,
        outcome,
        merge_limits,
    )
    .unwrap();
    assert_eq!(delta_publication.revision(), target_revision);
    assert_eq!(report.runs, 6);
    assert!(report.base_entries > 0);
    assert!(report.replacements > 0);
    assert!(report.insertions > 0);
    assert!(report.deletions > 0);
    assert!(report.pages_read >= 8);

    let stale_base_transaction = GraphTransaction::new(
        scope(),
        vec![Operation::ReplaceEntity {
            target: right,
            expected: Expected::Version(uste_graph::RecordVersion::FIRST),
            properties: Value::Bool(true),
        }],
    );
    assert!(matches!(
        prepare_graph_state_root_delta(
            &coordinator,
            base,
            &stale_base_transaction,
            CommitRevision::new(5).unwrap(),
            GraphStateDeltaLimits::new(100, 1024 * 1024).unwrap(),
        ),
        Err(GraphDiskError::RootStateMismatch)
    ));

    let target = coordinator.read_view().unwrap().state().clone();
    let delta_roots = load_graph_state_roots(&coordinator, &mut filesystem, &target).unwrap();
    assert_eq!(delta_roots.len(), 1);
    assert_eq!(delta_roots[0].generation(), delta_publication.generation());
    let full_publication =
        publish_graph_state_root(&mut coordinator, &mut filesystem, &target).unwrap();
    let equivalent_roots = load_graph_state_roots(&coordinator, &mut filesystem, &target).unwrap();
    assert_eq!(equivalent_roots.len(), 2);
    assert!(
        equivalent_roots
            .iter()
            .any(|root| root.generation() == delta_publication.generation())
    );
    assert!(
        equivalent_roots
            .iter()
            .any(|root| root.generation() == full_publication.generation)
    );

    let candidates = load_graph_state_root_candidates(&coordinator, &mut filesystem).unwrap();
    let delta_candidate = candidates
        .iter()
        .find(|candidate| candidate.generation() == delta_publication.generation())
        .unwrap();
    let (reconstructed, reconstruction) = reconstruct_graph_state_candidate(
        &coordinator,
        &mut filesystem,
        delta_candidate,
        GraphStateLoadLimits::new(20, 40, 10, 200, 200, 2 * 1024 * 1024).unwrap(),
    )
    .unwrap();
    assert_eq!(reconstructed.snapshot(), target);
    assert_eq!(reconstruction.runs, 6);

    drop(coordinator);
    filesystem.restart().unwrap();
    let (reopened, recovery) = CommitCoordinator::open(
        &mut filesystem,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(44_000),
        CounterEntropy(45_000),
        &mut TestKeyAdapter,
        GraphState::new(scope()),
    )
    .unwrap();
    assert_eq!(recovery.frontier, Some(target_revision));
    assert_eq!(reopened.read_view().unwrap().state(), &target);
    assert_eq!(
        load_graph_state_roots(&reopened, &mut filesystem, &target)
            .unwrap()
            .len(),
        2
    );
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
    let quotas = QuotaLimits::new(100, 1024 * 1024, 1024 * 1024, 8, 1024).unwrap();
    let mut metadata_policy = NamespacePolicy::new(scope(), PolicyVersion::new(1).unwrap(), quotas);
    for (digest, actions) in [
        (
            0x90,
            vec![
                uste_policy::Action::ReadOwnOutcome,
                uste_policy::Action::InspectQuota,
                uste_policy::Action::ReadRecord,
                uste_policy::Action::ReadHistory,
            ],
        ),
        (
            0x91,
            vec![
                uste_policy::Action::ReadOwnOutcome,
                uste_policy::Action::ReadRecord,
            ],
        ),
    ] {
        let mut grant = uste_policy::NamespaceGrant::new(
            uste_policy::PermissionSet::from_actions(actions),
            quotas,
        );
        if digest == 0x91 {
            grant
                .deny_record(
                    entity.record(),
                    uste_policy::PermissionSet::from_actions([uste_policy::Action::ReadRecord]),
                )
                .unwrap();
        }
        metadata_policy
            .grant(
                uste_policy::PrincipalDigest::from_bytes([digest; 32]),
                grant,
            )
            .unwrap();
    }
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
                policy: metadata_policy.clone(),
            },
        ),
    );
    let checkpoint_snapshot = coordinator.read_view().unwrap().state().clone();
    let graph_publication =
        publish_graph_state_root(&mut coordinator, &mut filesystem, &checkpoint_snapshot).unwrap();
    let metadata_publication =
        publish_coordinator_metadata_root(&mut coordinator, &mut filesystem).unwrap();
    assert_eq!(graph_publication.revision, metadata_publication.revision);
    uste_txn::publish_coordinator_transaction_index(&mut coordinator, &mut filesystem).unwrap();

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
    let mut reference_state = coordinator.reducer_state_for_checkpoint().unwrap().clone();
    let newer_graph_publication =
        publish_graph_state_root(&mut coordinator, &mut filesystem, &expected).unwrap();
    uste_txn::publish_coordinator_transaction_index(&mut coordinator, &mut filesystem).unwrap();
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
    let mut streamed_revisions = Vec::new();
    recovery
        .visit_transactions(
            &mut filesystem,
            CommitRevision::FIRST,
            CommitRevision::new(2).unwrap(),
            2,
            1_000_000,
            |filesystem, transaction| {
                // Derived-index reads and journal validation share one exclusive recovery owner.
                recovery
                    .load_index_root_manifests(
                        filesystem,
                        uste_txn::COORDINATOR_METADATA_PROFILE_V1,
                    )
                    .unwrap();
                assert_eq!(
                    uste_graph::decode_transaction(transaction.canonical_request())
                        .unwrap()
                        .scope(),
                    scope()
                );
                streamed_revisions.push(transaction.revision().get());
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(streamed_revisions, vec![1, 2]);
    let transaction_root = recovery
        .load_index_root_manifests(
            &mut filesystem,
            uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
        )
        .unwrap()
        .remove(0);
    let mut metadata_cache = PageCache::new(64 * 1024).unwrap();
    let lookup_limits = uste_storage::IndexGetLimits::new(16, 136).unwrap();
    let transaction_index = uste_txn::admit_coordinator_transaction_index_for_recovery(
        &recovery,
        &mut filesystem,
        transaction_root,
        uste_txn::CoordinatorTransactionAdmissionLimits {
            run: IndexRunReadLimits::new(16, 2, 4096).unwrap(),
            lookup: lookup_limits,
            maximum_groups: 2,
            maximum_encoded_bytes: 1_000_000,
        },
        &mut metadata_cache,
    )
    .unwrap();
    let metadata_candidate =
        load_coordinator_metadata_candidates_for_recovery::<GraphState, _, _, _, _>(
            &recovery,
            &mut filesystem,
        )
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.generation() == newer_metadata_publication.generation)
        .unwrap();
    let expected_anchor = transaction_index.anchor();
    let metadata_base = uste_txn::admit_coordinator_disk_base(
        &recovery,
        &mut filesystem,
        metadata_candidate,
        transaction_index,
        uste_txn::CoordinatorDiskAdmissionLimits {
            metadata: CoordinatorMetadataLoadLimits::new(2, 0, 3, 16, 4096).unwrap(),
            lookup: lookup_limits,
            maximum_total_journal_groups: 2,
            maximum_encoded_bytes_per_pass: 1_000_000,
        },
        &mut metadata_cache,
    )
    .unwrap();
    assert_eq!(metadata_base.anchor(), expected_anchor);
    let graph_candidates =
        load_graph_state_root_candidates_for_recovery(&recovery, &mut filesystem).unwrap();
    let graph_candidate = graph_candidates
        .iter()
        .find(|candidate| candidate.generation() == graph_publication.generation)
        .unwrap();
    let mut admission_cache = PageCache::new(INDEX_PAGE_BYTES * 4).unwrap();
    let (cold_base, admission) = admit_graph_disk_base_candidate_for_recovery(
        &recovery,
        &mut filesystem,
        graph_candidate,
        admission_limits(),
        &mut admission_cache,
    )
    .unwrap();
    assert_eq!(cold_base.anchor(), graph_candidate.anchor());
    assert_eq!(cold_base.revision(), CommitRevision::FIRST);
    assert_eq!(
        cold_base.namespace_policy(),
        checkpoint_snapshot.namespace_policy()
    );
    assert!(admission.scan.runs >= 4);
    assert!(admission.exact_lookups > 0);
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
    let current_candidate = graph_candidates
        .iter()
        .find(|candidate| candidate.generation() == newer_graph_publication.generation)
        .unwrap();
    let (current_base, _) = admit_graph_disk_base_candidate_for_recovery(
        &recovery,
        &mut filesystem,
        current_candidate,
        admission_limits(),
        &mut admission_cache,
    )
    .unwrap();
    let disk = uste_txn::DiskCommitCoordinator::recover_from_admitted_base(
        recovery,
        &mut filesystem,
        metadata_base,
        GraphDiskLiveState::new(current_base),
        RetentionDays::new(30).unwrap(),
        uste_txn::DiskCoordinatorRecoveryLimits {
            overlay: uste_txn::CoordinatorRecoveryLimits::new(0, 0).unwrap(),
            lookup: lookup_limits,
            maximum_encoded_bytes: 0,
        },
        &mut metadata_cache,
    )
    .unwrap();
    assert_eq!(disk.overlay_counts(), (0, 0));
    assert_eq!(disk.state().unwrap().revision().get(), 2);
    assert_authorized_disk_metadata(&disk, &mut filesystem, &metadata_policy);
    drop(disk);

    // An older metadata pair can accompany either a ready newer graph base or one pending
    // externally prepared transaction. Recovery retains only post-metadata-base overlays.
    for case in 0..7 {
        let (recovery, _, frontier) = AuthenticatedIndexRecovery::open_with_frontier_transaction(
            &mut filesystem,
            &name,
            scope(),
            CounterEntropy(50_000 + case * 2000),
            CounterEntropy(51_000 + case * 2000),
            &mut TestKeyAdapter,
        )
        .unwrap();
        let transaction_root = recovery
            .load_index_root_manifests(
                &mut filesystem,
                uste_txn::COORDINATOR_TRANSACTION_PROFILE_V1,
            )
            .unwrap()
            .into_iter()
            .find(|root| root.revision() == CommitRevision::FIRST)
            .unwrap();
        let transaction_index = uste_txn::admit_coordinator_transaction_index_for_recovery(
            &recovery,
            &mut filesystem,
            transaction_root,
            uste_txn::CoordinatorTransactionAdmissionLimits {
                run: IndexRunReadLimits::new(16, 2, 4096).unwrap(),
                lookup: lookup_limits,
                maximum_groups: 1,
                maximum_encoded_bytes: 1_000_000,
            },
            &mut metadata_cache,
        )
        .unwrap();
        let candidate =
            load_coordinator_metadata_candidates_for_recovery::<GraphState, _, _, _, _>(
                &recovery,
                &mut filesystem,
            )
            .unwrap()
            .into_iter()
            .find(|root| root.revision() == CommitRevision::FIRST)
            .unwrap();
        let metadata_base = uste_txn::admit_coordinator_disk_base(
            &recovery,
            &mut filesystem,
            candidate,
            transaction_index,
            uste_txn::CoordinatorDiskAdmissionLimits {
                metadata: CoordinatorMetadataLoadLimits::new(2, 0, 3, 16, 4096).unwrap(),
                lookup: lookup_limits,
                maximum_total_journal_groups: 1,
                maximum_encoded_bytes_per_pass: 1_000_000,
            },
            &mut metadata_cache,
        )
        .unwrap();
        let candidates =
            load_graph_state_root_candidates_for_recovery(&recovery, &mut filesystem).unwrap();
        let old_candidate = candidates
            .iter()
            .find(|root| root.revision() == CommitRevision::FIRST)
            .unwrap();
        let (old_base, _) = admit_graph_disk_base_candidate_for_recovery(
            &recovery,
            &mut filesystem,
            old_candidate,
            admission_limits(),
            &mut admission_cache,
        )
        .unwrap();
        let recovered_outcome = frontier.as_ref().unwrap().outcome();
        let suffix = if case == 1 || case == 4 {
            None
        } else {
            let frontier = frontier.unwrap();
            let prepared = load_graph_disk_recovery_preparation_view(
                &recovery,
                &mut filesystem,
                &old_base,
                &frontier,
                GraphDiskPreparationLimits::new(8, 8, 8, 8, 1024 * 1024).unwrap(),
                &mut admission_cache,
            )
            .unwrap()
            .prepare()
            .unwrap();
            Some(
                frontier.bind_prepared(
                    prepare_graph_disk_commit(
                        prepared,
                        GraphStateDeltaLimits::new(100, 1024 * 1024).unwrap(),
                    )
                    .unwrap(),
                ),
            )
        };
        let domain_base = if case == 1 || case == 5 {
            let current = candidates
                .iter()
                .find(|root| root.revision().get() == 2)
                .unwrap();
            admit_graph_disk_base_candidate_for_recovery(
                &recovery,
                &mut filesystem,
                current,
                admission_limits(),
                &mut admission_cache,
            )
            .unwrap()
            .0
        } else {
            old_base
        };
        let result = uste_txn::DiskCommitCoordinator::recover_with_prepared_suffix(
            recovery,
            &mut filesystem,
            metadata_base,
            GraphDiskLiveState::new(domain_base),
            suffix,
            RetentionDays::new(30).unwrap(),
            uste_txn::DiskCoordinatorRecoveryLimits {
                overlay: uste_txn::CoordinatorRecoveryLimits::new(if case == 2 { 0 } else { 1 }, 0)
                    .unwrap(),
                lookup: lookup_limits,
                maximum_encoded_bytes: if case == 3 { 1 } else { 1_000_000 },
            },
            &mut metadata_cache,
        );
        match case {
            0 | 1 | 6 => {
                let mut disk = result.unwrap();
                assert_eq!(disk.overlay_counts(), (1, 0));
                assert_eq!(disk.state().unwrap().revision().get(), 2);
                assert_eq!(disk.state().unwrap().is_pending(), case != 1);
                if case == 1 {
                    assert_authorized_disk_metadata(&disk, &mut filesystem, &metadata_policy);
                } else {
                    let mut kernel = uste_policy::PolicyKernel::new();
                    kernel
                        .install_initial_policy(metadata_policy.clone())
                        .unwrap();
                    assert!(matches!(
                        uste_txn::AuthorizedDiskMetadata::new(&disk, &kernel),
                        Err(uste_txn::AuthorizedError::InvalidPolicy)
                    ));
                }
                if case == 6 {
                    let read = IndexRunReadLimits::new(100, 100, 1024 * 1024).unwrap();
                    let merge =
                        IndexRunMergeLimits::new(read, 100, 1024 * 1024, 100, 1024 * 1024).unwrap();
                    assert!(
                        uste_graph::publish_graph_disk_coordinator_base(
                            &mut disk,
                            &mut filesystem,
                            recovered_outcome,
                            GraphStateRootMergeLimits::uniform(merge, 1).unwrap(),
                        )
                        .is_err()
                    );
                    assert!(disk.state().unwrap().is_pending());
                    let (root, _) = uste_graph::publish_graph_disk_coordinator_base(
                        &mut disk,
                        &mut filesystem,
                        recovered_outcome,
                        GraphStateRootMergeLimits::uniform(merge, 2 * 1024 * 1024).unwrap(),
                    )
                    .unwrap();
                    assert_eq!(root.revision().get(), 2);
                    assert!(!disk.state().unwrap().is_pending());
                    disk.rebase_metadata(
                        &mut filesystem,
                        uste_txn::CoordinatorMetadataRebaseLimits { merge, reuse: read },
                    )
                    .unwrap();
                    assert_eq!(disk.overlay_counts(), (0, 0));
                    assert_authorized_disk_metadata(&disk, &mut filesystem, &metadata_policy);
                    for revision in [3_u8, 4] {
                        let transaction = GraphTransaction::new(
                            scope(),
                            vec![Operation::ReplaceEntity {
                                target: entity,
                                expected: Expected::Version(
                                    uste_graph::RecordVersion::new(u64::from(revision - 1))
                                        .unwrap(),
                                ),
                                properties: if revision == 3 {
                                    Value::RecordRef(entity)
                                } else {
                                    Value::Bool(true)
                                },
                            }],
                        );
                        let encoded = encode_transaction(&transaction).unwrap();
                        assert!(matches!(
                            uste_graph::load_graph_disk_coordinator_preparation_view(
                                &disk,
                                &mut filesystem,
                                transaction.clone(),
                                GraphDiskPreparationLimits::new(8, 8, 8, 8, 1).unwrap(),
                                &mut admission_cache,
                            ),
                            Err(GraphDiskError::Storage(
                                uste_storage::journal::StorageError::ResourceLimit
                            ))
                        ));
                        assert_eq!(disk.overlay_counts(), (0, 0));
                        assert!(!disk.state().unwrap().is_pending());
                        let prepared = uste_graph::load_graph_disk_coordinator_preparation_view(
                            &disk,
                            &mut filesystem,
                            transaction.clone(),
                            GraphDiskPreparationLimits::new(8, 8, 8, 8, 1024 * 1024).unwrap(),
                            &mut admission_cache,
                        )
                        .unwrap()
                        .prepare()
                        .unwrap();
                        let prepared = prepare_graph_disk_commit(
                            prepared,
                            GraphStateDeltaLimits::new(100, 1024 * 1024).unwrap(),
                        )
                        .unwrap();
                        let request = TransactionRequest {
                            principal: uste_policy::PrincipalDigest::from_bytes([0x90; 32]),
                            idempotency_key: IdempotencyKey::from_bytes([revision; 16]),
                            transaction_id: TransactionId::from_bytes([revision + 32; 16]),
                            canonical_request: &encoded,
                            blob_inventory: None,
                        };
                        let outcome = disk
                            .commit_prepared(
                                &mut filesystem,
                                request,
                                prepared.clone(),
                                &mut clock(u64::from(revision)),
                                &NeverCancel,
                                lookup_limits,
                                &mut metadata_cache,
                            )
                            .unwrap();
                        let reference = uste_txn::TransactionState::prepare(
                            &reference_state,
                            &encoded,
                            None,
                            CommitRevision::new(u64::from(revision)).unwrap(),
                        )
                        .unwrap();
                        assert_eq!(
                            outcome.result_digest,
                            <GraphState as uste_txn::TransactionState>::result_digest(&reference)
                        );
                        uste_txn::TransactionState::publish(&mut reference_state, reference);
                        assert_eq!(
                            disk.commit_prepared(
                                &mut filesystem,
                                request,
                                prepared,
                                &mut clock(u64::from(revision)),
                                &NeverCancel,
                                lookup_limits,
                                &mut metadata_cache
                            )
                            .unwrap(),
                            outcome
                        );
                        assert!(matches!(
                            uste_graph::load_graph_disk_coordinator_preparation_view(
                                &disk,
                                &mut filesystem,
                                transaction,
                                GraphDiskPreparationLimits::new(8, 8, 8, 8, 1024 * 1024).unwrap(),
                                &mut admission_cache,
                            ),
                            Err(GraphDiskError::RootStateMismatch)
                        ));
                        uste_graph::publish_graph_disk_coordinator_base(
                            &mut disk,
                            &mut filesystem,
                            outcome,
                            GraphStateRootMergeLimits::uniform(merge, 2 * 1024 * 1024).unwrap(),
                        )
                        .unwrap();
                        if revision == 3 {
                            let mut filtered = 0;
                            let result =
                                <GraphDiskLiveState as uste_txn::AuthorizedDiskReadState<
                                    MemoryFileSystem,
                                    TestEnvelope,
                                    CounterEntropy,
                                    CounterEntropy,
                                >>::read_disk_authorized(
                                    &disk,
                                    &mut filesystem,
                                    &uste_graph::GraphReadRequest::Record { id: entity },
                                    &uste_graph::GraphDiskReadLimits {
                                        expansion: None,
                                        current: uste_storage::IndexGetLimits::new(64, 4096)
                                            .unwrap(),
                                        historical: IndexPredecessorLimits::new(64, 4096).unwrap(),
                                    },
                                    &mut admission_cache,
                                    &mut |action, target| {
                                        assert_eq!(action, uste_policy::Action::ReadRecord);
                                        assert_eq!(target, uste_policy::Target::Record(entity));
                                        filtered += 1;
                                        false
                                    },
                                )
                                .unwrap();
                            assert_eq!(filtered, 1);
                            assert_eq!(result, uste_graph::GraphReadOutput::Record(None));
                        }
                        disk.rebase_metadata(
                            &mut filesystem,
                            uste_txn::CoordinatorMetadataRebaseLimits { merge, reuse: read },
                        )
                        .unwrap();
                        assert_eq!(disk.overlay_counts(), (0, 0));
                    }
                }
            }
            2 => assert!(matches!(
                result,
                Err(uste_txn::TransactionError::ResourceLimit)
            )),
            3 => assert!(matches!(
                result,
                Err(uste_txn::TransactionError::Storage(
                    uste_storage::journal::StorageError::ResourceLimit
                ))
            )),
            _ => assert!(matches!(
                result,
                Err(uste_txn::TransactionError::IntegrityFailure)
            )),
        }
    }

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
    assert_eq!(seeded_report.frontier.unwrap().get(), 4);
    assert_eq!(
        recovered.read_view().unwrap().state(),
        &uste_txn::TransactionState::snapshot(&reference_state)
    );
    assert_eq!(recovered.checkpoint_outcomes().len(), 4);
    let final_candidates = load_graph_state_root_candidates(&recovered, &mut filesystem).unwrap();
    let final_candidate = final_candidates
        .iter()
        .find(|root| root.revision().get() == 4)
        .unwrap();
    let (final_root_state, _) = reconstruct_graph_state_candidate(
        &recovered,
        &mut filesystem,
        final_candidate,
        state_load_limits(),
    )
    .unwrap();
    assert_eq!(
        uste_txn::TransactionState::snapshot(&final_root_state),
        uste_txn::TransactionState::snapshot(&reference_state)
    );
}

fn assert_authorized_disk_metadata(
    disk: &uste_txn::DiskCommitCoordinator<
        GraphDiskLiveState,
        MemoryFileSystem,
        TestEnvelope,
        CounterEntropy,
        CounterEntropy,
    >,
    filesystem: &mut MemoryFileSystem,
    policy: &NamespacePolicy,
) {
    struct Identity;
    impl uste_policy::TrustedPrincipalAdapter for Identity {
        type Credential = u8;
        fn authenticate(
            &mut self,
            credential: &u8,
        ) -> Result<uste_policy::PrincipalDigest, uste_policy::AuthenticationError> {
            Ok(uste_policy::PrincipalDigest::from_bytes([*credential; 32]))
        }
    }
    let mut kernel = uste_policy::PolicyKernel::new();
    kernel.install_initial_policy(policy.clone()).unwrap();
    let alice = kernel.authenticate(&mut Identity, &0x90).unwrap();
    let bob = kernel.authenticate(&mut Identity, &0x91).unwrap();
    let denied = kernel.authenticate(&mut Identity, &0x92).unwrap();
    let foreign_kernel = uste_policy::PolicyKernel::new();
    let foreign = foreign_kernel.authenticate(&mut Identity, &0x90).unwrap();
    let facade = uste_txn::AuthorizedDiskMetadata::new(disk, &kernel).unwrap();
    let read_limits = uste_graph::GraphDiskReadLimits {
        expansion: None,
        current: uste_storage::IndexGetLimits::new(64, 4096).unwrap(),
        historical: IndexPredecessorLimits::new(64, 4096).unwrap(),
    };
    let reader = uste_txn::AuthorizedDiskReader::new(disk, &kernel, read_limits).unwrap();
    let current = uste_graph::GraphReadRequest::Record { id: record(0x21) };
    let historical = uste_graph::GraphReadRequest::RecordAt {
        id: record(0x21),
        revision: CommitRevision::FIRST,
    };
    assert!(matches!(
        reader.read(filesystem, &bob, &current, &NeverCancel),
        Err(uste_txn::AuthorizedReadError::Authorization(
            uste_txn::AuthorizedError::Unauthorized
        ))
    ));
    let foreign_record = RecordRef::new(
        scope().database(),
        NamespaceId::from_bytes([0xee; 16]),
        record(0x21).record(),
    );
    assert!(matches!(
        reader.read(
            filesystem,
            &alice,
            &uste_graph::GraphReadRequest::Record { id: foreign_record },
            &NeverCancel
        ),
        Err(uste_txn::AuthorizedReadError::Authorization(
            uste_txn::AuthorizedError::Unauthorized
        ))
    ));
    for (request, properties) in [(&current, Value::Bool(true)), (&historical, Value::Null)] {
        let output = reader
            .read(filesystem, &alice, request, &NeverCancel)
            .unwrap();
        let uste_graph::GraphReadOutput::Record(Some(found)) = output else {
            panic!("missing record")
        };
        let Record::Entity(found) = *found else {
            panic!("wrong record type")
        };
        assert_eq!(found.id, record(0x21));
        assert_eq!(found.properties, properties);
    }
    for principal in [&bob, &denied, &foreign] {
        assert!(matches!(
            reader.read(filesystem, principal, &historical, &NeverCancel),
            Err(uste_txn::AuthorizedReadError::Authorization(
                uste_txn::AuthorizedError::Unauthorized
            ))
        ));
    }
    assert_eq!(
        reader
            .read(
                filesystem,
                &alice,
                &uste_graph::GraphReadRequest::Record { id: record(0x99) },
                &NeverCancel
            )
            .unwrap(),
        uste_graph::GraphReadOutput::Record(None)
    );
    assert!(matches!(
        reader.read(
            filesystem,
            &alice,
            &uste_graph::GraphReadRequest::RecordAt {
                id: record(0x21),
                revision: CommitRevision::new(3).unwrap()
            },
            &NeverCancel
        ),
        Err(uste_txn::AuthorizedReadError::Domain(
            GraphDiskError::Graph(GraphError::UnknownReadView(_))
        ))
    ));
    struct CancelAt {
        calls: std::cell::Cell<usize>,
        at: usize,
    }
    impl uste_txn::Cancellation for CancelAt {
        fn is_cancelled(&self) -> bool {
            self.calls.set(self.calls.get() + 1);
            self.calls.get() >= self.at
        }
    }
    for at in [1, 2] {
        assert!(matches!(
            reader.read(
                filesystem,
                &alice,
                &current,
                &CancelAt {
                    calls: std::cell::Cell::new(0),
                    at
                }
            ),
            Err(uste_txn::AuthorizedReadError::Authorization(
                uste_txn::AuthorizedError::Transaction(uste_txn::TransactionError::Cancelled)
            ))
        ));
    }
    let tiny = uste_txn::AuthorizedDiskReader::new(
        disk,
        &kernel,
        uste_graph::GraphDiskReadLimits {
            current: uste_storage::IndexGetLimits::new(64, 1).unwrap(),
            ..read_limits
        },
    )
    .unwrap();
    assert!(matches!(
        tiny.read(filesystem, &alice, &current, &NeverCancel),
        Err(uste_txn::AuthorizedReadError::Domain(
            GraphDiskError::Transaction(uste_txn::TransactionError::Storage(
                uste_storage::journal::StorageError::ResourceLimit
            ))
        ))
    ));
    // Missing and another principal's transactions stay indistinguishable, including expiry.
    for now in [3, 10_000_000] {
        for transaction in [
            TransactionId::from_bytes([33; 16]),
            TransactionId::from_bytes([99; 16]),
        ] {
            assert_eq!(
                facade
                    .transaction_outcome(filesystem, &bob, transaction, &mut clock(now))
                    .unwrap(),
                None
            );
        }
    }
    for revision in [1_u8, 2] {
        let key = IdempotencyKey::from_bytes([revision; 16]);
        let transaction = TransactionId::from_bytes([revision + 32; 16]);
        let outcome = facade
            .outcome(filesystem, &alice, key, &mut clock(3))
            .unwrap()
            .unwrap();
        assert_eq!(outcome.revision.get(), u64::from(revision));
        assert_eq!(
            facade
                .transaction_outcome(filesystem, &alice, transaction, &mut clock(3),)
                .unwrap(),
            Some(outcome)
        );
        assert_eq!(
            facade
                .outcome(filesystem, &bob, key, &mut clock(3))
                .unwrap(),
            None
        );
        assert_eq!(
            facade
                .transaction_outcome(filesystem, &bob, transaction, &mut clock(3),)
                .unwrap(),
            None
        );
        for principal in [&denied, &foreign] {
            let mut unused_clock = ScriptedClock::new([]);
            assert_eq!(
                facade.outcome(filesystem, principal, key, &mut unused_clock,),
                Err(uste_txn::AuthorizedError::Unauthorized)
            );
            assert_eq!(
                facade.transaction_outcome(filesystem, principal, transaction, &mut unused_clock,),
                Err(uste_txn::AuthorizedError::Unauthorized)
            );
        }
        assert_eq!(
            facade.outcome(filesystem, &alice, key, &mut clock(10_000_000),),
            Err(uste_txn::AuthorizedError::Transaction(
                uste_txn::TransactionError::IdempotencyExpired
            ))
        );
    }
    let accounting = uste_txn::DiskBlobAccountingLimits {
        base: IndexRunReadLimits::new(1, 1, 1).unwrap(),
        maximum_total_owners: 0,
    };
    assert_eq!(
        facade
            .committed_blob_usage(filesystem, &alice, accounting)
            .unwrap(),
        uste_txn::CommittedBlobUsage::default()
    );
    assert_eq!(
        facade.committed_blob_usage(filesystem, &bob, accounting),
        Err(uste_txn::AuthorizedError::Unauthorized)
    );
    assert!(matches!(
        uste_txn::AuthorizedDiskMetadata::new(disk, &foreign_kernel),
        Err(uste_txn::AuthorizedError::InvalidPolicy)
    ));
    let mut stale = uste_policy::PolicyKernel::new();
    stale
        .install_initial_policy(NamespacePolicy::new(
            scope(),
            PolicyVersion::new(2).unwrap(),
            QuotaLimits::new(100, 1024 * 1024, 1024 * 1024, 8, 1024).unwrap(),
        ))
        .unwrap();
    assert!(matches!(
        uste_txn::AuthorizedDiskMetadata::new(disk, &stale),
        Err(uste_txn::AuthorizedError::InvalidPolicy)
    ));
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
