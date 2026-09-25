//! Production-engine equivalence check for bounded, explicitly nonqualifying development sizes.

pub(crate) mod disk;
pub use disk::{DiskDevelopmentVerification, verify_disk_development_profile};
pub mod packed;
pub mod recovery;

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
    ClockObservation, EntryName, OwnershipFileSystem, fault::ScriptedClock,
    journal::DurableKeyEnvelope, memory::MemoryFileSystem,
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
pub(crate) const PRINCIPAL: PrincipalDigest = PrincipalDigest::from_bytes([0xb1; 32]);

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
                digest: engine_mapping_digest(profile),
                locator: text("bm01-uste-graph-v1")?,
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
        )
        .map_err(|error| error.to_string())?;
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

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum EngineQueryError {
    VisitLimit,
    ResultLimit,
    Invalid(&'static str),
    Engine(String),
}

impl core::fmt::Display for EngineQueryError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::VisitLimit => formatter.write_str("engine visit limit exceeded"),
            Self::ResultLimit => formatter.write_str("engine result limit exceeded"),
            Self::Invalid(message) => formatter.write_str(message),
            Self::Engine(message) => formatter.write_str(message),
        }
    }
}

pub(crate) fn execute_query<F, W, E, I>(
    coordinator: &AuthorizedCoordinator<GraphState, F, W, E, I>,
    filesystem: &mut F,
    principal: &AuthenticatedPrincipal,
    view: &AuthorizedReadView<GraphSnapshot>,
    root: &AuthorizedIndexRoot<AuthorizedGraphIndex>,
    materializer: Materializer,
    query: QuerySpec,
) -> Result<OracleOutput, EngineQueryError>
where
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    execute_query_with(materializer, query, |request| {
        coordinator
            .read_indexed(filesystem, principal, view, root, request)
            .map_err(|error| EngineQueryError::Engine(debug(error)))
    })
}

pub(crate) fn execute_query_with(
    materializer: Materializer,
    query: QuerySpec,
    mut read: impl FnMut(&GraphReadRequest) -> Result<GraphReadOutput, EngineQueryError>,
) -> Result<OracleOutput, EngineQueryError> {
    let limits = OracleLimits::default();
    let mut visits = 0_usize;
    let mut relationships = OrdinalSet::new(materializer.profile().relationships());
    let mut reachable_entities = OrdinalSet::new(materializer.profile().entities());
    let mut seen_entities = OrdinalSet::new(materializer.profile().entities());
    seen_entities.insert(query.root);
    // The frontier is always ascending and duplicate-free, so reads are issued in exactly the
    // order an ordered set would have produced.
    let mut frontier = vec![query.root];
    for _ in 0..query.depth {
        let mut next = Vec::new();
        for entity in frontier.iter().copied() {
            let output = read(&GraphReadRequest::Adjacent {
                entity: entity_ref(scope(), materializer, entity),
                direction: direction(query.direction),
                maximum: limits.maximum_results,
            })?;
            let GraphReadOutput::Adjacent(candidates) = output else {
                return Err(EngineQueryError::Invalid(
                    "adjacency request returned another output kind",
                ));
            };
            for candidate in candidates {
                visits = visits
                    .checked_add(1)
                    .ok_or(EngineQueryError::Invalid("visit count overflow"))?;
                if visits > limits.maximum_visits {
                    return Err(EngineQueryError::VisitLimit);
                }
                if candidate.relationship.status != AssertionStatus::Accepted {
                    return Err(EngineQueryError::Invalid(
                        "non-accepted relationship entered adjacency",
                    ));
                }
                let relationship = ordinal(
                    candidate.relationship.id.record(),
                    materializer.profile().relationships(),
                    "relationship",
                )
                .map_err(EngineQueryError::Engine)?;
                let edge = materializer.edge(relationship);
                if candidate.relationship.id
                    != relationship_ref(scope(), materializer, relationship)
                    || candidate.relationship.from != entity_ref(scope(), materializer, edge.source)
                    || candidate.relationship.to
                        != entity_ref(scope(), materializer, edge.destination)
                {
                    return Err(EngineQueryError::Invalid(
                        "engine relationship does not match materializer",
                    ));
                }
                let neighbor = ordinal(
                    candidate.entity.id.record(),
                    materializer.profile().entities(),
                    "entity",
                )
                .map_err(EngineQueryError::Engine)?;
                if candidate.entity.id != entity_ref(scope(), materializer, neighbor) {
                    return Err(EngineQueryError::Invalid(
                        "engine entity does not match materializer",
                    ));
                }
                let expected_neighbor = if edge.source == entity {
                    edge.destination
                } else if edge.destination == entity {
                    edge.source
                } else {
                    return Err(EngineQueryError::Invalid(
                        "adjacency relationship does not touch frontier entity",
                    ));
                };
                if neighbor != expected_neighbor {
                    return Err(EngineQueryError::Invalid("adjacency neighbor mismatch"));
                }
                if relationships.insert(relationship)
                    && relationships.len() > limits.maximum_results
                {
                    return Err(EngineQueryError::ResultLimit);
                }
                reachable_entities.insert(neighbor);
                if seen_entities.insert(neighbor) {
                    next.push(neighbor);
                }
            }
        }
        next.sort_unstable();
        frontier = next;
        if frontier.is_empty() {
            break;
        }
    }
    reachable_entities.remove(query.root);
    Ok(OracleOutput {
        visits,
        relationships: relationships.into_sorted_vec(),
        reachable_entities: reachable_entities.into_sorted_vec(),
    })
}

