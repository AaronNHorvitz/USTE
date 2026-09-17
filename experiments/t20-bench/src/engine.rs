//! Production-engine equivalence check for bounded, explicitly nonqualifying development sizes.

use std::collections::BTreeSet;

use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};
use uste_graph::{
    AdjacencyDirection, AssertionAction, AssertionStatus, AuthorizedGraphIndex, Expected,
    GraphIndexCacheReport, GraphReadOutput, GraphReadRequest, GraphSnapshot, GraphState,
    GraphTransaction, MAX_TRANSACTION_OPERATIONS, NewEntity, NewEvidence, NewRecord,
    NewRelationship, Operation, RecordVersion, ValidTime, encode_transaction,
};
use uste_policy::{
    Action, AuthenticatedPrincipal, AuthenticationError, NamespaceGrant, NamespacePolicy,
    PermissionSet, PolicyKernel, PolicyVersion, PrincipalDigest, QuotaLimits,
    TrustedPrincipalAdapter,
};
use uste_storage::{
    ClockObservation, EntryName, fault::ScriptedClock, journal::DurableKeyEnvelope,
    memory::MemoryFileSystem,
};
use uste_txn::{
    AuthorizedCoordinator, AuthorizedIndexRoot, AuthorizedReadView, AuthorizedTransactionRequest,
    CommitCoordinator, NeverCancel, RetentionDays, TransactionRequest, open_authorized,
};
use uste_types::{
    BoundedString, DatabaseId, IdempotencyKey, NamespaceId, NamespaceRef, RecordId, RecordRef,
    TransactionId, UtcInstant, Value,
};

use crate::{
    Bm01Profile, Direction, Materializer, Oracle, OracleLimits, OracleOutput, QuerySpec,
    measured_queries,
};

pub const MAX_DEVELOPMENT_ENTITIES: u64 = 1_000;
const PRINCIPAL: PrincipalDigest = PrincipalDigest::from_bytes([0xb1; 32]);

type EngineCoordinator = AuthorizedCoordinator<
    GraphState,
    MemoryFileSystem,
    TestEnvelope,
    CounterEntropy,
    CounterEntropy,
>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DevelopmentVerification {
    pub entities: u64,
    pub relationships: u64,
    pub recovered_revision: u64,
    pub queries: usize,
    pub output_digest: [u8; 32],
    pub cache_report: GraphIndexCacheReport,
}

