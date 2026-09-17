use uste_graph::{
    DeletePolicy, DurablePolicyMutation, Expected, GraphTransaction, NewEntity, NewEvidence,
    NewRecord, Operation, RecordVersion, encode_transaction as encode_graph_transaction,
};
use uste_policy::{
    Action, AuthenticationError, NamespaceGrant, NamespacePolicy, PermissionSet, PolicyKernel,
    PolicyVersion, PrincipalDigest, QuotaLimits, Target, TrustedPrincipalAdapter,
};
use uste_replay::{capture_reducer_checkpoint, verify_reducer_checkpoint};
use uste_spatial::{
    CoordinateSystem, FrameDefinition, SpatialRecord, SpatialTransaction, SpatialVersion,
    VersionedRecordRef, WorldDefinition, encode_transaction as encode_spatial_transaction,
};
use uste_storage::{
    BlobId, BlobInventory, BlobReference, ClockObservation, EntryName, fault::ScriptedClock,
    journal::DurableKeyEnvelope, memory::MemoryFileSystem,
};
use uste_txn::{
    ApplyError, AuthorizedCoordinator, AuthorizedError, AuthorizedTransactionRequest,
    AuthorizedTransactionState, CheckpointState, CommitCoordinator, NeverCancel, RetentionDays,
    TransactionError, TransactionRequest, TransactionState,
};
use uste_types::{
    BoundedString, CommitRevision, DatabaseId, IdempotencyKey, NamespaceId, NamespaceRef, RecordId,
    RecordRef, SourceEventId, SourceEventRef, TransactionId, UtcInstant, Value,
};

use uste_ingest::{
    EngineTransaction, ImportBatch, ImportBatchId, ImportBatchOutcome, ImportCursor, ImportError,
    ImportReadOutput, ImportReadRequest, ImportStart, IngestState, MappedRowReceipt,
    MappingBinding, MappingProfile, SourceBinding, decode_transaction, encode_transaction,
};

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([71; 16]),
        NamespaceId::from_bytes([72; 16]),
    )
}

fn record(value: u8) -> RecordRef {
    RecordRef::new(
        scope().database(),
        scope().namespace(),
        RecordId::from_bytes([value; 16]),
    )
}

fn event(value: u8) -> SourceEventRef {
    SourceEventRef::new(
        scope().database(),
        scope().namespace(),
        SourceEventId::from_bytes([value; 16]),
    )
}

fn text(value: &str) -> BoundedString {
    BoundedString::new(value.to_owned()).unwrap()
}

fn blob(value: u8) -> BlobReference {
    BlobReference::new(scope(), BlobId::from_bytes([value; 16]), 1, 1, [value; 32]).unwrap()
}

fn graph_create(ids: &[u8], evidence: &[(u8, [u8; 32])]) -> Vec<u8> {
    let mut operations = Vec::new();
    for id in ids {
        operations.push(Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Entity(NewEntity {
                id: record(*id),
                entity_type: text("import-record"),
                schema_version: 1,
                properties: Value::Null,
            }),
        });
    }
    for (id, digest) in evidence {
        operations.push(Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Evidence(NewEvidence {
                id: record(*id),
                digest: *digest,
                locator: text("local:fixture"),
            }),
        });
    }
    encode_graph_transaction(&GraphTransaction::new(scope(), operations)).unwrap()
}

fn start_request() -> (EngineTransaction, BlobInventory) {
    let source = blob(91);
    let mapping = blob(92);
    start_request_for(source, mapping)
}

fn start_request_for(
    source: BlobReference,
    mapping: BlobReference,
) -> (EngineTransaction, BlobInventory) {
    let transaction = EngineTransaction::start_import(
        scope(),
        ImportStart::new(
            record(1),
            SourceBinding::new(record(2), 1, source).unwrap(),
            MappingBinding::new(record(3), 1, mapping, MappingProfile::TypedRecordsV1).unwrap(),
            graph_create(
                &[1, 10, 11],
                &[(2, source.content_digest()), (3, mapping.content_digest())],
            ),
        )
        .unwrap(),
    );
    let inventory = BlobInventory::new(scope(), [source, mapping]).unwrap();
    (transaction, inventory)
}

