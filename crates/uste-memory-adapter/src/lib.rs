//! Restricted embedded Linux adapter for the bounded USTE memory pilot.
//!
//! The consumer remains authoritative for source bytes, approval generation and the durable
//! upload outbox. This crate exposes no raw coordinator, filesystem, snapshot or blob capability.

#![forbid(unsafe_code)]
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::{
    fmt,
    fs::File,
    os::fd::OwnedFd,
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};
use uste_crypto::{
    KeyVault, OsEntropy, PortableRecoveryAdapter, RecoveryEnvelope, RecoveryPassword,
};
use uste_memory::{
    Citation, MemoryMutation, MemoryReadError, MemoryReadOutput, MemoryReadRequest, MemoryState,
    MemoryTransaction, PILOT_PROFILE, SourceVersionId, SourceVersionInput, encode_transaction,
};
use uste_policy::{AuthenticatedPrincipal, PolicyKernel};
use uste_storage::{
    AdapterError, AdapterErrorKind, BLOB_CHUNK_BYTES, BlobInventory, BlobUploadToken, Clock,
    ClockObservation, EntryName,
    journal::StorageError,
    linux::{LinuxFileSystem, LinuxFilesystemProfile},
};
use uste_txn::{
    AuthorizedCoordinator, AuthorizedError, AuthorizedReadError, AuthorizedTransactionRequest,
    Cancellation, CommitCoordinator, NeverCancel, RetentionDays, TransactionError,
    TransactionOutcome, open_authorized,
};
use uste_types::{IdempotencyKey, NamespaceRef, TransactionId, UtcInstant};

const CHECKPOINT_SCHEMA_VERSION: u16 = 1;
const RETENTION_DAYS: u16 = 30;
const SOURCE_READ_CHUNK_BYTES: usize = 64 * 1024;

type Coordinator =
    AuthorizedCoordinator<MemoryState, LinuxFileSystem, RecoveryEnvelope, OsEntropy, OsEntropy>;

