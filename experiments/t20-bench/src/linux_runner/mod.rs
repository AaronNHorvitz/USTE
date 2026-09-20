//! Linux/Btrfs materialization and recovery runner for the exact BM-01 graph mapping.
//!
//! This is qualification-candidate machinery, not a self-certifying benchmark. It intentionally
//! emits content-free reports and keeps filesystem paths and recovery metadata out of errors.

mod credential;
pub mod disk;
pub mod packed;
mod sampling;
mod supervision;

pub use sampling::{LinuxSamplingReport, sample, sample_worker, start_parent_watchdog};
pub use supervision::{supervise_disk_sample, supervise_sample};

use std::{
    fmt,
    fs::File,
    io::Read,
    path::Path,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use rustix::fs::{Mode, OFlags, open};
use uste_crypto::{KeyVault, OsEntropy, PortableRecoveryAdapter, RecoveryEnvelope};
use uste_graph::{
    AssertionAction, AuthorizedGraphIndex, Expected, GraphIndexCacheReport, GraphReadOutput,
    GraphReadRequest, GraphSnapshot, GraphState, GraphTransaction, MAX_TRANSACTION_OPERATIONS,
    NewEntity, NewEvidence, NewRecord, NewRelationship, Operation, Record, RecordVersion,
    ValidTime, encode_transaction,
};
use uste_policy::{
    AuthenticatedPrincipal, AuthenticationError, PolicyKernel, TrustedPrincipalAdapter,
};
use uste_storage::{
    AdapterError, AdapterErrorKind, Clock, ClockObservation, EntryName, journal::RecoveryReport,
    linux::LinuxFileSystem,
};
use uste_txn::{
    AuthorizedCoordinator, AuthorizedIndexRoot, AuthorizedReadView, AuthorizedTransactionRequest,
    CommitCoordinator, MAX_REQUEST_BYTES, NeverCancel, RetentionDays, TransactionRequest,
};
use uste_types::{IdempotencyKey, TransactionId, UtcInstant, Value};

use crate::{
    Bm01Profile, Materializer, OracleExpectedOutcome, OracleSummary,
    QUALIFYING_ORACLE_SUMMARY_DIGEST, QuerySet,
    engine::{
        EngineQueryError, PRINCIPAL, benchmark_policy, entity_ref, evidence_ref, execute_query,
        kernel, relationship_ref, scope, text,
    },
    engine_mapping_digest, materialization_revision_count,
    oracle_summary::logical_result_bytes,
};

const DATABASE_NAME: &str = "bm01-linux-engine";
const RETENTION_DAYS: u16 = 30;

type LinuxCoordinator =
    AuthorizedCoordinator<GraphState, LinuxFileSystem, RecoveryEnvelope, OsEntropy, OsEntropy>;
type LinuxRaw =
    CommitCoordinator<GraphState, LinuxFileSystem, RecoveryEnvelope, OsEntropy, OsEntropy>;
type LinuxRoot = AuthorizedIndexRoot<AuthorizedGraphIndex>;
type LinuxView = AuthorizedReadView<GraphSnapshot>;

struct Opened {
    filesystem: LinuxFileSystem,
    coordinator: LinuxCoordinator,
    principal: AuthenticatedPrincipal,
    root: LinuxRoot,
    view: LinuxView,
    recovery: RecoveryReport,
    setup_elapsed: std::time::Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinuxRunReport {
    pub phase: &'static str,
    pub entities: u64,
    pub relationships: u64,
    pub frontier: u64,
    pub durable_revisions: u64,
    pub current_roots: usize,
    pub repaired_certificate_tail_bytes: u64,
    pub ignored_uncommitted_journal_bytes: u64,
    pub elapsed_milliseconds: u128,
}

impl LinuxRunReport {
    #[must_use]
    pub fn to_json(self) -> String {
        let qualification = if self.entities == Bm01Profile::qualifying().entities() {
            "qualification-candidate-environment-unverified"
        } else {
            "nonqualifying-development-profile"
        };
        format!(
            concat!(
                "{{\"schema\":\"bm01-linux-run-v1\",",
                "\"phase\":\"{}\",\"engine_benchmark\":false,",
                "\"qualification\":\"{}\",",
                "\"filesystem_profile\":\"linux-x86_64-btrfs\",",
                "\"recovery_profile\":\"portable-argon2id-v1\",",
                "\"engine_mapping_profile\":\"bm01-uste-graph-v1\",",
                "\"entities\":{},\"relationships\":{},",
                "\"frontier\":{},\"durable_revisions\":{},",
                "\"current_roots\":{},",
                "\"maximum_transaction_operations\":{},",
                "\"repaired_certificate_tail_bytes\":{},",
                "\"ignored_uncommitted_journal_bytes\":{},",
                "\"elapsed_milliseconds\":{},",
                "\"uste_page_cache\":\"not-measured\",",
                "\"kernel_filesystem_device_cache\":\"uncontrolled\",",
                "\"full_memory_graph_state\":true}}"
            ),
            self.phase,
            qualification,
            self.entities,
            self.relationships,
            self.frontier,
            self.durable_revisions,
            self.current_roots,
            MAX_TRANSACTION_OPERATIONS,
            self.repaired_certificate_tail_bytes,
            self.ignored_uncommitted_journal_bytes,
            self.elapsed_milliseconds,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinuxQueryReport {
    pub entities: u64,
    pub relationships: u64,
    pub frontier: u64,
    pub queries: usize,
    pub successful_queries: usize,
    pub expected_visit_limits: usize,
    pub expected_result_limits: usize,
    pub setup_milliseconds: u128,
    pub query_milliseconds: u128,
    pub p50_nanoseconds: u128,
    pub p95_nanoseconds: u128,
    pub p99_nanoseconds: u128,
    pub visits: u64,
    pub logical_result_bytes: u64,
    pub current_rss_kib: u64,
    pub peak_rss_kib: u64,
    pub oracle_summary_digest: [u8; 32],
    pub output_digest: [u8; 32],
    pub cache_report: GraphIndexCacheReport,
}

impl LinuxQueryReport {
    #[must_use]
    pub fn to_json(self) -> String {
        let qualification = if self.entities == Bm01Profile::qualifying().entities() {
            "qualification-candidate-correctness-only"
        } else {
            "nonqualifying-development-correctness"
        };
        format!(
            concat!(
                "{{\"schema\":\"bm01-linux-query-v1\",",
                "\"engine_benchmark\":false,\"qualification\":\"{}\",",
                "\"filesystem_profile\":\"linux-x86_64-btrfs\",",
                "\"oracle_profile\":\"bm01-oracle-summary-v1\",",
                "\"result_size_profile\":\"bm01-result-v1\",",
                "\"uste_page_cache\":\"cleared-before-each-query\",",
                "\"kernel_filesystem_device_cache\":\"uncontrolled\",",
                "\"full_memory_graph_state\":true,",
                "\"entities\":{},\"relationships\":{},\"frontier\":{},",
                "\"queries\":{},\"successful_queries\":{},",
                "\"expected_visit_limits\":{},\"expected_result_limits\":{},",
                "\"setup_milliseconds\":{},\"query_milliseconds\":{},",
                "\"latency_nanoseconds\":{{\"p50\":{},\"p95\":{},\"p99\":{}}},",
                "\"visits\":{},\"logical_result_bytes\":{},",
                "\"current_rss_kib\":{},\"peak_rss_kib\":{},",
                "\"oracle_summary_digest\":\"{}\",\"output_digest\":\"{}\",",
                "\"index_cache_budget_bytes\":{},\"index_cache_accounted_bytes\":{},",
                "\"index_cache_hits\":{},\"index_cache_misses\":{},",
                "\"index_cache_evictions\":{},\"authorized_reads\":{},",
                "\"index_operations\":{},\"index_pages_read\":{},",
                "\"index_fragments_visited\":{},\"authenticated_index_result_bytes\":{}}}"
            ),
            qualification,
            self.entities,
            self.relationships,
            self.frontier,
            self.queries,
            self.successful_queries,
            self.expected_visit_limits,
            self.expected_result_limits,
            self.setup_milliseconds,
            self.query_milliseconds,
            self.p50_nanoseconds,
            self.p95_nanoseconds,
            self.p99_nanoseconds,
            self.visits,
            self.logical_result_bytes,
            self.current_rss_kib,
            self.peak_rss_kib,
            hex(&self.oracle_summary_digest),
            hex(&self.output_digest),
            self.cache_report.budget_bytes,
            self.cache_report.accounted_bytes,
            self.cache_report.hits,
            self.cache_report.misses,
            self.cache_report.evictions,
            self.cache_report.completed_authorized_reads,
            self.cache_report.completed_index_operations,
            self.cache_report.pages_read,
            self.cache_report.fragments_visited,
            self.cache_report.result_bytes,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinuxRunnerError {
    code: &'static str,
}

impl LinuxRunnerError {
    pub(super) const fn new(code: &'static str) -> Self {
        Self { code }
    }

    #[must_use]
    pub const fn code(self) -> &'static str {
        self.code
    }
}

impl fmt::Display for LinuxRunnerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for LinuxRunnerError {}

pub fn create(
    root: &Path,
    password_file: &Path,
    profile: Bm01Profile,
) -> Result<LinuxRunReport, LinuxRunnerError> {
    let started = Instant::now();
    let mut filesystem = open_filesystem(root)?;
    let password = credential::read_password(password_file)?;
    let mut adapter = PortableRecoveryAdapter::new(password);
    let vault = KeyVault::create(scope().database(), &mut adapter, OsEntropy)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_KEY_CREATE"))?;
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        retention()?,
        database_name()?,
        vault,
        OsEntropy,
        GraphState::new(scope()),
    )
    .map_err(|_| LinuxRunnerError::new("USTE_BM01_DATABASE_CREATE"))?;
    let (mut coordinator, principal, frontier) =
        materialize(raw, &mut filesystem, profile, &mut |_| Ok(()))?;
    let roots = ensure_current_root(&mut coordinator, &mut filesystem, &principal)?;
    Ok(report(
        "create",
        profile,
        frontier,
        roots,
        0,
        0,
        started.elapsed(),
    ))
}

/// Creates a durable prefix, announces its frontier, then waits to be killed by an external
/// process-loss harness. This command never returns after reaching a valid requested frontier.
pub fn create_crash_probe(
    root: &Path,
    password_file: &Path,
    profile: Bm01Profile,
    pause_after_revision: u64,
) -> Result<LinuxRunReport, LinuxRunnerError> {
    use std::io::Write as _;

    let final_revision = materialization_revision_count(profile);
    if pause_after_revision == 0 || pause_after_revision >= final_revision {
        return Err(LinuxRunnerError::new("USTE_BM01_CRASH_PROBE_REVISION"));
    }
    let mut filesystem = open_filesystem(root)?;
    let password = credential::read_password(password_file)?;
    let mut adapter = PortableRecoveryAdapter::new(password);
    let vault = KeyVault::create(scope().database(), &mut adapter, OsEntropy)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_KEY_CREATE"))?;
    let raw = CommitCoordinator::create(
        &mut filesystem,
        scope(),
        retention()?,
        database_name()?,
        vault,
        OsEntropy,
        GraphState::new(scope()),
    )
    .map_err(|_| LinuxRunnerError::new("USTE_BM01_DATABASE_CREATE"))?;
    let mut observer = |revision| {
        if revision == pause_after_revision {
            let marker = crash_probe_marker(revision, final_revision);
            let mut output = std::io::stdout().lock();
            writeln!(output, "{marker}")
                .and_then(|()| output.flush())
                .map_err(|_| LinuxRunnerError::new("USTE_BM01_CRASH_PROBE_SIGNAL"))?;
            loop {
                std::thread::park();
            }
        }
        Ok(())
    };
    let _ = materialize(raw, &mut filesystem, profile, &mut observer)?;
    Err(LinuxRunnerError::new("USTE_BM01_CRASH_PROBE_MISSED"))
}

fn crash_probe_marker(frontier: u64, planned_frontier: u64) -> String {
    format!(
        concat!(
            "{{\"schema\":\"bm01-linux-crash-probe-v1\",",
            "\"engine_benchmark\":false,\"phase\":\"durable-prefix-paused\",",
            "\"frontier\":{},\"planned_frontier\":{}}}"
        ),
        frontier, planned_frontier,
    )
}

pub fn resume(
    root: &Path,
    password_file: &Path,
    profile: Bm01Profile,
) -> Result<LinuxRunReport, LinuxRunnerError> {
    let started = Instant::now();
    let mut filesystem = open_filesystem(root)?;
    let password = credential::read_password(password_file)?;
    let mut adapter = PortableRecoveryAdapter::new(password);
    let (raw, recovery) = CommitCoordinator::open(
        &mut filesystem,
        &database_name()?,
        scope(),
        retention()?,
        OsEntropy,
        OsEntropy,
        &mut adapter,
        GraphState::new(scope()),
    )
    .map_err(|_| LinuxRunnerError::new("USTE_BM01_DATABASE_OPEN"))?;
    if recovery
        .frontier
        .map(|revision| revision.get())
        .is_some_and(|frontier| frontier > materialization_revision_count(profile))
    {
        return Err(LinuxRunnerError::new("USTE_BM01_FRONTIER_MISMATCH"));
    }
    let (mut coordinator, principal, frontier) =
        materialize(raw, &mut filesystem, profile, &mut |_| Ok(()))?;
    let roots = ensure_current_root(&mut coordinator, &mut filesystem, &principal)?;
    Ok(report(
        "resume",
        profile,
        frontier,
        roots,
        recovery.repaired_certificate_tail_bytes,
        recovery.ignored_uncommitted_journal_bytes,
        started.elapsed(),
    ))
}

pub fn validate_open(
    root: &Path,
    password_file: &Path,
    profile: Bm01Profile,
) -> Result<LinuxRunReport, LinuxRunnerError> {
    let opened = open_completed(root, password_file, profile)?;
    let frontier = opened
        .recovery
        .frontier
        .expect("validated open has a frontier")
        .get();
    Ok(report(
        "open",
        profile,
        frontier,
        1,
        opened.recovery.repaired_certificate_tail_bytes,
        opened.recovery.ignored_uncommitted_journal_bytes,
        opened.setup_elapsed,
    ))
}

pub fn query_correctness(
    root: &Path,
    password_file: &Path,
    oracle_file: &Path,
    profile: Bm01Profile,
) -> Result<LinuxQueryReport, LinuxRunnerError> {
    let summary = read_oracle_summary(oracle_file)?;
    validate_measured_summary(&summary, profile)?;
    let mut opened = open_completed(root, password_file, profile)?;
    let before = opened
        .coordinator
        .index_report(&opened.principal, &opened.root)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_INDEX_REPORT"))?;
    let materializer = Materializer::new(profile);
    let mut timings = Vec::with_capacity(summary.expectations().len());
    let mut successful_queries = 0_usize;
    let mut expected_visit_limits = 0_usize;
    let mut expected_result_limits = 0_usize;
    let mut visits = 0_u64;
    let mut result_bytes = 0_u64;
    let mut aggregate = blake3::Hasher::new_derive_key("USTE BM-01 linux-query-v1");
    let query_started = Instant::now();
    for expected in summary.expectations() {
        opened
            .coordinator
            .clear_index_cache(&opened.principal, &opened.root)
            .map_err(|_| LinuxRunnerError::new("USTE_BM01_INDEX_CLEAR"))?;
        let started = Instant::now();
        let actual = execute_query(
            &opened.coordinator,
            &mut opened.filesystem,
            &opened.principal,
            &opened.view,
            &opened.root,
            materializer,
            expected.query,
        );
        timings.push(started.elapsed().as_nanos());
        aggregate.update(&[
            expected.query.class.code(),
            expected.query.direction.code(),
            expected.query.depth,
        ]);
        aggregate.update(&expected.query.ordinal.to_be_bytes());
        match (expected.outcome, actual) {
            (
                OracleExpectedOutcome::Output {
                    visits: expected_visits,
                    relationships,
                    entities,
                    logical_result_bytes: expected_bytes,
                    output_digest,
                },
                Ok(actual),
            ) => {
                let actual_visits = u64::try_from(actual.visits)
                    .map_err(|_| LinuxRunnerError::new("USTE_BM01_QUERY_RESULT"))?;
                let actual_relationships = u64::try_from(actual.relationships.len())
                    .map_err(|_| LinuxRunnerError::new("USTE_BM01_QUERY_RESULT"))?;
                let actual_entities = u64::try_from(actual.reachable_entities.len())
                    .map_err(|_| LinuxRunnerError::new("USTE_BM01_QUERY_RESULT"))?;
                let actual_bytes = logical_result_bytes(&actual)
                    .map_err(|_| LinuxRunnerError::new("USTE_BM01_QUERY_RESULT"))?;
                let actual_digest = actual.digest();
                if actual_visits != expected_visits
                    || actual_relationships != relationships
                    || actual_entities != entities
                    || actual_bytes != expected_bytes
                    || actual_digest != output_digest
                {
                    return Err(LinuxRunnerError::new("USTE_BM01_QUERY_MISMATCH"));
                }
                successful_queries += 1;
                visits = visits
                    .checked_add(actual_visits)
                    .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_QUERY_RESULT"))?;
                result_bytes = result_bytes
                    .checked_add(actual_bytes)
                    .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_QUERY_RESULT"))?;
                aggregate.update(&[1]);
                aggregate.update(&actual_digest);
            }
            (OracleExpectedOutcome::VisitLimit, Err(EngineQueryError::VisitLimit)) => {
                expected_visit_limits += 1;
                aggregate.update(&[2]);
            }
            (OracleExpectedOutcome::ResultLimit, Err(EngineQueryError::ResultLimit)) => {
                expected_result_limits += 1;
                aggregate.update(&[3]);
            }
            _ => return Err(LinuxRunnerError::new("USTE_BM01_QUERY_MISMATCH")),
        }
    }
    let query_elapsed = query_started.elapsed();
    let after = opened
        .coordinator
        .index_report(&opened.principal, &opened.root)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_INDEX_REPORT"))?;
    let cache_report = cache_delta(before, after)?;
    let (current_rss_kib, peak_rss_kib) = process_rss()?;
    timings.sort_unstable();
    Ok(LinuxQueryReport {
        entities: profile.entities(),
        relationships: profile.relationships(),
        frontier: opened
            .recovery
            .frontier
            .expect("validated open has a frontier")
            .get(),
        queries: summary.expectations().len(),
        successful_queries,
        expected_visit_limits,
        expected_result_limits,
        setup_milliseconds: opened.setup_elapsed.as_millis(),
        query_milliseconds: query_elapsed.as_millis(),
        p50_nanoseconds: percentile(&timings, 50),
        p95_nanoseconds: percentile(&timings, 95),
        p99_nanoseconds: percentile(&timings, 99),
        visits,
        logical_result_bytes: result_bytes,
        current_rss_kib,
        peak_rss_kib,
        oracle_summary_digest: summary.digest(),
        output_digest: *aggregate.finalize().as_bytes(),
        cache_report,
    })
}

fn validate_measured_summary(
    summary: &OracleSummary,
    profile: Bm01Profile,
) -> Result<(), LinuxRunnerError> {
    if summary.profile() != profile {
        return Err(LinuxRunnerError::new("USTE_BM01_ORACLE_PROFILE"));
    }
    if summary.query_set() != QuerySet::Measured {
        return Err(LinuxRunnerError::new("USTE_BM01_ORACLE_QUERY_SET"));
    }
    if profile == Bm01Profile::qualifying() && summary.digest() != QUALIFYING_ORACLE_SUMMARY_DIGEST
    {
        return Err(LinuxRunnerError::new("USTE_BM01_ORACLE_ACCEPTANCE"));
    }
    Ok(())
}

fn open_completed(
    root: &Path,
    password_file: &Path,
    profile: Bm01Profile,
) -> Result<Opened, LinuxRunnerError> {
    let started = Instant::now();
    let mut filesystem = open_filesystem(root)?;
    let policy =
        benchmark_policy(scope()).map_err(|_| LinuxRunnerError::new("USTE_BM01_POLICY_PROFILE"))?;
    let policy_kernel =
        kernel(policy).map_err(|_| LinuxRunnerError::new("USTE_BM01_POLICY_PROFILE"))?;
    let principal = authenticate(&policy_kernel)?;
    let password = credential::read_password(password_file)?;
    let mut adapter = PortableRecoveryAdapter::new(password);
    let (coordinator, recovery) = uste_txn::open_authorized(
        &mut filesystem,
        &database_name()?,
        scope(),
        retention()?,
        OsEntropy,
        OsEntropy,
        &mut adapter,
        GraphState::new(scope()),
        policy_kernel,
    )
    .map_err(|_| LinuxRunnerError::new("USTE_BM01_DATABASE_OPEN"))?;
    let frontier = recovery
        .frontier
        .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_FRONTIER_MISSING"))?
        .get();
    require_expected_frontier(profile, frontier)?;
    let view = coordinator
        .read_view(&principal)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_READ_VIEW"))?;
    if coordinator
        .read_view_revision(&view)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_READ_VIEW"))?
        .map(|revision| revision.get())
        != Some(frontier)
    {
        return Err(LinuxRunnerError::new("USTE_BM01_REVISION_MISMATCH"));
    }
    let mut roots = coordinator
        .load_current_index_roots(&mut filesystem, &principal)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_INDEX_LOAD"))?;
    if roots.len() != 1 {
        return Err(LinuxRunnerError::new("USTE_BM01_INDEX_ROOT_COUNT"));
    }
    require_profile_binding(&coordinator, &principal, &view, profile)?;
    Ok(Opened {
        filesystem,
        coordinator,
        principal,
        root: roots.remove(0),
        view,
        recovery,
        setup_elapsed: started.elapsed(),
    })
}

fn materialize(
    mut raw: LinuxRaw,
    filesystem: &mut LinuxFileSystem,
    profile: Bm01Profile,
    observer: &mut impl FnMut(u64) -> Result<(), LinuxRunnerError>,
) -> Result<(LinuxCoordinator, AuthenticatedPrincipal, u64), LinuxRunnerError> {
    let policy =
        benchmark_policy(scope()).map_err(|_| LinuxRunnerError::new("USTE_BM01_POLICY_PROFILE"))?;
    let install = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        Vec::new(),
        uste_graph::DurablePolicyMutation::Install {
            policy: policy.clone(),
        },
    ))
    .map_err(|_| LinuxRunnerError::new("USTE_BM01_POLICY_ENCODE"))?;
    let mut clock = SystemClock::new();
    let install_outcome = raw
        .commit(
            filesystem,
            TransactionRequest {
                principal: PRINCIPAL,
                idempotency_key: identity(1, IdempotencyKey::from_bytes),
                transaction_id: identity(1, TransactionId::from_bytes),
                canonical_request: &install,
                blob_inventory: None,
            },
            &mut clock,
            &NeverCancel,
        )
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_POLICY_COMMIT"))?;
    if install_outcome.revision.get() != 1 {
        return Err(LinuxRunnerError::new("USTE_BM01_REVISION_MISMATCH"));
    }
    observer(1)?;

    let policy_kernel =
        kernel(policy).map_err(|_| LinuxRunnerError::new("USTE_BM01_POLICY_PROFILE"))?;
    let principal = authenticate(&policy_kernel)?;
    let mut coordinator = AuthorizedCoordinator::new(raw, policy_kernel)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_AUTHORIZED_OPEN"))?;
    let materializer = Materializer::new(profile);
    let mut sequence = 2_u64;

    commit_batches(
        &mut coordinator,
        filesystem,
        &principal,
        &mut clock,
        &mut sequence,
        observer,
        core::iter::once(Ok(Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Evidence(NewEvidence {
                id: evidence_ref(scope()),
                digest: engine_mapping_digest(profile),
                locator: text("bm01-uste-graph-v1")
                    .map_err(|_| LinuxRunnerError::new("USTE_BM01_MAPPING"))?,
            }),
        }))
        .chain((0..profile.entities()).map(|ordinal| {
            Ok(Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: entity_ref(scope(), materializer, ordinal),
                    entity_type: text("bm01-entity-v1")
                        .map_err(|_| LinuxRunnerError::new("USTE_BM01_MAPPING"))?,
                    schema_version: 1,
                    properties: Value::Null,
                }),
            })
        })),
    )?;
    commit_batches(
        &mut coordinator,
        filesystem,
        &principal,
        &mut clock,
        &mut sequence,
        observer,
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
                    })
                    .map_err(|_| LinuxRunnerError::new("USTE_BM01_MAPPING"))?,
                    properties: Value::Null,
                    evidence: vec![evidence_ref(scope())],
                    valid_time: ValidTime::Unknown,
                }),
            })
        }),
    )?;
    commit_batches(
        &mut coordinator,
        filesystem,
        &principal,
        &mut clock,
        &mut sequence,
        observer,
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
    let planned_frontier = sequence
        .checked_sub(1)
        .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SEQUENCE"))?;
    require_expected_frontier(profile, planned_frontier)?;
    let view = coordinator
        .read_view(&principal)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_READ_VIEW"))?;
    let actual_frontier = coordinator
        .read_view_revision(&view)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_READ_VIEW"))?
        .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_FRONTIER_MISSING"))?
        .get();
    require_expected_frontier(profile, actual_frontier)?;
    require_profile_binding(&coordinator, &principal, &view, profile)?;
    Ok((coordinator, principal, actual_frontier))
}

