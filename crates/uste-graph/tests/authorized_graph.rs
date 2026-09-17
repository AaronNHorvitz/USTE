use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
use uste_graph::{
    AdjacencyDirection, AssertionAction, DurablePolicyMutation, Expected, GraphError,
    GraphReadOutput, GraphReadRequest, GraphState, GraphTransaction, NewAssertion, NewEntity,
    NewEvidence, NewRecord, NewRelationship, Operation, RecordVersion, ValidTime,
    encode_transaction,
};
use uste_policy::{
    Action, AuthenticationError, NamespaceGrant, NamespacePolicy, PermissionSet, PolicyKernel,
    PolicyVersion, PrincipalDigest, QuotaLimits, TrustedPrincipalAdapter,
};
use uste_replay::{capture_coordinator_checkpoint, decode_coordinator_checkpoint};
use uste_storage::{
    ClockObservation, EntryName, fault::ScriptedClock, journal::DurableKeyEnvelope,
    memory::MemoryFileSystem,
};
use uste_txn::{
    AuthorizedCoordinator, AuthorizedError, AuthorizedReadError, AuthorizedTransactionRequest,
    CommitCoordinator, NeverCancel, RetentionDays, TransactionRequest,
    load_verified_checkpoint_candidates, open_authorized,
};
use uste_types::{
    BoundedString, DatabaseId, IdempotencyKey, NamespaceId, NamespaceRef, RecordId, RecordRef,
    TransactionId, UtcInstant, Value,
};

const ADMIN_ACTIONS: &[Action] = &[
    Action::ReadRecord,
    Action::ReadHistory,
    Action::ExpandGraph,
    Action::Commit,
    Action::ManagePolicy,
];

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([21; 16]),
        NamespaceId::from_bytes([22; 16]),
    )
}

fn record(value: u8) -> RecordRef {
    let scope = scope();
    RecordRef::new(
        scope.database(),
        scope.namespace(),
        RecordId::from_bytes([value; 16]),
    )
}

fn text(value: &str) -> BoundedString {
    BoundedString::new(value.to_owned()).unwrap()
}

fn limits() -> QuotaLimits {
    QuotaLimits::new(1024 * 1024, 1024 * 1024, 1024 * 1024, 2, 1024).unwrap()
}

fn policy(
    version: u64,
    hide_from_bob: &[RecordRef],
    admin_actions: &[Action],
    bob_actions: &[Action],
) -> NamespacePolicy {
    let mut policy = NamespacePolicy::new(scope(), PolicyVersion::new(version).unwrap(), limits());
    policy
        .grant(
            PrincipalDigest::from_bytes([3; 32]),
            NamespaceGrant::new(
                PermissionSet::from_actions(admin_actions.iter().copied()),
                limits(),
            ),
        )
        .unwrap();
    let mut bob = NamespaceGrant::new(
        PermissionSet::from_actions(bob_actions.iter().copied()),
        limits(),
    );
    for hidden in hide_from_bob {
        bob.deny_record(
            hidden.record(),
            PermissionSet::from_actions([
                Action::ReadRecord,
                Action::ReadHistory,
                Action::ExpandGraph,
            ]),
        )
        .unwrap();
    }
    policy
        .grant(PrincipalDigest::from_bytes([4; 32]), bob)
        .unwrap();
    policy
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

fn kernel(policy: NamespacePolicy) -> PolicyKernel {
    let mut kernel = PolicyKernel::new();
    kernel.install_initial_policy(policy).unwrap();
    kernel
}

fn clock(day: i64) -> ScriptedClock {
    ScriptedClock::new([Ok(ClockObservation {
        wall_utc: UtcInstant::new(day * 86_400, 0).unwrap(),
        monotonic_ticks: u64::try_from(day).unwrap(),
    })])
}

fn authorized_request<'a>(value: u8, bytes: &'a [u8]) -> AuthorizedTransactionRequest<'a> {
    AuthorizedTransactionRequest {
        idempotency_key: IdempotencyKey::from_bytes([value; 16]),
        transaction_id: TransactionId::from_bytes([value.wrapping_add(64); 16]),
        canonical_request: bytes,
        blob_inventory: None,
    }
}

