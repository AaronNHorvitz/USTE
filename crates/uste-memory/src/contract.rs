//! Producer-owned generic Rust memory consumer contract, `uste-memory-consumer` (DB-R04).
//!
//! This is the candidate contract a consumer (runtime or coordinator) programs against. It
//! restates the `research-memory-v1` operations as a transport-neutral, synchronous, in-process
//! Rust trait with typed outcomes. It defines no consumer policy: the consumer's trusted adapter
//! supplies authority through the underlying policy kernel, and USTE never fetches, executes or
//! grants anything a record describes. Consumer agreement and integration are separate work.

use uste_policy::NamespacePolicy;
use uste_txn::Cancellation;
use uste_types::{CommitRevision, UtcInstant};

use crate::SourceVersionId;
use crate::research::{
    ArtifactInput, ClaimInput, EdgeInput, FetchOutcome, Freshness, ResearchReadOutput,
    ResearchReadRequest, SourceKind,
};

pub mod conformance;

/// Stable contract name carried by every negotiation.
pub const MEMORY_CONTRACT_NAME: &str = "uste-memory-consumer";
/// The single contract version this producer implements.
pub const MEMORY_CONTRACT_VERSION: ContractVersion = ContractVersion { major: 1, minor: 0 };

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ContractVersion {
    pub major: u16,
    pub minor: u16,
}

/// Fail-closed negotiation: the major version must match and the requested minor version must
/// not exceed the producer's. Unknown names are refused.
pub fn negotiate_contract(
    name: &str,
    requested: ContractVersion,
) -> Result<ContractVersion, ContractError> {
    if name != MEMORY_CONTRACT_NAME
        || requested.major != MEMORY_CONTRACT_VERSION.major
        || requested.minor > MEMORY_CONTRACT_VERSION.minor
    {
        return Err(ContractError::UnsupportedVersion);
    }
    Ok(MEMORY_CONTRACT_VERSION)
}

/// Consumer-chosen stable identity of one write. Reusing it with the same request returns the
/// original receipt; reusing it with a different request is a conflict. Retries are honoured
/// within the producer's outcome-retention window.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OperationId {
    pub idempotency_key: [u8; 16],
    pub transaction_id: [u8; 16],
}

/// A source version as the consumer knows it. The producer stores `bytes` as an immutable blob
/// and binds the resulting reference; the consumer never handles blob capabilities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractSource {
    pub id: SourceVersionId,
    pub kind: SourceKind,
    pub locator_text: String,
    pub version_label: String,
    pub retrieved_at: UtcInstant,
    pub run_identity: [u8; 16],
    pub route_label: String,
    pub outcome: FetchOutcome,
    pub license_label: String,
    pub redistributable: bool,
    pub freshness: Freshness,
    /// Exact retained bytes and their media type; absent exactly when the outcome is
    /// `Inaccessible`.
    pub content: Option<ContractContent>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractContent {
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContractWrite {
    /// Start generation `next_generation` (the first is 1); all derived records are cleared.
    BeginGeneration {
        next_generation: u64,
    },
    PutSource {
        generation: u64,
        source: ContractSource,
    },
    /// Contract 1.0 admits artifacts without retained content; `artifact.content` must be absent.
    PutArtifact {
        generation: u64,
        artifact: ArtifactInput,
    },
    /// A claim whose `corrects` names an active claim supersedes it.
    PutClaim {
        generation: u64,
        claim: ClaimInput,
    },
    PutEdge {
        generation: u64,
        edge: EdgeInput,
    },
    RetractClaim {
        generation: u64,
        claim: uste_types::RecordRef,
    },
    ExpireClaim {
        generation: u64,
        claim: uste_types::RecordRef,
    },
    RevokeSource {
        generation: u64,
        source: SourceVersionId,
    },
    /// Make the generation queryable.
    CompleteGeneration {
        generation: u64,
    },
    /// Replace the namespace policy through the consumer's trusted authority. Existing read
    /// views then fail as `StalePolicy`.
    ReplacePolicy {
        expected_version: uste_policy::PolicyVersion,
        next: NamespacePolicy,
    },
}

/// Durable acknowledgement of a committed write; policy replacement has no journal revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteReceipt {
    pub revision: Option<CommitRevision>,
}