fn commit_batches<I>(
    coordinator: &mut LinuxCoordinator,
    filesystem: &mut LinuxFileSystem,
    principal: &AuthenticatedPrincipal,
    clock: &mut SystemClock,
    sequence: &mut u64,
    observer: &mut impl FnMut(u64) -> Result<(), LinuxRunnerError>,
    operations: I,
) -> Result<(), LinuxRunnerError>
where
    I: IntoIterator<Item = Result<Operation, LinuxRunnerError>>,
{
    let mut batch = Vec::with_capacity(MAX_TRANSACTION_OPERATIONS);
    for operation in operations {
        batch.push(operation?);
        if batch.len() == MAX_TRANSACTION_OPERATIONS {
            let full =
                core::mem::replace(&mut batch, Vec::with_capacity(MAX_TRANSACTION_OPERATIONS));
            commit_batch(coordinator, filesystem, principal, clock, *sequence, full)?;
            observer(*sequence)?;
            *sequence = sequence
                .checked_add(1)
                .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SEQUENCE"))?;
        }
    }
    if !batch.is_empty() {
        commit_batch(coordinator, filesystem, principal, clock, *sequence, batch)?;
        observer(*sequence)?;
        *sequence = sequence
            .checked_add(1)
            .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SEQUENCE"))?;
    }
    Ok(())
}

