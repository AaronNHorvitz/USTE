use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
use uste_graph::{
    AdjacencyDirection, AssertionAction, Expected, GraphDiskError, GraphState, GraphTransaction,
    NewEntity, NewEvidence, NewRecord, NewRelationship, Operation, Record, ValidTime,
    disk_adjacent_ids, disk_record, disk_supported_ids, encode_transaction,
    load_current_graph_index_roots, publish_current_graph_index, scrub_current_graph_index,
};
use uste_storage::{
    ClockObservation, EntryName, INDEX_PAGE_BYTES, IndexEntry, IndexRootInput, PageCache,
    fault::ScriptedClock, journal::DurableKeyEnvelope, memory::MemoryFileSystem,
};
use uste_txn::{
    CheckpointState, CommitCoordinator, NeverCancel, RetentionDays, TransactionRequest,
};
use uste_types::{
    BoundedString, DatabaseId, IdempotencyKey, NamespaceId, NamespaceRef, RecordId, RecordRef,
    TransactionId, UtcInstant, Value,
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
        GraphTransaction::new(
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
    assert!(
        load_current_graph_index_roots(&coordinator, &mut filesystem, &newer)
            .unwrap()
            .is_empty()
    );
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