#[derive(Clone, Debug)]
pub struct LocalAdapterConfig {
    pub root: PathBuf,
    pub database_name: EntryName,
    pub scope: NamespaceRef,
    pub filesystem_profile: LinuxFilesystemProfile,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationIdentity {
    pub idempotency_key: IdempotencyKey,
    pub transaction_id: TransactionId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceEncoding {
    ExactUtf8,
    Opaque,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingSourceUpload {
    pub token: BlobUploadToken,
    pub source: SourceVersionId,
    pub encoding: SourceEncoding,
    pub source_event_time: Option<UtcInstant>,
    pub operation: OperationIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsumerCheckpoint {
    pub schema_version: u16,
    pub scope: NamespaceRef,
    pub authority_generation: u64,
    pub pending_uploads: Vec<PendingSourceUpload>,
}

impl ConsumerCheckpoint {
    pub fn validate(&self) -> Result<(), LocalAdapterError> {
        if self.schema_version != CHECKPOINT_SCHEMA_VERSION {
            return Err(LocalAdapterError::UnsupportedVersion);
        }
        if self.authority_generation == 0
            || self.pending_uploads.len() > PILOT_PROFILE.maximum_staged_uploads
            || self.pending_uploads.iter().any(|pending| {
                pending.token.scope() != self.scope
                    || pending.source.version == 0
                    || pending.source.source.database() != self.scope.database()
                    || pending.source.source.namespace() != self.scope.namespace()
            })
        {
            return Err(LocalAdapterError::InvalidCheckpoint);
        }
        let mut tokens = self
            .pending_uploads
            .iter()
            .map(|pending| pending.token)
            .collect::<Vec<_>>();
        tokens.sort_unstable_by_key(|token| token.upload_id());
        if tokens.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(LocalAdapterError::InvalidCheckpoint);
        }
        Ok(())
    }
}

/// Consumer-owned durable source and synchronization authority.
///
/// `persist_pending` must be durable before it returns. `source_bytes` must return the exact
/// immutable bytes for the named version. A failed `clear_pending` is safe: the next open retries
/// the same idempotent source commit before clearing it again.
pub trait ConsumerAuthority {
    fn checkpoint(&mut self) -> Result<ConsumerCheckpoint, LocalAdapterError>;

    fn source_bytes(&mut self, source: SourceVersionId) -> Result<Vec<u8>, LocalAdapterError>;

    fn persist_pending(&mut self, pending: &PendingSourceUpload) -> Result<(), LocalAdapterError>;

    fn clear_pending(&mut self, token: BlobUploadToken) -> Result<(), LocalAdapterError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedCitation {
    pub citation: Citation,
    pub exact_source_bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalAdapterError {
    InvalidConfiguration,
    InvalidCheckpoint,
    SourceChanged,
    Unauthorized,
    NotFound,
    Conflict,
    ResourceLimit,
    Cancelled,
    HistoryUnavailable,
    Rebuilding,
    StaleGeneration,
    StaleView,
    UnsupportedQuery,
    UnsupportedVersion,
    Locked,
    KeyOrIntegrityFailure,
    OutcomeUnknown,
    RetryableUnavailable,
}

impl LocalAdapterError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidConfiguration => "USTE_MEMORY_INVALID_CONFIGURATION",
            Self::InvalidCheckpoint => "USTE_MEMORY_INVALID_CHECKPOINT",
            Self::SourceChanged => "USTE_MEMORY_SOURCE_CHANGED",
            Self::Unauthorized => "USTE_MEMORY_UNAUTHORIZED",
            Self::NotFound => "USTE_MEMORY_NOT_FOUND",
            Self::Conflict => "USTE_MEMORY_CONFLICT",
            Self::ResourceLimit => "USTE_MEMORY_RESOURCE_LIMIT",
            Self::Cancelled => "USTE_MEMORY_CANCELLED",
            Self::HistoryUnavailable => "USTE_MEMORY_HISTORY_UNAVAILABLE",
            Self::Rebuilding => "USTE_MEMORY_REBUILDING",
            Self::StaleGeneration => "USTE_MEMORY_STALE_GENERATION",
            Self::StaleView => "USTE_MEMORY_STALE_VIEW",
            Self::UnsupportedQuery => "USTE_MEMORY_UNSUPPORTED_QUERY",
            Self::UnsupportedVersion => "USTE_MEMORY_UNSUPPORTED_VERSION",
            Self::Locked => "USTE_MEMORY_LOCKED",
            Self::KeyOrIntegrityFailure => "USTE_MEMORY_KEY_OR_INTEGRITY_FAILURE",
            Self::OutcomeUnknown => "USTE_MEMORY_OUTCOME_UNKNOWN",
            Self::RetryableUnavailable => "USTE_MEMORY_RETRYABLE_UNAVAILABLE",
        }
    }
}

impl fmt::Display for LocalAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for LocalAdapterError {}

/// One exclusively owned embedded memory-pilot writer. Reads return owned bounded values and do
/// not expose reusable snapshots or raw blob references as capabilities.
pub struct LocalMemoryAdapter {
    filesystem: LinuxFileSystem,
    coordinator: Coordinator,
    principal: AuthenticatedPrincipal,
    scope: NamespaceRef,
    authority_generation: u64,
    clock: SystemClock,
}

impl LocalMemoryAdapter {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        config: &LocalAdapterConfig,
        password: RecoveryPassword,
        policy: PolicyKernel,
        principal: AuthenticatedPrincipal,
        authority: &mut impl ConsumerAuthority,
        begin_operation: OperationIdentity,
    ) -> Result<Self, LocalAdapterError> {
        let checkpoint = checked_checkpoint(authority, config.scope)?;
        if !checkpoint.pending_uploads.is_empty() {
            return Err(LocalAdapterError::InvalidCheckpoint);
        }
        let mut filesystem = open_filesystem(&config.root, config.filesystem_profile)?;
        let mut key_adapter = PortableRecoveryAdapter::new(password);
        let vault = KeyVault::create(config.scope.database(), &mut key_adapter, OsEntropy)
            .map_err(|_| LocalAdapterError::KeyOrIntegrityFailure)?;
        let raw = CommitCoordinator::create(
            &mut filesystem,
            config.scope,
            retention()?,
            config.database_name.clone(),
            vault,
            OsEntropy,
            MemoryState::new(config.scope),
        )
        .map_err(map_transaction)?;
        let coordinator = AuthorizedCoordinator::new(raw, policy).map_err(map_authorized)?;
        let mut adapter = Self {
            filesystem,
            coordinator,
            principal,
            scope: config.scope,
            authority_generation: checkpoint.authority_generation,
            clock: SystemClock::new(),
        };
        adapter.commit_mutation(
            MemoryMutation::BeginRebuild {
                next_generation: checkpoint.authority_generation,
            },
            begin_operation,
            None,
            &NeverCancel,
        )?;
        Ok(adapter)
    }

    pub fn open(
        config: &LocalAdapterConfig,
        password: RecoveryPassword,
        policy: PolicyKernel,
        principal: AuthenticatedPrincipal,
        authority: &mut impl ConsumerAuthority,
    ) -> Result<Self, LocalAdapterError> {
        let checkpoint = checked_checkpoint(authority, config.scope)?;
        let mut filesystem = open_filesystem(&config.root, config.filesystem_profile)?;
        let mut key_adapter = PortableRecoveryAdapter::new(password);
        let (coordinator, _) = open_authorized(
            &mut filesystem,
            &config.database_name,
            config.scope,
            retention()?,
            OsEntropy,
            OsEntropy,
            &mut key_adapter,
            MemoryState::new(config.scope),
            policy,
        )
        .map_err(map_authorized)?;
        let mut adapter = Self {
            filesystem,
            coordinator,
            principal,
            scope: config.scope,
            authority_generation: checkpoint.authority_generation,
            clock: SystemClock::new(),
        };
        adapter.reconcile_pending(authority, checkpoint)?;
        Ok(adapter)
    }

    #[must_use]
    pub const fn authority_generation(&self) -> u64 {
        self.authority_generation
    }

    pub fn ingest_source(
        &mut self,
        authority: &mut impl ConsumerAuthority,
        source: SourceVersionId,
        encoding: SourceEncoding,
        source_event_time: Option<UtcInstant>,
        operation: OperationIdentity,
    ) -> Result<TransactionOutcome, LocalAdapterError> {
        let checkpoint = checked_checkpoint(authority, self.scope)?;
        if checkpoint.authority_generation != self.authority_generation
            || !checkpoint.pending_uploads.is_empty()
            || source.source.database() != self.scope.database()
            || source.source.namespace() != self.scope.namespace()
        {
            return Err(LocalAdapterError::InvalidCheckpoint);
        }
        let bytes = checked_source_bytes(authority, source)?;
        let upload = self
            .coordinator
            .start_blob_upload(&self.principal)
            .map_err(map_authorized)?;
        let pending = PendingSourceUpload {
            token: upload.token(),
            source,
            encoding,
            source_event_time,
            operation,
        };
        authority.persist_pending(&pending)?;
        self.finish_pending(authority, pending, upload, &bytes)
    }

    pub fn put_fact(
        &mut self,
        fact: uste_memory::FactInput,
        operation: OperationIdentity,
    ) -> Result<TransactionOutcome, LocalAdapterError> {
        self.commit_mutation(MemoryMutation::PutFact(fact), operation, None, &NeverCancel)
    }

    pub fn correct_fact(
        &mut self,
        target: uste_types::RecordRef,
        replacement: uste_memory::FactInput,
        operation: OperationIdentity,
    ) -> Result<TransactionOutcome, LocalAdapterError> {
        self.commit_mutation(
            MemoryMutation::CorrectFact {
                target,
                replacement,
            },
            operation,
            None,
            &NeverCancel,
        )
    }

    pub fn revoke_source(
        &mut self,
        source: SourceVersionId,
        operation: OperationIdentity,
    ) -> Result<TransactionOutcome, LocalAdapterError> {
        self.commit_mutation(
            MemoryMutation::RevokeSource { source },
            operation,
            None,
            &NeverCancel,
        )
    }

    pub fn complete_rebuild(
        &mut self,
        operation: OperationIdentity,
    ) -> Result<TransactionOutcome, LocalAdapterError> {
        self.commit_mutation(
            MemoryMutation::CompleteRebuild,
            operation,
            None,
            &NeverCancel,
        )
    }

    pub fn begin_rebuild(
        &mut self,
        authority: &mut impl ConsumerAuthority,
        operation: OperationIdentity,
    ) -> Result<TransactionOutcome, LocalAdapterError> {
        let checkpoint = checked_checkpoint(authority, self.scope)?;
        if !checkpoint.pending_uploads.is_empty()
            || self.authority_generation.checked_add(1) != Some(checkpoint.authority_generation)
        {
            return Err(LocalAdapterError::InvalidCheckpoint);
        }
        let canonical = encode_transaction(&MemoryTransaction {
            scope: self.scope,
            generation: checkpoint.authority_generation,
            mutation: MemoryMutation::BeginRebuild {
                next_generation: checkpoint.authority_generation,
            },
        })
        .map_err(|_| LocalAdapterError::InvalidConfiguration)?;
        let outcome = self
            .coordinator
            .commit(
                &mut self.filesystem,
                &self.principal,
                AuthorizedTransactionRequest {
                    idempotency_key: operation.idempotency_key,
                    transaction_id: operation.transaction_id,
                    canonical_request: &canonical,
                    blob_inventory: None,
                },
                &mut self.clock,
                &NeverCancel,
            )
            .map_err(map_authorized)?;
        self.authority_generation = checkpoint.authority_generation;
        Ok(outcome)
    }

    pub fn query(
        &mut self,
        request: &MemoryReadRequest,
        cancellation: &impl Cancellation,
    ) -> Result<MemoryReadOutput, LocalAdapterError> {
        if request_generation(request) != self.authority_generation {
            return Err(LocalAdapterError::StaleGeneration);
        }
        let view = self
            .coordinator
            .read_view(&self.principal)
            .map_err(map_authorized)?;
        self.coordinator
            .read_cancellable(&self.principal, &view, request, cancellation)
            .map_err(map_read)
    }

    pub fn resolve_citation(
        &mut self,
        request: &MemoryReadRequest,
    ) -> Result<ResolvedCitation, LocalAdapterError> {
        let MemoryReadOutput::Citation(citation) = self.query(request, &NeverCancel)? else {
            return Err(LocalAdapterError::InvalidConfiguration);
        };
        let length = usize::try_from(citation.blob.byte_len())
            .map_err(|_| LocalAdapterError::ResourceLimit)?;
        if citation.blob.byte_len() > PILOT_PROFILE.maximum_source_bytes_per_version {
            return Err(LocalAdapterError::ResourceLimit);
        }
        let mut bytes = vec![0_u8; length];
        let mut offset = 0_usize;
        while offset < bytes.len() {
            let end = offset
                .checked_add(SOURCE_READ_CHUNK_BYTES)
                .map_or(bytes.len(), |end| end.min(bytes.len()));
            let count = self
                .coordinator
                .read_blob_range(
                    &mut self.filesystem,
                    &self.principal,
                    citation.blob,
                    u64::try_from(offset).map_err(|_| LocalAdapterError::ResourceLimit)?,
                    &mut bytes[offset..end],
                )
                .map_err(map_authorized)?;
            if count == 0 || count > end - offset {
                return Err(LocalAdapterError::KeyOrIntegrityFailure);
            }
            offset = offset
                .checked_add(count)
                .ok_or(LocalAdapterError::ResourceLimit)?;
        }
        Ok(ResolvedCitation {
            citation,
            exact_source_bytes: bytes,
        })
    }

    fn reconcile_pending(
        &mut self,
        authority: &mut impl ConsumerAuthority,
        checkpoint: ConsumerCheckpoint,
    ) -> Result<(), LocalAdapterError> {
        if checkpoint.pending_uploads.is_empty() {
            self.coordinator
                .complete_recovered_upload_reconciliation(
                    &mut self.filesystem,
                    &self.principal,
                    &[],
                )
                .map_err(map_authorized)?;
            return Ok(());
        }
        for pending in &checkpoint.pending_uploads {
            let bytes = checked_source_bytes(authority, pending.source)?;
            let upload = self
                .coordinator
                .resume_blob_upload(&mut self.filesystem, &self.principal, pending.token)
                .map_err(map_authorized)?;
            self.finish_pending(authority, pending.clone(), upload, &bytes)?;
        }
        let tokens = checkpoint
            .pending_uploads
            .iter()
            .map(|pending| pending.token)
            .collect::<Vec<_>>();
        self.coordinator
            .complete_recovered_upload_reconciliation(
                &mut self.filesystem,
                &self.principal,
                &tokens,
            )
            .map_err(map_authorized)
    }

    fn finish_pending(
        &mut self,
        authority: &mut impl ConsumerAuthority,
        pending: PendingSourceUpload,
        mut upload: uste_txn::AuthorizedBlobUpload,
        bytes: &[u8],
    ) -> Result<TransactionOutcome, LocalAdapterError> {
        let accepted = usize::try_from(upload.accepted_bytes())
            .map_err(|_| LocalAdapterError::ResourceLimit)?;
        if accepted > bytes.len() {
            return Err(LocalAdapterError::SourceChanged);
        }
        for chunk in bytes[accepted..].chunks(BLOB_CHUNK_BYTES) {
            self.coordinator
                .write_blob_upload(&mut self.filesystem, &self.principal, &mut upload, chunk)
                .map_err(map_authorized)?;
        }
        let reference = self
            .coordinator
            .finish_blob_upload(&mut self.filesystem, &self.principal, &mut upload)
            .map_err(map_authorized)?;
        if reference.byte_len()
            != u64::try_from(bytes.len()).map_err(|_| LocalAdapterError::ResourceLimit)?
            || reference.content_digest() != <[u8; 32]>::from(Sha256::digest(bytes))
        {
            return Err(LocalAdapterError::SourceChanged);
        }
        let exact_utf8 = match pending.encoding {
            SourceEncoding::ExactUtf8 => Some(
                String::from_utf8(bytes.to_vec()).map_err(|_| LocalAdapterError::SourceChanged)?,
            ),
            SourceEncoding::Opaque => None,
        };
        let media_type = match pending.encoding {
            SourceEncoding::ExactUtf8 => uste_memory::TEXT_MEDIA_TYPE,
            SourceEncoding::Opaque => uste_memory::OPAQUE_MEDIA_TYPE,
        };
        let inventory = BlobInventory::new(self.scope, [reference])
            .map_err(|_| LocalAdapterError::InvalidConfiguration)?;
        let outcome = self.commit_mutation(
            MemoryMutation::PutSource(SourceVersionInput {
                id: pending.source,
                blob: reference,
                media_type: media_type.to_owned(),
                exact_utf8,
                source_event_time: pending.source_event_time,
            }),
            pending.operation,
            Some(&inventory),
            &NeverCancel,
        )?;
        authority.clear_pending(pending.token)?;
        Ok(outcome)
    }

    fn commit_mutation(
        &mut self,
        mutation: MemoryMutation,
        operation: OperationIdentity,
        inventory: Option<&BlobInventory>,
        cancellation: &impl Cancellation,
    ) -> Result<TransactionOutcome, LocalAdapterError> {
        let canonical = encode_transaction(&MemoryTransaction {
            scope: self.scope,
            generation: self.authority_generation,
            mutation,
        })
        .map_err(|_| LocalAdapterError::InvalidConfiguration)?;
        self.coordinator
            .commit(
                &mut self.filesystem,
                &self.principal,
                AuthorizedTransactionRequest {
                    idempotency_key: operation.idempotency_key,
                    transaction_id: operation.transaction_id,
                    canonical_request: &canonical,
                    blob_inventory: inventory,
                },
                &mut self.clock,
                cancellation,
            )
            .map_err(map_authorized)
    }
}

fn checked_checkpoint(
    authority: &mut impl ConsumerAuthority,
    scope: NamespaceRef,
) -> Result<ConsumerCheckpoint, LocalAdapterError> {
    let checkpoint = authority.checkpoint()?;
    checkpoint.validate()?;
    if checkpoint.scope != scope {
        return Err(LocalAdapterError::InvalidCheckpoint);
    }
    Ok(checkpoint)
}

fn checked_source_bytes(
    authority: &mut impl ConsumerAuthority,
    source: SourceVersionId,
) -> Result<Vec<u8>, LocalAdapterError> {
    let bytes = authority.source_bytes(source)?;
    if bytes.len()
        > usize::try_from(PILOT_PROFILE.maximum_source_bytes_per_version)
            .map_err(|_| LocalAdapterError::ResourceLimit)?
    {
        return Err(LocalAdapterError::ResourceLimit);
    }
    Ok(bytes)
}

fn request_generation(request: &MemoryReadRequest) -> u64 {
    match request {
        MemoryReadRequest::GetFact {
            authority_generation,
            ..
        }
        | MemoryReadRequest::Search {
            authority_generation,
            ..
        }
        | MemoryReadRequest::OneHop {
            authority_generation,
            ..
        }
        | MemoryReadRequest::ResolveCitation {
            authority_generation,
            ..
        }
        | MemoryReadRequest::Unsupported {
            authority_generation,
        } => *authority_generation,
    }
}

fn retention() -> Result<RetentionDays, LocalAdapterError> {
    RetentionDays::new(RETENTION_DAYS).map_err(|_| LocalAdapterError::InvalidConfiguration)
}

fn open_filesystem(
    root: &Path,
    profile: LinuxFilesystemProfile,
) -> Result<LinuxFileSystem, LocalAdapterError> {
    let directory: OwnedFd = File::open(root)
        .map_err(|_| LocalAdapterError::InvalidConfiguration)?
        .into();
    LinuxFileSystem::from_directory_for_profile(directory, profile).map_err(map_adapter)
}

fn map_adapter(error: AdapterError) -> LocalAdapterError {
    match error.kind() {
        AdapterErrorKind::OwnershipConflict => LocalAdapterError::Locked,
        AdapterErrorKind::NoSpace
        | AdapterErrorKind::QuotaExceeded
        | AdapterErrorKind::ResourceLimit => LocalAdapterError::ResourceLimit,
        AdapterErrorKind::Interrupted => LocalAdapterError::RetryableUnavailable,
        AdapterErrorKind::Unsupported => LocalAdapterError::UnsupportedVersion,
        _ => LocalAdapterError::InvalidConfiguration,
    }
}

fn map_storage(error: StorageError) -> LocalAdapterError {
    match error {
        StorageError::Adapter(kind) => map_adapter(AdapterError::new(kind)),
        StorageError::UnsupportedProfile => LocalAdapterError::UnsupportedVersion,
        StorageError::ResourceLimit | StorageError::RevisionExhausted => {
            LocalAdapterError::ResourceLimit
        }
        StorageError::NeedsRecovery => LocalAdapterError::OutcomeUnknown,
        StorageError::Crypto(_) | StorageError::IntegrityFailure | StorageError::InvalidState => {
            LocalAdapterError::KeyOrIntegrityFailure
        }
    }
}

fn map_transaction(error: TransactionError) -> LocalAdapterError {
    match error {
        TransactionError::Conflict | TransactionError::IdempotencyExpired => {
            LocalAdapterError::Conflict
        }
        TransactionError::SourceChanged => LocalAdapterError::SourceChanged,
        TransactionError::InvalidRequest => LocalAdapterError::InvalidConfiguration,
        TransactionError::ResourceLimit | TransactionError::RevisionExhausted => {
            LocalAdapterError::ResourceLimit
        }
        TransactionError::UnsupportedPredicate => LocalAdapterError::UnsupportedQuery,
        TransactionError::Cancelled => LocalAdapterError::Cancelled,
        TransactionError::OutcomeUnknown => LocalAdapterError::OutcomeUnknown,
        TransactionError::IntegrityFailure => LocalAdapterError::KeyOrIntegrityFailure,
        TransactionError::RetryableUnavailable => LocalAdapterError::RetryableUnavailable,
        TransactionError::Storage(error) => map_storage(error),
    }
}

fn map_authorized(error: AuthorizedError) -> LocalAdapterError {
    match error {
        AuthorizedError::Unauthorized | AuthorizedError::StalePolicy => {
            LocalAdapterError::Unauthorized
        }
        AuthorizedError::ResourceLimit => LocalAdapterError::ResourceLimit,
        AuthorizedError::InvalidPolicy => LocalAdapterError::InvalidConfiguration,
        AuthorizedError::IntegrityFailure => LocalAdapterError::KeyOrIntegrityFailure,
        AuthorizedError::Transaction(error) => map_transaction(error),
    }
}

fn map_read(error: AuthorizedReadError<MemoryReadError>) -> LocalAdapterError {
    match error {
        AuthorizedReadError::Authorization(error) => map_authorized(error),
        AuthorizedReadError::Domain(error) => match error {
            MemoryReadError::StaleGeneration => LocalAdapterError::StaleGeneration,
            MemoryReadError::StaleView => LocalAdapterError::StaleView,
            MemoryReadError::Rebuilding => LocalAdapterError::Rebuilding,
            MemoryReadError::HistoryUnavailable => LocalAdapterError::HistoryUnavailable,
            MemoryReadError::NotFound => LocalAdapterError::NotFound,
            MemoryReadError::ResourceLimit => LocalAdapterError::ResourceLimit,
            MemoryReadError::UnsupportedQuery => LocalAdapterError::UnsupportedQuery,
            MemoryReadError::IntegrityFailure => LocalAdapterError::KeyOrIntegrityFailure,
        },
    }
}

struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn observe(&mut self) -> Result<ClockObservation, AdapterError> {
        let wall_utc = system_utc(SystemTime::now())?;
        let monotonic_ticks = u64::try_from(self.origin.elapsed().as_nanos())
            .map_err(|_| AdapterError::new(AdapterErrorKind::ResourceLimit))?;
        Ok(ClockObservation {
            wall_utc,
            monotonic_ticks,
        })
    }
}

