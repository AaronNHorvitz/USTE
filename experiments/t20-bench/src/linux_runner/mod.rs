//! Linux/Btrfs materialization and recovery runner for the exact BM-01 graph mapping.
//!
//! This is qualification-candidate machinery, not a self-certifying benchmark. It intentionally
//! emits content-free reports and keeps filesystem paths and recovery metadata out of errors.

mod credential;

use std::{
    fmt,
    path::Path,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use rustix::fs::{Mode, OFlags, open};
use uste_crypto::{KeyVault, OsEntropy, PortableRecoveryAdapter, RecoveryEnvelope};
use uste_graph::{
    AssertionAction, Expected, GraphReadOutput, GraphReadRequest, GraphState, GraphTransaction,
    MAX_TRANSACTION_OPERATIONS, NewEntity, NewEvidence, NewRecord, NewRelationship, Operation,
    Record, RecordVersion, ValidTime, encode_transaction,
};
use uste_policy::{
    AuthenticatedPrincipal, AuthenticationError, PolicyKernel, TrustedPrincipalAdapter,
};
use uste_storage::{
    AdapterError, AdapterErrorKind, Clock, ClockObservation, EntryName, linux::LinuxFileSystem,
};
use uste_txn::{
    AuthorizedCoordinator, AuthorizedTransactionRequest, CommitCoordinator, MAX_REQUEST_BYTES,
    NeverCancel, RetentionDays, TransactionRequest,
};
use uste_types::{IdempotencyKey, TransactionId, UtcInstant, Value};

use crate::{
    Bm01Profile, Materializer,
    engine::{
        PRINCIPAL, benchmark_policy, entity_ref, evidence_ref, kernel, relationship_ref, scope,
        text,
    },
    engine_mapping_digest, materialization_revision_count,
};

const DATABASE_NAME: &str = "bm01-linux-engine";
const RETENTION_DAYS: u16 = 30;

type LinuxCoordinator =
    AuthorizedCoordinator<GraphState, LinuxFileSystem, RecoveryEnvelope, OsEntropy, OsEntropy>;
type LinuxRaw =
    CommitCoordinator<GraphState, LinuxFileSystem, RecoveryEnvelope, OsEntropy, OsEntropy>;

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
    let (mut coordinator, principal, frontier) = materialize(raw, &mut filesystem, profile)?;
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
    let (mut coordinator, principal, frontier) = materialize(raw, &mut filesystem, profile)?;
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
    let roots = coordinator
        .load_current_index_roots(&mut filesystem, &principal)
        .map_err(|_| LinuxRunnerError::new("USTE_BM01_INDEX_LOAD"))?
        .len();
    if roots != 1 {
        return Err(LinuxRunnerError::new("USTE_BM01_INDEX_ROOT_COUNT"));
    }
    require_profile_binding(&coordinator, &principal, &view, profile)?;
    Ok(report(
        "open",
        profile,
        frontier,
        roots,
        recovery.repaired_certificate_tail_bytes,
        recovery.ignored_uncommitted_journal_bytes,
        started.elapsed(),
    ))
}

fn materialize(
    mut raw: LinuxRaw,
    filesystem: &mut LinuxFileSystem,
    profile: Bm01Profile,
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
            *sequence = sequence
                .checked_add(1)
                .ok_or_else(|| LinuxRunnerError::new("USTE_BM01_SEQUENCE"))?;
        }
    }
    if !batch.is_empty() {
        commit_batch(coordinator, filesystem, principal, clock, *sequence, batch)?;
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
    use super::{LinuxRunReport, system_utc};
    use std::time::{Duration, UNIX_EPOCH};

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
}