/// Materialize a bounded fixture through production transactions and compare every measured query
/// with the independent oracle. This uses the durable memory fault-model adapter and a development
/// key wrapper, so even an exact result is never BM-01 performance or platform evidence.
pub fn verify_development_profile(profile: Bm01Profile) -> Result<DevelopmentVerification, String> {
    if profile.entities() > MAX_DEVELOPMENT_ENTITIES {
        return Err(format!(
            "development verifier accepts at most {MAX_DEVELOPMENT_ENTITIES} entities"
        ));
    }
    let scope = scope();
    let policy = benchmark_policy(scope)?;
    let install = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope,
        Vec::new(),
        uste_graph::DurablePolicyMutation::Install {
            policy: policy.clone(),
        },
    ))
    .map_err(debug)?;

    let mut filesystem = MemoryFileSystem::default();
    let vault = KeyVault::create(scope.database(), &mut TestKeyAdapter, CounterEntropy(1))
        .map_err(debug)?;
    let database_name = EntryName::new("bm01-development-engine").map_err(debug)?;
    let mut raw = CommitCoordinator::create(
        &mut filesystem,
        scope,
        RetentionDays::new(30).map_err(debug)?,
        database_name.clone(),
        vault,
        CounterEntropy(100),
        GraphState::new(scope),
    )
    .map_err(debug)?;
    raw.commit(
        &mut filesystem,
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

    let reopen_policy = policy.clone();
    let policy_kernel = kernel(policy)?;
    let principal = policy_kernel
        .authenticate(&mut AuthAdapter, &())
        .map_err(debug)?;
    let mut coordinator = AuthorizedCoordinator::new(raw, policy_kernel).map_err(debug)?;
    let materializer = Materializer::new(profile);
    let mut sequence = 2_u64;

    commit_operation_batches(
        &mut coordinator,
        &mut filesystem,
        &principal,
        &mut sequence,
        core::iter::once(Ok(Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Evidence(NewEvidence {
                id: evidence_ref(scope),
                digest: crate::ACCEPTED_SEED,
                locator: text("bm01-materialization-v1")?,
            }),
        }))
        .chain((0..profile.entities()).map(|ordinal| {
            Ok(Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: entity_ref(scope, materializer, ordinal),
                    entity_type: text("bm01-entity-v1")?,
                    schema_version: 1,
                    properties: Value::Null,
                }),
            })
        })),
    )
    .map_err(|error| format!("entity materialization: {error}"))?;
    commit_operation_batches(
        &mut coordinator,
        &mut filesystem,
        &principal,
        &mut sequence,
        (0..profile.relationships()).map(|ordinal| {
            let edge = materializer.edge(ordinal);
            Ok(Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Relationship(NewRelationship {
                    id: relationship_ref(scope, materializer, ordinal),
                    from: entity_ref(scope, materializer, edge.source),
                    to: entity_ref(scope, materializer, edge.destination),
                    relationship_type: text(match edge.topology {
                        crate::Topology::Uniform => "bm01-uniform-v1",
                        crate::Topology::DistributedHub => "bm01-hub-v1",
                        crate::Topology::RingCycle => "bm01-ring-v1",
                    })?,
                    properties: Value::Null,
                    evidence: vec![evidence_ref(scope)],
                    valid_time: ValidTime::Unknown,
                }),
            })
        }),
    )
    .map_err(|error| format!("relationship materialization: {error}"))?;
    commit_operation_batches(
        &mut coordinator,
        &mut filesystem,
        &principal,
        &mut sequence,
        (0..profile.relationships()).map(|ordinal| {
            Ok(Operation::ActOnRelationship {
                target: relationship_ref(scope, materializer, ordinal),
                expected: Expected::Version(RecordVersion::FIRST),
                action: AssertionAction::Accept,
                correction: None,
                correction_expected: None,
            })
        }),
    )
    .map_err(|error| format!("relationship acceptance: {error}"))?;

    coordinator
        .publish_current_index(&mut filesystem, &principal)
        .map_err(debug)?;
    drop(coordinator);
    filesystem.restart().map_err(debug)?;

    let reopened_kernel = kernel(reopen_policy)?;
    let principal = reopened_kernel
        .authenticate(&mut AuthAdapter, &())
        .map_err(debug)?;
    let (coordinator, recovery) = open_authorized(
        &mut filesystem,
        &database_name,
        scope,
        RetentionDays::new(30).map_err(debug)?,
        CounterEntropy(200),
        CounterEntropy(300),
        &mut TestKeyAdapter,
        GraphState::new(scope),
        reopened_kernel,
    )
    .map_err(debug)?;
    let recovered_revision = recovery
        .frontier
        .ok_or("development recovery has no frontier")?
        .get();
    let mut roots = coordinator
        .load_current_index_roots(&mut filesystem, &principal)
        .map_err(debug)?;
    if roots.len() != 1 {
        return Err(format!(
            "expected one recovered index root, got {}",
            roots.len()
        ));
    }
    let root = roots.remove(0);
    let view = coordinator.read_view(&principal).map_err(debug)?;
    let oracle = Oracle::build(profile).map_err(debug)?;
    let queries = measured_queries(profile);
    let mut outputs = blake3::Hasher::new_derive_key("USTE BM-01 engine-equivalence-v1");
    for query in &queries {
        let expected = oracle
            .expand(*query, OracleLimits::default())
            .map_err(debug)?;
        let actual = execute_query(
            &coordinator,
            &mut filesystem,
            &principal,
            &view,
            &root,
            materializer,
            *query,
        )?;
        if actual != expected {
            return Err(format!("engine/oracle mismatch for query {query:?}"));
        }
        outputs.update(&[query.class.code(), query.direction.code(), query.depth]);
        outputs.update(&query.ordinal.to_be_bytes());
        outputs.update(&actual.digest());
    }
    let cache_report = coordinator.index_report(&principal, &root).map_err(debug)?;
    Ok(DevelopmentVerification {
        entities: profile.entities(),
        relationships: profile.relationships(),
        recovered_revision,
        queries: queries.len(),
        output_digest: *outputs.finalize().as_bytes(),
        cache_report,
    })
}