fn commit_batch(
    coordinator: &mut LinuxCoordinator,
    filesystem: &mut LinuxFileSystem,
    principal: &AuthenticatedPrincipal,
    clock: &mut SystemClock,
    sequence: u64,
    operations: Vec<Operation>,
) -> Result<(), LinuxRunnerError> {
    if operations.is_empty() || operations.len() > MAX_TRANSACTION_OPERATIONS {
        return Err(LinuxRunnerError::new("USTE_BM01_BATCH_BOUNDS"));
    }
    let bytes = encode_transaction(&GraphTransaction::new(scope(), operations))
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_TRANSACTION_ENCODE"))?;
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(LinuxRunnerError::new("USTE_BM01_REQUEST_BOUNDS"));
    }
    let outcome = coordinator
        .commit(
            filesystem,
            principal,
            AuthorizedTransactionRequest {
                idempotency_key: identity(sequence, IdempotencyKey::from_bytes),
                transaction_id: identity(sequence, TransactionId::from_bytes),
                canonical_request: &bytes,
                blob_inventory: None,
            },
            clock,
            &NeverCancel,
        )
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_TRANSACTION_COMMIT"))?;
    if outcome.revision.get() != sequence {
        return Err(LinuxRunnerError::new("USTE_BM01_REVISION_MISMATCH"));
    }
    Ok(())
}