#[test]
fn authorized_graph_rejects_missing_durable_policy() {
    let mut filesystem = MemoryFileSystem::default();
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        EntryName::new("missing-durable-policy").unwrap(),
        create_vault(scope().database(), 1),
        CounterEntropy(2),
        GraphState::new(scope()),
    )
    .unwrap();
    assert!(matches!(
        AuthorizedCoordinator::new(raw, kernel(policy(1, &[], ADMIN_ACTIONS, ADMIN_ACTIONS))),
        Err(AuthorizedError::InvalidPolicy)
    ));
}

#[test]
fn durable_policy_graph_queries_revocation_and_restart_are_coherent() {
    let left = record(1);
    let right = record(2);
    let evidence = record(3);
    let relationship = record(4);
    let assertion = record(6);
    let correction = record(7);
    let hidden_missing = record(99);
    let bob_read_actions = [
        Action::ReadRecord,
        Action::ReadHistory,
        Action::ExpandGraph,
        Action::Commit,
    ];
    let initial_policy = policy(
        1,
        &[right, correction, hidden_missing],
        ADMIN_ACTIONS,
        &bob_read_actions,
    );
    let install = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        Vec::new(),
        DurablePolicyMutation::Install {
            policy: initial_policy.clone(),
        },
    ))
    .unwrap();
    let mut filesystem = MemoryFileSystem::default();
    let mut raw = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        RetentionDays::new(30).unwrap(),
        EntryName::new("authorized-graph").unwrap(),
        create_vault(scope().database(), 10),
        CounterEntropy(100),
        GraphState::new(scope()),
    )
    .unwrap();
    raw.commit(
        &mut filesystem,
        TransactionRequest {
            principal: PrincipalDigest::from_bytes([3; 32]),
            idempotency_key: IdempotencyKey::from_bytes([1; 16]),
            transaction_id: TransactionId::from_bytes([65; 16]),
            canonical_request: &install,
            blob_inventory: None,
        },
        &mut clock(0),
        &NeverCancel,
    )
    .unwrap();

    let policy_kernel = kernel(initial_policy);
    let admin = policy_kernel.authenticate(&mut AuthAdapter, &3).unwrap();
    let bob = policy_kernel.authenticate(&mut AuthAdapter, &4).unwrap();
    let mut coordinator = AuthorizedCoordinator::new(raw, policy_kernel).unwrap();
    assert_eq!(
        coordinator.replace_namespace_policy(
            &admin,
            PolicyVersion::new(1).unwrap(),
            policy(2, &[right], ADMIN_ACTIONS, &bob_read_actions),
        ),
        Err(AuthorizedError::InvalidPolicy)
    );
    assert_eq!(
        coordinator.commit(
            &mut filesystem,
            &admin,
            authorized_request(8, &install),
            &mut clock(1),
            &NeverCancel,
        ),
        Err(AuthorizedError::Transaction(
            uste_txn::TransactionError::InvalidRequest
        ))
    );
    assert_eq!(
        coordinator
            .read_view_revision(&coordinator.read_view(&admin).unwrap())
            .unwrap()
            .unwrap()
            .get(),
        1
    );

    let create = encode_transaction(&GraphTransaction::new(
        scope(),
        vec![
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: left,
                    entity_type: text("node"),
                    schema_version: 1,
                    properties: Value::RecordRef(right),
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
                    digest: [8; 32],
                    locator: text("source:row:1"),
                }),
            },
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Relationship(NewRelationship {
                    id: relationship,
                    from: left,
                    to: right,
                    relationship_type: text("connects"),
                    properties: Value::Null,
                    evidence: vec![evidence],
                    valid_time: ValidTime::Unknown,
                }),
            },
            Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Assertion(NewAssertion {
                    id: assertion,
                    subject: left,
                    predicate: text("label"),
                    object: Value::String(text("original")),
                    evidence: vec![evidence],
                    valid_time: ValidTime::Unknown,
                }),
            },
        ],
    ))
    .unwrap();
    coordinator
        .commit(
            &mut filesystem,
            &admin,
            authorized_request(2, &create),
            &mut clock(1),
            &NeverCancel,
        )
        .unwrap();
    let accept = encode_transaction(&GraphTransaction::new(
        scope(),
        vec![
            Operation::ActOnRelationship {
                target: relationship,
                expected: Expected::Version(RecordVersion::FIRST),
                action: AssertionAction::Accept,
                correction: None,
                correction_expected: None,
            },
            Operation::ActOnAssertion {
                target: assertion,
                expected: Expected::Version(RecordVersion::FIRST),
                action: AssertionAction::Accept,
                correction: None,
                correction_expected: None,
            },
        ],
    ))
    .unwrap();
    coordinator
        .commit(
            &mut filesystem,
            &admin,
            authorized_request(3, &accept),
            &mut clock(2),
            &NeverCancel,
        )
        .unwrap();

    let admin_view = coordinator.read_view(&admin).unwrap();
    let bob_view = coordinator.read_view(&bob).unwrap();
    let request = GraphReadRequest::Adjacent {
        entity: left,
        direction: AdjacencyDirection::Outgoing,
        maximum: 10,
    };
    let GraphReadOutput::Adjacent(admin_neighbors) =
        coordinator.read(&admin, &admin_view, &request).unwrap()
    else {
        panic!("adjacent")
    };
    assert_eq!(admin_neighbors.len(), 1);
    let GraphReadOutput::Adjacent(bob_neighbors) =
        coordinator.read(&bob, &bob_view, &request).unwrap()
    else {
        panic!("adjacent")
    };
    assert!(bob_neighbors.is_empty());
    for id in [left, relationship] {
        assert_eq!(
            coordinator
                .read(&bob, &bob_view, &GraphReadRequest::Record { id })
                .unwrap(),
            GraphReadOutput::Record(None)
        );
    }
    let GraphReadOutput::Supported(supported) = coordinator
        .read(
            &bob,
            &bob_view,
            &GraphReadRequest::SupportedBy {
                evidence,
                maximum: 10,
            },
        )
        .unwrap()
    else {
        panic!("supported")
    };
    assert_eq!(supported.len(), 1);
    assert_eq!(supported[0].id(), assertion);
    let zero_limit_request = GraphReadRequest::Adjacent {
        entity: left,
        direction: AdjacencyDirection::Outgoing,
        maximum: 0,
    };
    assert_eq!(
        coordinator.read(&bob, &bob_view, &zero_limit_request),
        Ok(GraphReadOutput::Adjacent(Vec::new()))
    );
    assert_eq!(
        coordinator.read(&admin, &admin_view, &zero_limit_request),
        Err(AuthorizedReadError::Domain(GraphError::ResultLimit {
            actual: 1,
            maximum: 0,
        }))
    );
    let denied_claim = encode_transaction(&GraphTransaction::new(
        scope(),
        vec![Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Assertion(NewAssertion {
                id: record(5),
                subject: right,
                predicate: text("hidden-subject"),
                object: Value::Null,
                evidence: vec![evidence],
                valid_time: ValidTime::Unknown,
            }),
        }],
    ))
    .unwrap();
    assert_eq!(
        coordinator.commit(
            &mut filesystem,
            &bob,
            authorized_request(9, &denied_claim),
            &mut clock(2),
            &NeverCancel,
        ),
        Err(AuthorizedError::Unauthorized)
    );
    assert_eq!(
        coordinator
            .read_view_revision(&bob_view)
            .unwrap()
            .unwrap()
            .get(),
        3
    );
    let denied_correction = encode_transaction(&GraphTransaction::new(
        scope(),
        vec![Operation::ActOnAssertion {
            target: assertion,
            expected: Expected::Version(RecordVersion::new(2).unwrap()),
            action: AssertionAction::Correct,
            correction: Some(NewAssertion {
                id: correction,
                subject: left,
                predicate: text("label"),
                object: Value::String(text("corrected")),
                evidence: vec![evidence],
                valid_time: ValidTime::Unknown,
            }),
            correction_expected: Some(Expected::Absent),
        }],
    ))
    .unwrap();
    assert_eq!(
        coordinator.commit(
            &mut filesystem,
            &bob,
            authorized_request(10, &denied_correction),
            &mut clock(2),
            &NeverCancel,
        ),
        Err(AuthorizedError::Unauthorized)
    );
    assert_eq!(
        coordinator
            .read_view_revision(&bob_view)
            .unwrap()
            .unwrap()
            .get(),
        3
    );
    for denied in [right, hidden_missing] {
        assert_eq!(
            coordinator.read(&bob, &bob_view, &GraphReadRequest::Record { id: denied },),
            Err(AuthorizedReadError::Authorization(
                AuthorizedError::Unauthorized
            ))
        );
    }

    let second_policy = policy(2, &[right, correction, hidden_missing], ADMIN_ACTIONS, &[]);
    let replace = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        Vec::new(),
        DurablePolicyMutation::Replace {
            expected: PolicyVersion::new(1).unwrap(),
            policy: second_policy.clone(),
        },
    ))
    .unwrap();
    let second_outcome = coordinator
        .commit(
            &mut filesystem,
            &admin,
            authorized_request(4, &replace),
            &mut clock(3),
            &NeverCancel,
        )
        .unwrap();
    assert_eq!(
        coordinator.read_view_revision(&admin_view),
        Err(AuthorizedError::StalePolicy)
    );
    assert_eq!(
        coordinator.read_view_revision(&bob_view),
        Err(AuthorizedError::StalePolicy)
    );

    let final_policy = policy(3, &[right, correction, hidden_missing], ADMIN_ACTIONS, &[]);
    let replace_again = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        Vec::new(),
        DurablePolicyMutation::Replace {
            expected: PolicyVersion::new(2).unwrap(),
            policy: final_policy.clone(),
        },
    ))
    .unwrap();
    coordinator
        .commit(
            &mut filesystem,
            &admin,
            authorized_request(5, &replace_again),
            &mut clock(4),
            &NeverCancel,
        )
        .unwrap();
    assert_eq!(
        coordinator
            .commit(
                &mut filesystem,
                &admin,
                authorized_request(4, &replace),
                &mut clock(5),
                &NeverCancel,
            )
            .unwrap(),
        second_outcome
    );
    drop(coordinator);
    filesystem.restart().unwrap();

    let wrong_kernel = kernel(policy(
        1,
        &[right, correction, hidden_missing],
        ADMIN_ACTIONS,
        &bob_read_actions,
    ));
    assert!(matches!(
        open_authorized(
            &mut filesystem,
            &EntryName::new("authorized-graph").unwrap(),
            scope(),
            RetentionDays::new(30).unwrap(),
            CounterEntropy(150),
            CounterEntropy(160),
            &mut TestKeyAdapter,
            GraphState::new(scope()),
            wrong_kernel,
        ),
        Err(AuthorizedError::InvalidPolicy)
    ));

    let reopened_kernel = kernel(final_policy);
    let admin = reopened_kernel.authenticate(&mut AuthAdapter, &3).unwrap();
    let bob = reopened_kernel.authenticate(&mut AuthAdapter, &4).unwrap();
    let (reopened, report) = open_authorized(
        &mut filesystem,
        &EntryName::new("authorized-graph").unwrap(),
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(200),
        CounterEntropy(300),
        &mut TestKeyAdapter,
        GraphState::new(scope()),
        reopened_kernel,
    )
    .unwrap();
    assert_eq!(report.frontier.unwrap().get(), 5);
    let view = reopened.read_view(&admin).unwrap();
    let GraphReadOutput::Adjacent(neighbors) = reopened
        .read(
            &admin,
            &view,
            &GraphReadRequest::Adjacent {
                entity: left,
                direction: AdjacencyDirection::Outgoing,
                maximum: 10,
            },
        )
        .unwrap()
    else {
        panic!("adjacent")
    };
    assert_eq!(neighbors.len(), 1);
    assert!(matches!(
        reopened.read_view(&bob),
        Err(AuthorizedError::Unauthorized)
    ));
}