fn execute_query(
    coordinator: &EngineCoordinator,
    filesystem: &mut MemoryFileSystem,
    principal: &AuthenticatedPrincipal,
    view: &AuthorizedReadView<GraphSnapshot>,
    root: &AuthorizedIndexRoot<AuthorizedGraphIndex>,
    materializer: Materializer,
    query: QuerySpec,
) -> Result<OracleOutput, String> {
    let limits = OracleLimits::default();
    let mut visits = 0_usize;
    let mut relationships = BTreeSet::new();
    let mut reachable_entities = BTreeSet::new();
    let mut seen_entities = BTreeSet::from([query.root]);
    let mut frontier = BTreeSet::from([query.root]);
    for _ in 0..query.depth {
        let mut next = BTreeSet::new();
        for entity in frontier {
            let output = coordinator
                .read_indexed(
                    filesystem,
                    principal,
                    view,
                    root,
                    &GraphReadRequest::Adjacent {
                        entity: entity_ref(scope(), materializer, entity),
                        direction: direction(query.direction),
                        maximum: limits.maximum_results,
                    },
                )
                .map_err(debug)?;
            let GraphReadOutput::Adjacent(candidates) = output else {
                return Err("adjacency request returned another output kind".into());
            };
            for candidate in candidates {
                visits = visits.checked_add(1).ok_or("visit count overflow")?;
                if visits > limits.maximum_visits {
                    return Err("engine visit limit exceeded".into());
                }
                if candidate.relationship.status != AssertionStatus::Accepted {
                    return Err("non-accepted relationship entered adjacency".into());
                }
                let relationship = ordinal(
                    candidate.relationship.id.record(),
                    materializer.profile().relationships(),
                    "relationship",
                )?;
                let edge = materializer.edge(relationship);
                if candidate.relationship.id
                    != relationship_ref(scope(), materializer, relationship)
                    || candidate.relationship.from != entity_ref(scope(), materializer, edge.source)
                    || candidate.relationship.to
                        != entity_ref(scope(), materializer, edge.destination)
                {
                    return Err("engine relationship does not match materializer".into());
                }
                let neighbor = ordinal(
                    candidate.entity.id.record(),
                    materializer.profile().entities(),
                    "entity",
                )?;
                if candidate.entity.id != entity_ref(scope(), materializer, neighbor) {
                    return Err("engine entity does not match materializer".into());
                }
                let expected_neighbor = if edge.source == entity {
                    edge.destination
                } else if edge.destination == entity {
                    edge.source
                } else {
                    return Err("adjacency relationship does not touch frontier entity".into());
                };
                if neighbor != expected_neighbor {
                    return Err("adjacency neighbor mismatch".into());
                }
                if relationships.insert(relationship)
                    && relationships.len() > limits.maximum_results
                {
                    return Err("engine result limit exceeded".into());
                }
                reachable_entities.insert(neighbor);
                if seen_entities.insert(neighbor) {
                    next.insert(neighbor);
                }
            }
        }
        frontier = next;
        if frontier.is_empty() {
            break;
        }
    }
    reachable_entities.remove(&query.root);
    Ok(OracleOutput {
        visits,
        relationships: relationships.into_iter().collect(),
        reachable_entities: reachable_entities.into_iter().collect(),
    })
}

fn commit_operations(
    coordinator: &mut EngineCoordinator,
    filesystem: &mut MemoryFileSystem,
    principal: &AuthenticatedPrincipal,
    sequence: u64,
    operations: Vec<Operation>,
) -> Result<(), String> {
    let bytes = encode_transaction(&GraphTransaction::new(scope(), operations)).map_err(debug)?;
    coordinator
        .commit(
            filesystem,
            principal,
            AuthorizedTransactionRequest {
                idempotency_key: identity(sequence, IdempotencyKey::from_bytes),
                transaction_id: identity(sequence, TransactionId::from_bytes),
                canonical_request: &bytes,
                blob_inventory: None,
            },
            &mut clock(sequence),
            &NeverCancel,
        )
        .map_err(debug)?;
    Ok(())
}

fn commit_operation_batches<I>(
    coordinator: &mut EngineCoordinator,
    filesystem: &mut MemoryFileSystem,
    principal: &AuthenticatedPrincipal,
    sequence: &mut u64,
    operations: I,
) -> Result<(), String>
where
    I: IntoIterator<Item = Result<Operation, String>>,
{
    let mut batch = Vec::with_capacity(MAX_TRANSACTION_OPERATIONS);
    for operation in operations {
        batch.push(operation?);
        if batch.len() == MAX_TRANSACTION_OPERATIONS {
            let full =
                core::mem::replace(&mut batch, Vec::with_capacity(MAX_TRANSACTION_OPERATIONS));
            commit_operations(coordinator, filesystem, principal, *sequence, full)?;
            *sequence = sequence
                .checked_add(1)
                .ok_or("transaction sequence overflow")?;
        }
    }
    if !batch.is_empty() {
        commit_operations(coordinator, filesystem, principal, *sequence, batch)?;
        *sequence = sequence
            .checked_add(1)
            .ok_or("transaction sequence overflow")?;
    }
    Ok(())
}

/// Exact policy-plus-record revision count for the fixed maximum-size batching protocol.
#[must_use]
pub fn materialization_revision_count(profile: Bm01Profile) -> u64 {
    let batch = u64::try_from(MAX_TRANSACTION_OPERATIONS).expect("fixed transaction cap");
    1 + (profile.entities() + 1).div_ceil(batch)
        + profile.relationships().div_ceil(batch)
        + profile.relationships().div_ceil(batch)
}