fn require_profile_binding(
    coordinator: &LinuxCoordinator,
    principal: &AuthenticatedPrincipal,
    view: &uste_txn::AuthorizedReadView<uste_graph::GraphSnapshot>,
    profile: Bm01Profile,
) -> Result<(), LinuxRunnerError> {
    let output = coordinator
        .read(
            principal,
            view,
            &GraphReadRequest::Record {
                id: evidence_ref(scope()),
            },
        )
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_PROFILE_BINDING"))?;
    let GraphReadOutput::Record(Some(record)) = output else {
        return Err(LinuxRunnerError::new("USTE_BM01_PROFILE_BINDING"));
    };
    let Record::Evidence(evidence) = *record else {
        return Err(LinuxRunnerError::new("USTE_BM01_PROFILE_BINDING"));
    };
    if evidence.id != evidence_ref(scope())
        || evidence.version != RecordVersion::FIRST
        || evidence.created_revision.get() != 2
        || evidence.locator.as_str() != "bm01-uste-graph-v1"
        || evidence.digest != engine_mapping_digest(profile)
    {
        return Err(LinuxRunnerError::new("USTE_BM01_PROFILE_BINDING"));
    }
    Ok(())
}

fn ensure_current_root(
    coordinator: &mut LinuxCoordinator,
    filesystem: &mut LinuxFileSystem,
    principal: &AuthenticatedPrincipal,
) -> Result<usize, LinuxRunnerError> {
    let roots = coordinator
        .load_current_index_roots(filesystem, principal)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_INDEX_LOAD"))?;
    match roots.len() {
        0 => {
            coordinator
                .publish_current_index(filesystem, principal)
                .map_err(|_| LinuxRunnerError::new("USTE_BM01_INDEX_PUBLISH"))?;
            Ok(1)
        }
        1 => Ok(1),
        _ => Err(LinuxRunnerError::new("USTE_BM01_INDEX_ROOT_COUNT")),
    }
}