fn clock(day: i64) -> ScriptedClock {
    ScriptedClock::new([Ok(ClockObservation {
        wall_utc: UtcInstant::new(day * 86_400, 0).unwrap(),
        monotonic_ticks: u64::try_from(day).unwrap(),
    })])
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

fn vault(seed: u64) -> KeyVault<TestEnvelope, CounterEntropy> {
    KeyVault::create(
        scope().database(),
        &mut TestKeyAdapter,
        CounterEntropy(seed),
    )
    .unwrap()
}

struct AuthAdapter;

impl TrustedPrincipalAdapter for AuthAdapter {
    type Credential = u8;

    fn authenticate(
        &mut self,
        credential: &Self::Credential,
    ) -> Result<PrincipalDigest, AuthenticationError> {
        Ok(PrincipalDigest::from_bytes([*credential; 32]))
    }
}

fn import_policy() -> NamespacePolicy {
    let quotas = QuotaLimits::new(1_048_576, 1_048_576, 1_048_576, 2, 1_048_576).unwrap();
    let permissions =
        PermissionSet::from_actions([Action::ReadRecord, Action::ReadBlob, Action::Import]);
    let mut policy = NamespacePolicy::new(scope(), PolicyVersion::new(1).unwrap(), quotas);
    let mut restricted = NamespaceGrant::new(permissions, quotas);
    restricted
        .deny_record(
            record(2).record(),
            PermissionSet::from_actions([Action::ReadRecord]),
        )
        .unwrap();
    policy
        .grant(PrincipalDigest::from_bytes([7; 32]), restricted)
        .unwrap();
    policy
        .grant(
            PrincipalDigest::from_bytes([8; 32]),
            NamespaceGrant::new(permissions, quotas),
        )
        .unwrap();
    policy
}

fn publish(
    state: &mut IngestState,
    transaction: &EngineTransaction,
    inventory: Option<&BlobInventory>,
    revision: u64,
) -> ImportBatchOutcome {
    let encoded = encode_transaction(transaction).unwrap();
    let prepared = state
        .prepare(&encoded, inventory, CommitRevision::new(revision).unwrap())
        .unwrap();
    let outcome = prepared.outcome().clone();
    state.publish(prepared);
    outcome
}

fn spatial_world(world: u8, frame: u8) -> Vec<u8> {
    let frame_reference = VersionedRecordRef::new(record(frame), SpatialVersion::FIRST);
    encode_spatial_transaction(
        &SpatialTransaction::new(
            scope(),
            vec![
                SpatialRecord::World(
                    WorldDefinition::new(record(world), SpatialVersion::FIRST, frame_reference)
                        .unwrap(),
                ),
                SpatialRecord::Frame(
                    FrameDefinition::new(
                        record(frame),
                        SpatialVersion::FIRST,
                        record(world),
                        CoordinateSystem::LocalCartesian2,
                        None,
                    )
                    .unwrap(),
                ),
            ],
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn preview_batch_and_checkpoint_are_atomic_and_resumable() {
    let (start, inventory) = start_request();
    let start_bytes = encode_transaction(&start).unwrap();
    assert_eq!(decode_transaction(&start_bytes).unwrap(), start);
    for end in 0..start_bytes.len() {
        assert!(
            decode_transaction(&start_bytes[..end]).is_err(),
            "cut {end}"
        );
    }

    let mut state = IngestState::new(scope());
    let preview = state.preview(&start, Some(&inventory)).unwrap();
    assert_eq!(preview.base_revision(), None);
    assert_eq!(preview.proposed_revision(), CommitRevision::FIRST);
    assert_eq!(state.snapshot().revision(), None);

    let checkpoint = match publish(&mut state, &start, Some(&inventory), 1) {
        ImportBatchOutcome::JobStarted(value) => value,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert_eq!(checkpoint.next_batch(), 1);

    let delete_job = EngineTransaction::graph(
        scope(),
        encode_graph_transaction(&GraphTransaction::new(
            scope(),
            vec![Operation::DeleteEntity {
                target: record(1),
                expected: Expected::Version(RecordVersion::FIRST),
                policy: DeletePolicy::Reject,
                affected: Vec::new(),
            }],
        ))
        .unwrap(),
    );
    assert!(matches!(
        state.prepare(
            &encode_transaction(&delete_job).unwrap(),
            None,
            CommitRevision::new(2).unwrap()
        ),
        Err(ApplyError::InvalidRequest)
    ));
    assert_eq!(state.snapshot().revision(), Some(CommitRevision::FIRST));

    let dangling = ImportBatch::new(
        ImportBatchId::new(record(1), 1).unwrap(),
        checkpoint.clone(),
        ImportCursor::START,
        vec![MappedRowReceipt::new(event(1), [31; 32])],
        graph_create(&[12], &[]),
        Some(spatial_world(20, 21)),
        false,
    )
    .unwrap();
    let dangling = EngineTransaction::apply_import_batch(scope(), dangling);
    let dangling_bytes = encode_transaction(&dangling).unwrap();
    assert!(matches!(
        state.prepare(&dangling_bytes, None, CommitRevision::new(2).unwrap()),
        Err(ApplyError::InvalidRequest)
    ));
    assert_eq!(state.snapshot().revision(), Some(CommitRevision::FIRST));
    assert!(
        state
            .snapshot()
            .spatial()
            .catalog()
            .last_revision()
            .is_none()
    );

    let batch = ImportBatch::new(
        ImportBatchId::new(record(1), 1).unwrap(),
        checkpoint,
        ImportCursor::START,
        vec![MappedRowReceipt::new(event(1), [31; 32])],
        graph_create(&[12], &[]),
        Some(spatial_world(10, 11)),
        true,
    )
    .unwrap();
    let batch = EngineTransaction::apply_import_batch(scope(), batch);
    let mut changed_source = encode_transaction(&batch).unwrap();
    // Request header + batch identity + checkpoint job/version/source metadata.
    const EXPECTED_SOURCE_DIGEST_OFFSET: usize = 288;
    changed_source[EXPECTED_SOURCE_DIGEST_OFFSET] ^= 1;
    assert!(decode_transaction(&changed_source).is_ok());
    assert!(matches!(
        state.prepare(&changed_source, None, CommitRevision::new(2).unwrap()),
        Err(ApplyError::SourceChanged)
    ));
    let mut changed_mapping = encode_transaction(&batch).unwrap();
    const EXPECTED_MAPPING_DIGEST_OFFSET: usize = 440;
    changed_mapping[EXPECTED_MAPPING_DIGEST_OFFSET] ^= 1;
    assert!(decode_transaction(&changed_mapping).is_ok());
    assert!(matches!(
        state.prepare(&changed_mapping, None, CommitRevision::new(2).unwrap()),
        Err(ApplyError::SourceChanged)
    ));
    assert_eq!(state.snapshot().revision(), Some(CommitRevision::FIRST));
    let batch_preview = state.preview(&batch, None).unwrap();
    assert_eq!(batch_preview.proposed_revision().get(), 2);
    assert_eq!(state.snapshot().revision().unwrap().get(), 1);
    let final_checkpoint = match publish(&mut state, &batch, None, 2) {
        ImportBatchOutcome::BatchCommitted {
            accepted,
            checkpoint,
            ..
        } => {
            assert_eq!(accepted, 1);
            checkpoint
        }
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert_eq!(final_checkpoint.accepted_rows(), 1);
    assert_eq!(final_checkpoint.next_batch(), 2);
    assert_eq!(state.snapshot().batch_count(), 1);
    assert!(
        state
            .snapshot()
            .spatial()
            .catalog()
            .world_at(
                VersionedRecordRef::new(record(10), SpatialVersion::FIRST),
                CommitRevision::new(2).unwrap(),
            )
            .is_ok()
    );

    let durable = capture_reducer_checkpoint(&state).unwrap();
    let checkpoint_payload = IngestState::encode_checkpoint(&state.snapshot()).unwrap();
    for end in 0..checkpoint_payload.len() {
        assert!(
            IngestState::decode_checkpoint(
                scope(),
                CommitRevision::new(2).unwrap(),
                &checkpoint_payload[..end],
            )
            .is_err(),
            "checkpoint cut {end}"
        );
    }
    let restored = verify_reducer_checkpoint::<IngestState>(scope(), &durable).unwrap();
    assert_eq!(
        IngestState::encode_checkpoint(&restored.snapshot()).unwrap(),
        checkpoint_payload
    );
    assert_eq!(
        IngestState::logical_state_digest(&state.snapshot()).unwrap(),
        IngestState::logical_state_digest(&restored.snapshot()).unwrap()
    );
    assert_eq!(
        restored.snapshot().job(record(1)).unwrap().checkpoint(),
        &final_checkpoint
    );

    let batch_bytes = encode_transaction(&batch).unwrap();
    for end in 0..batch_bytes.len() {
        assert!(
            decode_transaction(&batch_bytes[..end]).is_err(),
            "batch cut {end}"
        );
    }
    assert!(matches!(
        restored.prepare(&batch_bytes, None, CommitRevision::new(3).unwrap()),
        Err(ApplyError::Conflict)
    ));
    let delete_world = EngineTransaction::graph(
        scope(),
        encode_graph_transaction(&GraphTransaction::new(
            scope(),
            vec![Operation::DeleteEntity {
                target: record(10),
                expected: Expected::Version(RecordVersion::FIRST),
                policy: DeletePolicy::Reject,
                affected: Vec::new(),
            }],
        ))
        .unwrap(),
    );
    let delete_world_bytes = encode_transaction(&delete_world).unwrap();
    for end in 0..delete_world_bytes.len() {
        assert!(
            decode_transaction(&delete_world_bytes[..end]).is_err(),
            "graph cut {end}"
        );
    }
    assert!(matches!(
        restored.prepare(&delete_world_bytes, None, CommitRevision::new(3).unwrap()),
        Err(ApplyError::InvalidRequest)
    ));
    assert_eq!(restored.snapshot().revision().unwrap().get(), 2);

    let second_start = EngineTransaction::start_import(
        scope(),
        ImportStart::new(
            record(4),
            final_checkpoint.source().clone(),
            final_checkpoint.mapping().clone(),
            graph_create(&[4], &[]),
        )
        .unwrap(),
    );
    let second_inventory = BlobInventory::new(
        scope(),
        [
            final_checkpoint.source().blob(),
            final_checkpoint.mapping().blob(),
        ],
    )
    .unwrap();
    let second_checkpoint = match publish(&mut state, &second_start, Some(&second_inventory), 3) {
        ImportBatchOutcome::JobStarted(value) => value,
        other => panic!("unexpected outcome: {other:?}"),
    };
    let repeated_event = EngineTransaction::apply_import_batch(
        scope(),
        ImportBatch::new(
            ImportBatchId::new(record(4), 1).unwrap(),
            second_checkpoint,
            ImportCursor::START,
            vec![MappedRowReceipt::new(event(1), [99; 32])],
            graph_create(&[13], &[]),
            None,
            false,
        )
        .unwrap(),
    );
    assert!(matches!(
        state.prepare(
            &encode_transaction(&repeated_event).unwrap(),
            None,
            CommitRevision::new(4).unwrap()
        ),
        Err(ApplyError::Conflict)
    ));
    assert_eq!(state.snapshot().revision().unwrap().get(), 3);
}

#[test]
fn inventory_and_authorization_requirements_fail_closed() {
    let (start, inventory) = start_request();
    let encoded = encode_transaction(&start).unwrap();
    assert!(matches!(
        IngestState::new(scope()).prepare(&encoded, None, CommitRevision::FIRST),
        Err(ApplyError::InvalidRequest)
    ));
    let wrong = BlobInventory::new(scope(), [blob(91)]).unwrap();
    assert!(matches!(
        IngestState::new(scope()).prepare(&encoded, Some(&wrong), CommitRevision::FIRST),
        Err(ApplyError::InvalidRequest)
    ));
    let shared_id = BlobId::from_bytes([99; 16]);
    let left = BlobReference::new(scope(), shared_id, 1, 1, [41; 32]).unwrap();
    let right = BlobReference::new(scope(), shared_id, 1, 1, [42; 32]).unwrap();
    let colliding = EngineTransaction::start_import(
        scope(),
        ImportStart::new(
            record(1),
            SourceBinding::new(record(2), 1, left).unwrap(),
            MappingBinding::new(record(3), 1, right, MappingProfile::TypedRecordsV1).unwrap(),
            graph_create(&[1], &[(2, [41; 32]), (3, [42; 32])]),
        )
        .unwrap(),
    );
    let colliding_inventory = BlobInventory::new(scope(), [left]).unwrap();
    assert!(matches!(
        IngestState::new(scope()).prepare(
            &encode_transaction(&colliding).unwrap(),
            Some(&colliding_inventory),
            CommitRevision::FIRST,
        ),
        Err(ApplyError::InvalidRequest)
    ));

    let requirements = IngestState::authorization_requirements(&encoded, Some(&inventory)).unwrap();
    let values: Vec<_> = requirements.iter().collect();
    assert!(values.iter().any(|value| {
        value.action == Action::Import && value.target == Target::Namespace(scope())
    }));
    for id in [1, 2, 3, 10, 11] {
        assert!(
            values
                .iter()
                .any(|value| value.target == Target::Record(record(id)))
        );
    }

    let mut noncanonical = encoded;
    noncanonical[40] = 2;
    assert!(matches!(
        decode_transaction(&noncanonical),
        Err(ImportError::UnsupportedProfile)
    ));
}

#[test]
fn coordinator_retry_and_restart_resume_use_the_durable_import_ledger() {
    let mut filesystem = MemoryFileSystem::default();
    let name = EntryName::new("ingest-restart").unwrap();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        vault(100),
        CounterEntropy(200),
        IngestState::new(scope()),
    )
    .unwrap();
    let mut source_upload = coordinator.start_blob_upload(scope()).unwrap();
    coordinator
        .write_blob_upload(&mut filesystem, &mut source_upload, b"source")
        .unwrap();
    let source = coordinator
        .finish_blob_upload(&mut filesystem, &mut source_upload)
        .unwrap();
    let mut mapping_upload = coordinator.start_blob_upload(scope()).unwrap();
    coordinator
        .write_blob_upload(&mut filesystem, &mut mapping_upload, b"mapping")
        .unwrap();
    let mapping = coordinator
        .finish_blob_upload(&mut filesystem, &mut mapping_upload)
        .unwrap();
    let (start, inventory) = start_request_for(source, mapping);
    let start_bytes = encode_transaction(&start).unwrap();
    let principal = PrincipalDigest::from_bytes([7; 32]);
    let start_request = TransactionRequest {
        principal,
        idempotency_key: IdempotencyKey::from_bytes([1; 16]),
        transaction_id: TransactionId::from_bytes([11; 16]),
        canonical_request: &start_bytes,
        blob_inventory: Some(&inventory),
    };
    let started = coordinator
        .commit(&mut filesystem, start_request, &mut clock(0), &NeverCancel)
        .unwrap();
    assert_eq!(
        coordinator
            .commit(&mut filesystem, start_request, &mut clock(1), &NeverCancel,)
            .unwrap(),
        started
    );
    assert_eq!(
        coordinator
            .commit(
                &mut filesystem,
                TransactionRequest {
                    transaction_id: TransactionId::from_bytes([12; 16]),
                    ..start_request
                },
                &mut clock(1),
                &NeverCancel,
            )
            .unwrap_err(),
        TransactionError::Conflict
    );

    drop(coordinator);
    filesystem.restart().unwrap();
    let (mut coordinator, report) = CommitCoordinator::open(
        &mut filesystem,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(300),
        CounterEntropy(400),
        &mut TestKeyAdapter,
        IngestState::new(scope()),
    )
    .unwrap();
    assert_eq!(report.frontier.unwrap().get(), 1);
    assert_eq!(
        coordinator
            .commit(&mut filesystem, start_request, &mut clock(2), &NeverCancel,)
            .unwrap(),
        started
    );
    let checkpoint = coordinator
        .read_view()
        .unwrap()
        .state()
        .job(record(1))
        .unwrap()
        .checkpoint()
        .clone();
    let batch = EngineTransaction::apply_import_batch(
        scope(),
        ImportBatch::new(
            ImportBatchId::new(record(1), 1).unwrap(),
            checkpoint,
            ImportCursor::START,
            vec![MappedRowReceipt::new(event(8), [88; 32])],
            graph_create(&[12], &[]),
            Some(spatial_world(10, 11)),
            true,
        )
        .unwrap(),
    );
    let batch_bytes = encode_transaction(&batch).unwrap();
    let batch_request = TransactionRequest {
        principal,
        idempotency_key: IdempotencyKey::from_bytes([2; 16]),
        transaction_id: TransactionId::from_bytes([22; 16]),
        canonical_request: &batch_bytes,
        blob_inventory: None,
    };
    let completed = coordinator
        .commit(&mut filesystem, batch_request, &mut clock(2), &NeverCancel)
        .unwrap();
    assert_eq!(completed.revision.get(), 2);
    assert_eq!(
        coordinator
            .commit(&mut filesystem, batch_request, &mut clock(3), &NeverCancel,)
            .unwrap(),
        completed
    );
    assert_eq!(
        coordinator
            .commit(
                &mut filesystem,
                TransactionRequest {
                    idempotency_key: IdempotencyKey::from_bytes([3; 16]),
                    transaction_id: TransactionId::from_bytes([23; 16]),
                    ..batch_request
                },
                &mut clock(3),
                &NeverCancel,
            )
            .unwrap_err(),
        TransactionError::Conflict
    );
    assert_eq!(
        coordinator
            .read_view()
            .unwrap()
            .state()
            .job(record(1))
            .unwrap()
            .checkpoint()
            .accepted_rows(),
        1
    );
}

#[test]
fn authorized_job_projection_conceals_hidden_embedded_bindings() {
    let policy = import_policy();
    let install = EngineTransaction::graph(
        scope(),
        encode_graph_transaction(&GraphTransaction::with_policy_mutation(
            scope(),
            Vec::new(),
            DurablePolicyMutation::Install {
                policy: policy.clone(),
            },
        ))
        .unwrap(),
    );
    let install_bytes = encode_transaction(&install).unwrap();
    let mut filesystem = MemoryFileSystem::default();
    let mut raw = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        EntryName::new("authorized-import-read").unwrap(),
        vault(500),
        CounterEntropy(600),
        IngestState::new(scope()),
    )
    .unwrap();
    raw.commit(
        &mut filesystem,
        TransactionRequest {
            principal: PrincipalDigest::from_bytes([1; 32]),
            idempotency_key: IdempotencyKey::from_bytes([41; 16]),
            transaction_id: TransactionId::from_bytes([42; 16]),
            canonical_request: &install_bytes,
            blob_inventory: None,
        },
        &mut clock(0),
        &NeverCancel,
    )
    .unwrap();
    let mut source_upload = raw.start_blob_upload(scope()).unwrap();
    raw.write_blob_upload(&mut filesystem, &mut source_upload, b"private source")
        .unwrap();
    let source = raw
        .finish_blob_upload(&mut filesystem, &mut source_upload)
        .unwrap();
    let mut mapping_upload = raw.start_blob_upload(scope()).unwrap();
    raw.write_blob_upload(&mut filesystem, &mut mapping_upload, b"mapping")
        .unwrap();
    let mapping = raw
        .finish_blob_upload(&mut filesystem, &mut mapping_upload)
        .unwrap();
    let (start, inventory) = start_request_for(source, mapping);
    let start_bytes = encode_transaction(&start).unwrap();
    raw.commit(
        &mut filesystem,
        TransactionRequest {
            principal: PrincipalDigest::from_bytes([1; 32]),
            idempotency_key: IdempotencyKey::from_bytes([43; 16]),
            transaction_id: TransactionId::from_bytes([44; 16]),
            canonical_request: &start_bytes,
            blob_inventory: Some(&inventory),
        },
        &mut clock(1),
        &NeverCancel,
    )
    .unwrap();

    let mut kernel = PolicyKernel::new();
    kernel.install_initial_policy(policy).unwrap();
    let restricted = kernel.authenticate(&mut AuthAdapter, &7).unwrap();
    let complete = kernel.authenticate(&mut AuthAdapter, &8).unwrap();
    let mut authorized = AuthorizedCoordinator::new(raw, kernel).unwrap();
    let request = ImportReadRequest::Job { id: record(1) };

    let denied_graph = EngineTransaction::graph(scope(), graph_create(&[99], &[]));
    let denied_bytes = encode_transaction(&denied_graph).unwrap();
    assert_eq!(
        authorized.commit(
            &mut filesystem,
            &restricted,
            AuthorizedTransactionRequest {
                idempotency_key: IdempotencyKey::from_bytes([50; 16]),
                transaction_id: TransactionId::from_bytes([51; 16]),
                canonical_request: &denied_bytes,
                blob_inventory: None,
            },
            &mut clock(2),
            &NeverCancel,
        ),
        Err(AuthorizedError::Unauthorized)
    );

    let restricted_view = authorized.read_view(&restricted).unwrap();
    assert_eq!(
        authorized
            .read(&restricted, &restricted_view, &request)
            .unwrap(),
        ImportReadOutput::Job(None)
    );
    let complete_view = authorized.read_view(&complete).unwrap();
    assert!(matches!(
        authorized
            .read(&complete, &complete_view, &request)
            .unwrap(),
        ImportReadOutput::Job(Some(_))
    ));
    assert_eq!(
        authorized
            .read_view_revision(&complete_view)
            .unwrap()
            .unwrap()
            .get(),
        2
    );
}
use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