/// Fixed-width membership set over fixture ordinals below a known bound. Members are reported
/// in ascending order, matching the ordered-set iteration the harness previously relied on.
struct OrdinalSet {
    words: Vec<u64>,
    len: usize,
}

impl OrdinalSet {
    fn new(bound: u64) -> Self {
        let words = usize::try_from(bound.div_ceil(64)).expect("fixture bound fits in memory");
        Self {
            words: vec![0; words],
            len: 0,
        }
    }

    /// Insert one ordinal below the bound; returns whether it was newly inserted.
    fn insert(&mut self, ordinal: u64) -> bool {
        let (word, bit) = Self::locate(ordinal);
        let mask = 1_u64 << bit;
        if self.words[word] & mask != 0 {
            return false;
        }
        self.words[word] |= mask;
        self.len += 1;
        true
    }

    fn remove(&mut self, ordinal: u64) {
        let (word, bit) = Self::locate(ordinal);
        let mask = 1_u64 << bit;
        if self.words[word] & mask != 0 {
            self.words[word] &= !mask;
            self.len -= 1;
        }
    }

    const fn len(&self) -> usize {
        self.len
    }

    fn locate(ordinal: u64) -> (usize, u32) {
        let word = usize::try_from(ordinal / 64).expect("ordinal below the fixture bound");
        (word, (ordinal % 64) as u32)
    }

    fn into_sorted_vec(self) -> Vec<u64> {
        let mut output = Vec::with_capacity(self.len);
        for (index, word) in self.words.iter().enumerate() {
            let mut bits = *word;
            while bits != 0 {
                let bit = bits.trailing_zeros();
                output.push(index as u64 * 64 + u64::from(bit));
                bits &= bits - 1;
            }
        }
        output
    }
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

/// Durable profile binding stored in the shared Evidence record for the production mapping.
#[must_use]
pub fn engine_mapping_digest(profile: Bm01Profile) -> [u8; 32] {
    let mut digest = blake3::Hasher::new_derive_key("USTE BM-01 engine-mapping-v1");
    digest.update(b"bm01-uste-graph-v1");
    digest.update(&crate::ACCEPTED_SEED);
    digest.update(&Materializer::new(profile).digests().materialization);
    *digest.finalize().as_bytes()
}

pub(crate) fn benchmark_policy(scope: NamespaceRef) -> Result<NamespacePolicy, String> {
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

pub(crate) fn kernel(policy: NamespacePolicy) -> Result<PolicyKernel, String> {
    let mut kernel = PolicyKernel::new();
    kernel.install_initial_policy(policy).map_err(debug)?;
    Ok(kernel)
}

pub(crate) fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([0xb0; 16]),
        NamespaceId::from_bytes([0xb1; 16]),
    )
}

pub(crate) fn entity_ref(
    scope: NamespaceRef,
    materializer: Materializer,
    ordinal: u64,
) -> RecordRef {
    RecordRef::new(
        scope.database(),
        scope.namespace(),
        RecordId::from_bytes(materializer.entity_id(ordinal).0),
    )
}

pub(crate) fn relationship_ref(
    scope: NamespaceRef,
    materializer: Materializer,
    ordinal: u64,
) -> RecordRef {
    RecordRef::new(
        scope.database(),
        scope.namespace(),
        RecordId::from_bytes(materializer.relationship_id(ordinal).0),
    )
}

pub(crate) fn evidence_ref(scope: NamespaceRef) -> RecordRef {
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

pub(crate) fn text(value: &str) -> Result<BoundedString, String> {
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

pub(crate) struct AuthAdapter;

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
    use super::{
        engine_mapping_digest, materialization_revision_count, verify_development_profile,
    };
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

    #[test]
    fn qualifying_engine_mapping_digest_is_pinned() {
        assert_eq!(
            engine_mapping_digest(Bm01Profile::qualifying()),
            *blake3::Hash::from_hex(
                "b23db073664e93178d6b1dd23acf4cd72e12de9cb1b0d15007d616ea08907fe8"
            )
            .unwrap()
            .as_bytes()
        );
    }
}