fn open_filesystem(root: &Path) -> Result<LinuxFileSystem, LinuxRunnerError> {
    let descriptor = open(
        root,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::DIRECTORY,
        Mode::empty(),
    )
    .map_err(|_| LinuxRunnerError::new("USTE_BM01_ROOT_OPEN"))?;
    LinuxFileSystem::from_directory(descriptor)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_ROOT_PROFILE"))
}

fn authenticate(kernel: &PolicyKernel) -> Result<AuthenticatedPrincipal, LinuxRunnerError> {
    kernel
        .authenticate(&mut AuthAdapter, &())
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_AUTHENTICATION"))
}

fn database_name() -> Result<EntryName, LinuxRunnerError> {
    EntryName::new(DATABASE_NAME).map_err(|_| LinuxRunnerError::new("USTE_BM01_PROFILE"))
}

fn retention() -> Result<RetentionDays, LinuxRunnerError> {
    RetentionDays::new(RETENTION_DAYS).map_err(|_| LinuxRunnerError::new("USTE_BM01_PROFILE"))
}

fn require_expected_frontier(profile: Bm01Profile, frontier: u64) -> Result<(), LinuxRunnerError> {
    if frontier != materialization_revision_count(profile) {
        return Err(LinuxRunnerError::new("USTE_BM01_FRONTIER_MISMATCH"));
    }
    Ok(())
}