#[test]
fn encrypted_graph_checkpoint_matches_cold_state_and_replays_only_the_suffix() {
    let scope = scope();
    let name = EntryName::new("graph-checkpoint-recovery").unwrap();
    let entity = record(81);
    let mut filesystem = MemoryFileSystem::default();
    let mut coordinator = CommitCoordinator::create(
        &mut filesystem,
        scope,
        RetentionDays::new(30).unwrap(),
        name.clone(),
        create_vault(scope.database(), 700),
        CounterEntropy(800),
        GraphState::new(scope),
    )
    .unwrap();
    let create = encode_transaction(&GraphTransaction::new(
        scope,
        vec![Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Entity(NewEntity {
                id: entity,
                entity_type: text("checkpointed-item"),
                schema_version: 1,
                properties: Value::Unsigned(1),
            }),
        }],
    ))
    .unwrap();
    coordinator
        .commit(
            &mut filesystem,
            TransactionRequest {
                principal: PrincipalDigest::from_bytes([3; 32]),
                idempotency_key: IdempotencyKey::from_bytes([81; 16]),
                transaction_id: TransactionId::from_bytes([181; 16]),
                canonical_request: &create,
                blob_inventory: None,
            },
            &mut clock(10),
            &NeverCancel,
        )
        .unwrap();
    let checkpoint = capture_coordinator_checkpoint(
        coordinator.reducer_state_for_checkpoint().unwrap(),
        coordinator.checkpoint_anchor().unwrap().unwrap(),
        coordinator.checkpoint_outcomes(),
        coordinator.committed_blob_owners(),
    )
    .unwrap();
    coordinator
        .publish_checkpoint(&mut filesystem, checkpoint.storage_input())
        .unwrap();

    let replace = encode_transaction(&GraphTransaction::new(
        scope,
        vec![Operation::ReplaceEntity {
            target: entity,
            expected: Expected::Version(RecordVersion::FIRST),
            properties: Value::Unsigned(2),
        }],
    ))
    .unwrap();
    coordinator
        .commit(
            &mut filesystem,
            TransactionRequest {
                principal: PrincipalDigest::from_bytes([3; 32]),
                idempotency_key: IdempotencyKey::from_bytes([82; 16]),
                transaction_id: TransactionId::from_bytes([182; 16]),
                canonical_request: &replace,
                blob_inventory: None,
            },
            &mut clock(11),
            &NeverCancel,
        )
        .unwrap();
    let expected = coordinator.read_view().unwrap().state().clone();
    drop(coordinator);
    filesystem.restart().unwrap();

    let (candidates, verified) = load_verified_checkpoint_candidates::<_, TestEnvelope, _, _, _>(
        &mut filesystem,
        &name,
        scope,
        CounterEntropy(900),
        CounterEntropy(1_000),
        &mut TestKeyAdapter,
    )
    .unwrap();
    assert_eq!(verified.frontier.unwrap().get(), 2);
    let seed = decode_coordinator_checkpoint::<GraphState>(&candidates[0]).unwrap();
    let (recovered, report) = CommitCoordinator::open_seeded(
        &mut filesystem,
        &name,
        scope,
        RetentionDays::new(30).unwrap(),
        CounterEntropy(1_100),
        CounterEntropy(1_200),
        &mut TestKeyAdapter,
        seed,
    )
    .unwrap();
    assert_eq!(report.frontier.unwrap().get(), 2);
    let actual = recovered.read_view().unwrap().state().clone();
    assert_eq!(actual, expected);
    actual.validate_derived_indexes().unwrap();
    assert_eq!(actual.record(entity).unwrap().version().get(), 2);
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