/// Stable, content-free contract errors. Every producer failure maps onto exactly one variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContractError {
    UnsupportedVersion,
    /// The principal lacks the required action or the record or namespace is outside its scope.
    Unauthorized,
    /// The read view predates a policy replacement; take a new view.
    StalePolicy,
    /// The read view predates a newer commit; take a new view.
    StaleView,
    /// The request names a generation that is not current.
    StaleGeneration,
    /// The current generation has not completed.
    Rebuilding,
    HistoryUnavailable,
    NotFound,
    /// Identity reuse, a missing or inactive reference, or an illegal lifecycle transition.
    Conflict,
    /// A structurally or semantically invalid request.
    InvalidRequest,
    /// Supplied content no longer matches what the request declares.
    SourceChanged,
    /// A frozen profile, quota or request limit would be exceeded.
    ResourceLimit,
    UnsupportedQuery,
    /// Cancellation was observed before publication; nothing was committed.
    Cancelled,
    /// Publication may or may not have happened; reopen before retrying.
    OutcomeUnknown,
    /// The operation identity is past the retention window.
    IdempotencyExpired,
    /// A transient condition; the same operation may be retried.
    Unavailable,
    IntegrityFailure,
    /// A storage or filesystem failure without further detail.
    Storage,
}

impl ContractError {
    /// Whether the unchanged operation may simply be retried.
    #[must_use]
    pub const fn is_retryable(self) -> bool {
        matches!(self, Self::Unavailable | Self::Storage)
    }
}

/// The generic Rust memory consumer contract. Implementations own one namespace and one
/// authenticated principal; every call is authorized against current policy.
pub trait MemoryConsumerContract {
    /// Opaque coherent read view bound to one revision and policy.
    type View;

    fn contract_version(&self) -> ContractVersion;

    fn write(
        &mut self,
        operation: OperationId,
        request: &ContractWrite,
        cancellation: &dyn Cancellation,
    ) -> Result<WriteReceipt, ContractError>;

    fn view(&self) -> Result<Self::View, ContractError>;

    fn read(
        &self,
        view: &Self::View,
        request: &ResearchReadRequest,
        cancellation: &dyn Cancellation,
    ) -> Result<ResearchReadOutput, ContractError>;
}

/// Wrapper that lets a `&dyn Cancellation` satisfy APIs taking `&impl Cancellation`.
pub struct DynCancellation<'a>(pub &'a dyn Cancellation);

impl Cancellation for DynCancellation<'_> {
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

/// Helper for producers: the research-record form of a contract source once its content has
/// been stored under `blob`.
#[must_use]
pub fn source_record(
    source: &ContractSource,
    blob: Option<uste_storage::BlobReference>,
) -> crate::research::SourceRecordInput {
    crate::research::SourceRecordInput {
        id: source.id,
        kind: source.kind,
        locator_text: source.locator_text.clone(),
        version_label: source.version_label.clone(),
        retrieved_at: source.retrieved_at,
        run_identity: source.run_identity,
        route_label: source.route_label.clone(),
        outcome: source.outcome.clone(),
        content: source.content.as_ref().zip(blob).map(|(content, blob)| {
            crate::research::RetainedContent {
                blob,
                media_type: content.media_type.clone(),
            }
        }),
        license_label: source.license_label.clone(),
        redistributable: source.redistributable,
        freshness: source.freshness,
    }
}

/// Whether a stored source version is exactly what `source` requests, comparing every
/// descriptive field and the content by length and SHA-256. Producers use this to answer a
/// retried `PutSource` without storing the bytes a second time.
#[must_use]
pub fn source_matches_view(
    source: &ContractSource,
    view: &crate::research::SourceVersionView,
) -> bool {
    use sha2::{Digest, Sha256};
    let content_matches = match &source.content {
        None => view.retained_bytes.is_none() && view.media_type.is_none(),
        Some(content) => {
            view.media_type.as_deref() == Some(content.media_type.as_str())
                && view.retained_bytes == u64::try_from(content.bytes.len()).ok()
                && view.content_digest == Some(Sha256::digest(&content.bytes).into())
        }
    };
    content_matches
        && view.id == source.id
        && view.kind == source.kind
        && view.locator_text == source.locator_text
        && view.version_label == source.version_label
        && view.retrieved_at == source.retrieved_at
        && view.run_identity == source.run_identity
        && view.route_label == source.route_label
        && view.outcome == source.outcome
        && view.license_label == source.license_label
        && view.redistributable == source.redistributable
        && view.freshness_policy == source.freshness
}