fn report(
    phase: &'static str,
    profile: Bm01Profile,
    frontier: u64,
    current_roots: usize,
    repaired_certificate_tail_bytes: u64,
    ignored_uncommitted_journal_bytes: u64,
    elapsed: std::time::Duration,
) -> LinuxRunReport {
    LinuxRunReport {
        phase,
        entities: profile.entities(),
        relationships: profile.relationships(),
        frontier,
        durable_revisions: materialization_revision_count(profile),
        current_roots,
        repaired_certificate_tail_bytes,
        ignored_uncommitted_journal_bytes,
        elapsed_milliseconds: elapsed.as_millis(),
    }
}

fn read_oracle_summary(path: &Path) -> Result<OracleSummary, LinuxRunnerError> {
    let mut file = File::open(path).map_err(|_| LinuxRunnerError::new("USTE_BM01_ORACLE_OPEN"))?;
    let mut input = String::new();
    file.by_ref()
        .take(
            u64::try_from(crate::MAX_ORACLE_SUMMARY_BYTES + 1)
                .expect("oracle summary bound fits u64"),
        )
        .read_to_string(&mut input)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_ORACLE_READ"))?;
    if input.len() > crate::MAX_ORACLE_SUMMARY_BYTES {
        return Err(LinuxRunnerError::new("USTE_BM01_ORACLE_SIZE"));
    }
    OracleSummary::parse(&input).map_err(|_| LinuxRunnerError::new("USTE_BM01_ORACLE_INVALID"))
}