fn benchmark_policy(scope: NamespaceRef) -> Result<NamespacePolicy, String> {
    let quotas = QuotaLimits::new(16 * 1024 * 1024, 0, 0, 0, 1024 * 1024).map_err(debug)?;
    let mut policy = NamespacePolicy::new(scope, PolicyVersion::new(1).map_err(debug)?, quotas);
    policy
        .grant(
            PRINCIPAL,
            NamespaceGrant::new(
                PermissionSet::from_actions([
                    Action::ReadRecord,
                    Action::ExpandGraph,
                    Action::Commit,
                    Action::ManageSchema,
                ]),
                quotas,
            ),
        )
        .map_err(debug)?;
    Ok(policy)
}

fn kernel(policy: NamespacePolicy) -> Result<PolicyKernel, String> {
    let mut kernel = PolicyKernel::new();
    kernel.install_initial_policy(policy).map_err(debug)?;
    Ok(kernel)
}

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([0xb0; 16]),
        NamespaceId::from_bytes([0xb1; 16]),
    )
}

fn entity_ref(scope: NamespaceRef, materializer: Materializer, ordinal: u64) -> RecordRef {
    RecordRef::new(
        scope.database(),
        scope.namespace(),
        RecordId::from_bytes(materializer.entity_id(ordinal).0),
    )
}

fn relationship_ref(scope: NamespaceRef, materializer: Materializer, ordinal: u64) -> RecordRef {
    RecordRef::new(
        scope.database(),
        scope.namespace(),
        RecordId::from_bytes(materializer.relationship_id(ordinal).0),
    )
}

fn evidence_ref(scope: NamespaceRef) -> RecordRef {
    RecordRef::new(
        scope.database(),
        scope.namespace(),
        RecordId::from_bytes([0xbe; 16]),
    )
}

fn ordinal(id: RecordId, upper_bound: u64, kind: &str) -> Result<u64, String> {
    let ordinal = u64::from_be_bytes(
        id.as_bytes()[8..]
            .try_into()
            .expect("fixed identity suffix"),
    );
    if ordinal >= upper_bound {
        return Err(format!("engine returned out-of-range {kind} ID"));
    }
    Ok(ordinal)
}

fn direction(value: Direction) -> AdjacencyDirection {
    match value {
        Direction::Outgoing => AdjacencyDirection::Outgoing,
        Direction::Incoming => AdjacencyDirection::Incoming,
        Direction::Either => AdjacencyDirection::Either,
    }
}

fn text(value: &str) -> Result<BoundedString, String> {
    BoundedString::new(value.to_owned()).map_err(debug)
}

fn identity<T>(sequence: u64, construct: impl FnOnce([u8; 16]) -> T) -> T {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(b"BM01DEV\0");
    bytes[8..].copy_from_slice(&sequence.to_be_bytes());
    construct(bytes)
}

fn clock(sequence: u64) -> ScriptedClock {
    ScriptedClock::new([Ok(ClockObservation {
        wall_utc: UtcInstant::new(i64::try_from(sequence).expect("small sequence"), 0)
            .expect("valid instant"),
        monotonic_ticks: sequence,
    })])
}

fn debug(error: impl core::fmt::Debug) -> String {
    format!("{error:?}")
}

struct AuthAdapter;

impl TrustedPrincipalAdapter for AuthAdapter {
    type Credential = ();

    fn authenticate(
        &mut self,
        _credential: &Self::Credential,
    ) -> Result<PrincipalDigest, AuthenticationError> {
        Ok(PRINCIPAL)
    }
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

#[cfg(test)]
mod tests {
    use super::{materialization_revision_count, verify_development_profile};
    use crate::Bm01Profile;

    #[test]
    fn production_engine_matches_oracle_for_every_scaled_query() {
        let report = verify_development_profile(Bm01Profile::new(20).unwrap()).unwrap();
        assert_eq!(report.entities, 20);
        assert_eq!(report.relationships, 200);
        assert_eq!(report.recovered_revision, 4);
        assert_eq!(report.queries, 384);
        assert_eq!(
            report.output_digest,
            *blake3::Hash::from_hex(
                "46f1bdb3138d6325e4c0f56b5fd3bbf5ff092d816e8a0f6c23acd15687b910b5"
            )
            .unwrap()
            .as_bytes()
        );
        assert!(report.cache_report.completed_authorized_reads > 0);
        assert!(report.cache_report.pages_read > 0);
    }

    #[test]
    fn qualifying_materialization_has_a_pinned_bounded_batch_plan() {
        assert_eq!(
            materialization_revision_count(Bm01Profile::qualifying()),
            212
        );
    }
}