fn system_utc(now: SystemTime) -> Result<UtcInstant, AdapterError> {
    let (seconds, nanoseconds) = match now.duration_since(UNIX_EPOCH) {
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
    UtcInstant::new(seconds, nanoseconds)
        .map_err(|_| AdapterError::new(AdapterErrorKind::ResourceLimit))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use uste_storage::BlobUploadToken;
    use uste_types::{DatabaseId, NamespaceId, NamespaceRef};

    use super::{ConsumerCheckpoint, LocalAdapterError, system_utc};

    fn scope() -> NamespaceRef {
        NamespaceRef::new(
            DatabaseId::from_bytes([1; 16]),
            NamespaceId::from_bytes([2; 16]),
        )
    }

    #[test]
    fn checkpoint_version_scope_count_and_duplicate_tokens_fail_closed() {
        let mut checkpoint = ConsumerCheckpoint {
            schema_version: 2,
            scope: scope(),
            authority_generation: 1,
            pending_uploads: Vec::new(),
        };
        assert_eq!(
            checkpoint.validate(),
            Err(LocalAdapterError::UnsupportedVersion)
        );
        checkpoint.schema_version = 1;
        assert_eq!(checkpoint.validate(), Ok(()));

        let token = BlobUploadToken::from_upload_id(scope(), [3; 16]);
        let foreign = BlobUploadToken::from_upload_id(
            NamespaceRef::new(scope().database(), NamespaceId::from_bytes([4; 16])),
            [3; 16],
        );
        let source = uste_memory::SourceVersionId {
            source: uste_types::RecordRef::new(
                scope().database(),
                scope().namespace(),
                uste_types::RecordId::from_bytes([5; 16]),
            ),
            version: 1,
        };
        let pending = |token| super::PendingSourceUpload {
            token,
            source,
            encoding: super::SourceEncoding::Opaque,
            source_event_time: None,
            operation: super::OperationIdentity {
                idempotency_key: uste_types::IdempotencyKey::from_bytes([6; 16]),
                transaction_id: uste_types::TransactionId::from_bytes([7; 16]),
            },
        };
        checkpoint.pending_uploads = vec![pending(foreign)];
        assert_eq!(
            checkpoint.validate(),
            Err(LocalAdapterError::InvalidCheckpoint)
        );
        checkpoint.pending_uploads = vec![pending(token), pending(token)];
        assert_eq!(
            checkpoint.validate(),
            Err(LocalAdapterError::InvalidCheckpoint)
        );
    }

    #[test]
    fn system_clock_conversion_is_exact_on_both_sides_of_epoch() {
        assert_eq!(
            system_utc(UNIX_EPOCH + Duration::new(2, 3)).unwrap(),
            uste_types::UtcInstant::new(2, 3).unwrap()
        );
        assert_eq!(
            system_utc(UNIX_EPOCH - Duration::new(2, 3)).unwrap(),
            uste_types::UtcInstant::new(-3, 999_999_997).unwrap()
        );
    }
}