fn cache_delta(
    before: GraphIndexCacheReport,
    after: GraphIndexCacheReport,
) -> Result<GraphIndexCacheReport, LinuxRunnerError> {
    let subtract = |new: u64, old: u64| {
        new.checked_sub(old)
            .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_INDEX_COUNTER"))
    };
    Ok(GraphIndexCacheReport {
        budget_bytes: after.budget_bytes,
        accounted_bytes: after.accounted_bytes,
        hits: subtract(after.hits, before.hits)?,
        misses: subtract(after.misses, before.misses)?,
        evictions: subtract(after.evictions, before.evictions)?,
        completed_authorized_reads: subtract(
            after.completed_authorized_reads,
            before.completed_authorized_reads,
        )?,
        completed_index_operations: subtract(
            after.completed_index_operations,
            before.completed_index_operations,
        )?,
        pages_read: subtract(after.pages_read, before.pages_read)?,
        fragments_visited: subtract(after.fragments_visited, before.fragments_visited)?,
        result_bytes: subtract(after.result_bytes, before.result_bytes)?,
    })
}

fn process_rss() -> Result<(u64, u64), LinuxRunnerError> {
    let mut file =
        File::open("/proc/self/status").map_err(|_| LinuxRunnerError::new("USTE_BM01_RSS_OPEN"))?;
    let mut status = String::new();
    file.by_ref()
        .take(128 * 1024)
        .read_to_string(&mut status)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_RSS_READ"))?;
    let current = status_value_kib(&status, "VmRSS:")?;
    let peak = status_value_kib(&status, "VmHWM:")?;
    Ok((current, peak))
}

fn status_value_kib(status: &str, key: &str) -> Result<u64, LinuxRunnerError> {
    let line = status
        .lines()
        .find(|line| line.starts_with(key))
        .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_RSS_FORMAT"))?;
    let mut fields = line.split_ascii_whitespace();
    if fields.next() != Some(key) {
        return Err(LinuxRunnerError::new("USTE_BM01_RSS_FORMAT"));
    }
    let value = fields
        .next()
        .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_RSS_FORMAT"))?
        .parse()
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_RSS_FORMAT"))?;
    if fields.next() != Some("kB") || fields.next().is_some() {
        return Err(LinuxRunnerError::new("USTE_BM01_RSS_FORMAT"));
    }
    Ok(value)
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = sorted
        .len()
        .checked_mul(percentile)
        .and_then(|value| value.checked_add(99))
        .map(|value| value / 100)
        .unwrap_or(sorted.len());
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn hex(bytes: &[u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(64);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn identity<T>(sequence: u64, construct: impl FnOnce([u8; 16]) -> T) -> T {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(b"BM01LIN\0");
    bytes[8..].copy_from_slice(&sequence.to_be_bytes());
    construct(bytes)
}

struct AuthAdapter;

impl TrustedPrincipalAdapter for AuthAdapter {
    type Credential = ();

    fn authenticate(
        &mut self,
        _credential: &Self::Credential,
    ) -> Result<uste_policy::PrincipalDigest, AuthenticationError> {
        Ok(PRINCIPAL)
    }
}

struct SystemClock {
    monotonic_origin: Instant,
}

impl SystemClock {
    fn new() -> Self {
        Self {
            monotonic_origin: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn observe(&mut self) -> Result<ClockObservation, AdapterError> {
        let wall_utc = system_utc(SystemTime::now())?;
        let monotonic_ticks = u64::try_from(self.monotonic_origin.elapsed().as_nanos())
            .map_err(|_| AdapterError::new(AdapterErrorKind::ResourceLimit))?;
        Ok(ClockObservation {
            wall_utc,
            monotonic_ticks,
        })
    }
}

fn system_utc(now: SystemTime) -> Result<UtcInstant, AdapterError> {
    let (seconds, nanos) = match now.duration_since(UNIX_EPOCH) {
        Ok(duration) => (
            i64::try_from(duration.as_secs())
                .map_err(|_| AdapterError::new(AdapterErrorKind::ResourceLimit))?,
            duration.subsec_nanos(),
        ),
        Err(error) => {
            let duration = error.duration();
            let seconds = i64::try_from(duration.as_secs())
                .map_err(|_| AdapterError::new(AdapterErrorKind::ResourceLimit))?;
            if duration.subsec_nanos() == 0 {
                (-seconds, 0)
            } else {
                (
                    seconds
                        .checked_neg()
                        .and_then(|value| value.checked_sub(1))
                        .ok_or_else(|| AdapterError::new(AdapterErrorKind::ResourceLimit))?,
                    1_000_000_000 - duration.subsec_nanos(),
                )
            }
        }
    };
    UtcInstant::new(seconds, nanos).map_err(|_| AdapterError::new(AdapterErrorKind::ResourceLimit))
}

#[cfg(test)]
mod tests {
    use super::{
        LinuxQueryReport, LinuxRunReport, cache_delta, crash_probe_marker, create_crash_probe,
        percentile, status_value_kib, system_utc, validate_measured_summary,
    };
    use crate::{Bm01Profile, OracleSummary};
    use std::path::Path;
    use std::time::{Duration, UNIX_EPOCH};
    use uste_graph::GraphIndexCacheReport;

    #[test]
    fn system_clock_conversion_handles_both_sides_of_epoch() {
        assert_eq!(
            system_utc(UNIX_EPOCH + Duration::new(1, 2)).unwrap(),
            uste_types::UtcInstant::new(1, 2).unwrap()
        );
        assert_eq!(
            system_utc(UNIX_EPOCH - Duration::new(1, 2)).unwrap(),
            uste_types::UtcInstant::new(-2, 999_999_998).unwrap()
        );
    }

    #[test]
    fn report_is_content_free_and_discloses_uncontrolled_caches() {
        let report = LinuxRunReport {
            phase: "open",
            entities: 100_000,
            relationships: 1_000_000,
            frontier: 212,
            durable_revisions: 212,
            current_roots: 1,
            repaired_certificate_tail_bytes: 0,
            ignored_uncommitted_journal_bytes: 0,
            elapsed_milliseconds: 123,
        }
        .to_json();
        assert!(report.contains("qualification-candidate-environment-unverified"));
        assert!(report.contains("kernel_filesystem_device_cache\":\"uncontrolled"));
        assert!(report.contains("full_memory_graph_state\":true"));
        assert!(!report.contains('/'));
    }

    #[test]
    fn query_report_is_content_free_and_discloses_measurement_limits() {
        let report = LinuxQueryReport {
            entities: 100_000,
            relationships: 1_000_000,
            frontier: 212,
            queries: 384,
            successful_queries: 299,
            expected_visit_limits: 0,
            expected_result_limits: 85,
            setup_milliseconds: 10,
            query_milliseconds: 20,
            p50_nanoseconds: 30,
            p95_nanoseconds: 40,
            p99_nanoseconds: 50,
            visits: 60,
            logical_result_bytes: 70,
            current_rss_kib: 80,
            peak_rss_kib: 90,
            oracle_summary_digest: [1; 32],
            output_digest: [2; 32],
            cache_report: GraphIndexCacheReport {
                budget_bytes: 100,
                accounted_bytes: 10,
                hits: 1,
                misses: 2,
                evictions: 3,
                completed_authorized_reads: 4,
                completed_index_operations: 5,
                pages_read: 6,
                fragments_visited: 7,
                result_bytes: 8,
            },
        }
        .to_json();
        assert!(report.contains("qualification-candidate-correctness-only"));
        assert!(report.contains("\"engine_benchmark\":false"));
        assert!(report.contains("\"expected_result_limits\":85"));
        assert!(report.contains("uste_page_cache\":\"cleared-before-each-query"));
        assert!(report.contains("kernel_filesystem_device_cache\":\"uncontrolled"));
        assert!(!report.contains('/'));
    }

    #[test]
    fn measurement_helpers_are_checked_and_deterministic() {
        assert_eq!(percentile(&[1, 2, 3, 4], 50), 2);
        assert_eq!(percentile(&[1, 2, 3, 4], 95), 4);
        assert_eq!(percentile(&[], 99), 0);
        assert_eq!(
            status_value_kib("VmRSS:\t123 kB\nVmHWM:\t456 kB\n", "VmRSS:").unwrap(),
            123
        );
        assert!(status_value_kib("VmRSS:\t123 MB\n", "VmRSS:").is_err());

        let before = GraphIndexCacheReport {
            budget_bytes: 100,
            accounted_bytes: 20,
            hits: 1,
            misses: 2,
            evictions: 3,
            completed_authorized_reads: 4,
            completed_index_operations: 5,
            pages_read: 6,
            fragments_visited: 7,
            result_bytes: 8,
        };
        let after = GraphIndexCacheReport {
            budget_bytes: 100,
            accounted_bytes: 30,
            hits: 11,
            misses: 22,
            evictions: 33,
            completed_authorized_reads: 44,
            completed_index_operations: 55,
            pages_read: 66,
            fragments_visited: 77,
            result_bytes: 88,
        };
        let delta = cache_delta(before, after).unwrap();
        assert_eq!(delta.accounted_bytes, 30);
        assert_eq!(delta.hits, 10);
        assert_eq!(delta.misses, 20);
        assert_eq!(delta.result_bytes, 80);
        assert!(cache_delta(after, before).is_err());
    }

    #[test]
    fn query_correctness_rejects_wrong_profile_and_warmup_oracles() {
        let profile = Bm01Profile::new(20).unwrap();
        let other_profile = Bm01Profile::new(30).unwrap();
        let measured = OracleSummary::build(profile).unwrap();
        let warmup = OracleSummary::build_warmup(profile).unwrap();

        assert_eq!(
            validate_measured_summary(&measured, other_profile)
                .unwrap_err()
                .code(),
            "USTE_BM01_ORACLE_PROFILE"
        );
        assert_eq!(
            validate_measured_summary(&warmup, profile)
                .unwrap_err()
                .code(),
            "USTE_BM01_ORACLE_QUERY_SET"
        );
    }

    #[test]
    fn crash_probe_rejects_nonprefixes_before_io_and_marker_is_content_free() {
        let profile = Bm01Profile::new(20).unwrap();
        for revision in [0, 4, u64::MAX] {
            let error = create_crash_probe(
                Path::new("unused-root"),
                Path::new("unused-password"),
                profile,
                revision,
            )
            .unwrap_err();
            assert_eq!(error.code(), "USTE_BM01_CRASH_PROBE_REVISION");
        }
        let marker = crash_probe_marker(2, 4);
        assert_eq!(
            marker,
            "{\"schema\":\"bm01-linux-crash-probe-v1\",\"engine_benchmark\":false,\"phase\":\"durable-prefix-paused\",\"frontier\":2,\"planned_frontier\":4}"
        );
        assert!(!marker.contains('/'));
    }
}
