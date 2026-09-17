//! Durable single-writer commit coordination over the authenticated USTE journal.
//!
//! Domain reducers remain separate: this crate orders their bounded canonical requests, validates
//! explicit prepared changes, durably binds retry outcomes, and publishes a coherent read snapshot.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use uste_crypto::{EntropySource, KeyAdapter};
use uste_storage::{
    BlobInventory, BlobReference, BlobUpload, BlobUploadToken, Clock, EMPTY_BLOB_INVENTORY_DIGEST,
    OwnershipFileSystem,
    journal::{
        CommitInput, CreationOptions, DurableKeyEnvelope, JournalStore, RecoveredGroup,
        RecoveryReport, StorageError,
    },
};
use uste_types::{CommitRevision, IdempotencyKey, NamespaceRef, TransactionId, UtcInstant};

const GROUP_HEADER_BYTES: usize = 192;
const GROUP_MAGIC: &[u8; 4] = b"UTXN";
const GROUP_MAJOR: u8 = 1;
const GROUP_MINOR: u8 = 0;
const SECONDS_PER_DAY: i64 = 86_400;
const MIN_RETENTION_DAYS: u16 = 30;
const MAX_RETENTION_DAYS: u16 = 365;
/// Maximum canonical reducer request admitted by the transaction group format.
pub const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024 - GROUP_HEADER_BYTES;
/// Accepted `limits-v1` cap for retained idempotency outcomes in one namespace.
pub const MAX_OUTCOMES_PER_NAMESPACE: usize = 10_000_000;

/// Stable digest of an authenticated principal identity, supplied by the trusted policy adapter.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PrincipalDigest([u8; 32]);

impl PrincipalDigest {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; 32] {
        self.0
    }
}

/// Retry-outcome retention fixed by Decision 0003.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionDays(u16);

impl RetentionDays {
    pub const fn new(days: u16) -> Result<Self, TransactionError> {
        if days < MIN_RETENTION_DAYS || days > MAX_RETENTION_DAYS {
            return Err(TransactionError::InvalidRequest);
        }
        Ok(Self(days))
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Domain reducer contract used by the coordinator and recovery.
///
/// `prepare` must be deterministic and side-effect free. `Prepared` and `Snapshot` must be owned
/// values without shared mutable access to live state. `publish` must be infallible and may only
/// apply the exact prepared change. These requirements make journal publication the only point
/// between private validation and visible state.
pub trait TransactionState {
    type Prepared;
    type Snapshot;

    fn prepare(
        &self,
        canonical_request: &[u8],
        blob_inventory: Option<&BlobInventory>,
        revision: CommitRevision,
    ) -> Result<Self::Prepared, ApplyError>;

    fn result_digest(prepared: &Self::Prepared) -> [u8; 32];

    fn publish(&mut self, prepared: Self::Prepared);

    fn snapshot(&self) -> Self::Snapshot;
}

/// Closed logical validation failures that precede journal publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyError {
    Conflict,
    InvalidRequest,
    ResourceLimit,
    UnsupportedPredicate,
}

/// Cooperative cancellation checked only before durable publication begins.
pub trait Cancellation {
    fn is_cancelled(&self) -> bool;
}

/// Cancellation source that never requests cancellation.
#[derive(Clone, Copy, Debug, Default)]
pub struct NeverCancel;

impl Cancellation for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// One bounded commit request. `canonical_request` is interpreted only by the selected reducer.
#[derive(Clone, Copy, Debug)]
pub struct TransactionRequest<'a> {
    pub principal: PrincipalDigest,
    pub idempotency_key: IdempotencyKey,
    pub transaction_id: TransactionId,
    pub canonical_request: &'a [u8],
    /// Newly referenced durable blobs, already finalized by this journal owner.
    pub blob_inventory: Option<&'a BlobInventory>,
}

/// Durable commit outcome returned identically for an eligible retry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransactionOutcome {
    pub transaction_id: TransactionId,
    pub revision: CommitRevision,
    pub request_digest: [u8; 32],
    pub result_digest: [u8; 32],
    pub expires_at: UtcInstant,
}

/// Stable transaction failure categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionError {
    Conflict,
    InvalidRequest,
    ResourceLimit,
    UnsupportedPredicate,
    Cancelled,
    OutcomeUnknown,
    IdempotencyExpired,
    RevisionExhausted,
    IntegrityFailure,
    RetryableUnavailable,
    Storage(StorageError),
}

/// Immutable coherent reader snapshot.
#[derive(Clone, Debug)]
pub struct ReadView<S> {
    revision: Option<CommitRevision>,
    state: S,
}

impl<S> ReadView<S> {
    #[must_use]
    pub const fn revision(&self) -> Option<CommitRevision> {
        self.revision
    }

    #[must_use]
    pub const fn state(&self) -> &S {
        &self.state
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RetryKey {
    principal: PrincipalDigest,
    key: IdempotencyKey,
}

/// One serialized commit coordinator and its fully recovered logical state.
pub struct CommitCoordinator<S, F, W, E, I>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    scope: NamespaceRef,
    retention: RetentionDays,
    journal: JournalStore<F, W, E, I>,
    state: S,
    outcomes: BTreeMap<RetryKey, TransactionOutcome>,
    transactions: BTreeMap<TransactionId, TransactionOutcome>,
    uncertain: bool,
}

impl<S, F, W, E, I> CommitCoordinator<S, F, W, E, I>
where
    S: TransactionState,
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
{
    pub fn create(
        filesystem: &mut F,
        scope: NamespaceRef,
        retention: RetentionDays,
        final_name: uste_storage::EntryName,
        vault: uste_crypto::KeyVault<W, E>,
        identity_entropy: I,
        initial_state: S,
    ) -> Result<Self, TransactionError> {
        let journal = JournalStore::create(
            filesystem,
            CreationOptions {
                database: scope.database(),
                final_name,
            },
            vault,
            identity_entropy,
        )
        .map_err(TransactionError::Storage)?;
        Ok(Self {
            scope,
            retention,
            journal,
            state: initial_state,
            outcomes: BTreeMap::new(),
            transactions: BTreeMap::new(),
            uncertain: false,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn open<A>(
        filesystem: &mut F,
        final_name: &uste_storage::EntryName,
        scope: NamespaceRef,
        retention: RetentionDays,
        vault_entropy: E,
        identity_entropy: I,
        key_adapter: &mut A,
        initial_state: S,
    ) -> Result<(Self, RecoveryReport), TransactionError>
    where
        A: KeyAdapter<Envelope = W>,
    {
        let mut state = initial_state;
        let mut outcomes = BTreeMap::new();
        let mut transactions = BTreeMap::new();
        let (journal, report) = JournalStore::open(
            filesystem,
            final_name,
            scope.database(),
            vault_entropy,
            identity_entropy,
            key_adapter,
            |group| replay_group(scope, &mut state, &mut outcomes, &mut transactions, group),
        )
        .map_err(map_open_error)?;
        Ok((
            Self {
                scope,
                retention,
                journal,
                state,
                outcomes,
                transactions,
                uncertain: false,
            },
            report,
        ))
    }

    pub fn read_view(&self) -> Result<ReadView<S::Snapshot>, TransactionError> {
        if self.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        Ok(ReadView {
            revision: self.journal.frontier(),
            state: self.state.snapshot(),
        })
    }

    pub fn start_blob_upload(
        &mut self,
        scope: NamespaceRef,
    ) -> Result<BlobUpload, TransactionError> {
        if self.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        if scope != self.scope {
            return Err(TransactionError::InvalidRequest);
        }
        self.journal
            .start_blob_upload(scope)
            .map_err(TransactionError::Storage)
    }

    pub fn resume_blob_upload(
        &mut self,
        filesystem: &mut F,
        token: BlobUploadToken,
    ) -> Result<BlobUpload, TransactionError> {
        if self.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        if token.scope() != self.scope {
            return Err(TransactionError::InvalidRequest);
        }
        self.journal
            .resume_blob_upload(filesystem, token)
            .map_err(TransactionError::Storage)
    }

    pub fn write_blob_upload(
        &mut self,
        filesystem: &mut F,
        upload: &mut BlobUpload,
        input: &[u8],
    ) -> Result<(), TransactionError> {
        if self.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        if upload.token().scope() != self.scope {
            return Err(TransactionError::InvalidRequest);
        }
        self.journal
            .write_blob_upload(filesystem, upload, input)
            .map_err(TransactionError::Storage)
    }

    pub fn finish_blob_upload(
        &mut self,
        filesystem: &mut F,
        upload: &mut BlobUpload,
    ) -> Result<BlobReference, TransactionError> {
        if self.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        if upload.token().scope() != self.scope {
            return Err(TransactionError::InvalidRequest);
        }
        self.journal
            .finish_blob_upload(filesystem, upload)
            .map_err(TransactionError::Storage)
    }

    pub fn abort_blob_upload(
        &mut self,
        filesystem: &mut F,
        upload: &mut BlobUpload,
    ) -> Result<(), TransactionError> {
        if self.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        if upload.token().scope() != self.scope {
            return Err(TransactionError::InvalidRequest);
        }
        self.journal
            .abort_blob_upload(filesystem, upload)
            .map_err(TransactionError::Storage)
    }

    pub fn read_blob_range(
        &self,
        filesystem: &mut F,
        reference: BlobReference,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, TransactionError> {
        if self.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        if reference.scope() != self.scope {
            return Err(TransactionError::InvalidRequest);
        }
        self.journal
            .read_blob_range(filesystem, reference, offset, output)
            .map_err(TransactionError::Storage)
    }

    pub fn outcome(
        &self,
        principal: PrincipalDigest,
        key: IdempotencyKey,
        clock: &mut impl Clock,
    ) -> Result<Option<TransactionOutcome>, TransactionError> {
        if self.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        let now = clock
            .observe()
            .map_err(|_| TransactionError::RetryableUnavailable)?
            .wall_utc;
        let outcome = self.outcomes.get(&RetryKey { principal, key }).copied();
        match outcome {
            Some(outcome) if now >= outcome.expires_at => Err(TransactionError::IdempotencyExpired),
            other => Ok(other),
        }
    }

    pub fn transaction_outcome(
        &self,
        transaction_id: TransactionId,
        clock: &mut impl Clock,
    ) -> Result<Option<TransactionOutcome>, TransactionError> {
        if self.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        let now = clock
            .observe()
            .map_err(|_| TransactionError::RetryableUnavailable)?
            .wall_utc;
        let outcome = self.transactions.get(&transaction_id).copied();
        match outcome {
            Some(outcome) if now >= outcome.expires_at => Err(TransactionError::IdempotencyExpired),
            other => Ok(other),
        }
    }

    pub fn commit(
        &mut self,
        filesystem: &mut F,
        request: TransactionRequest<'_>,
        clock: &mut impl Clock,
        cancellation: &impl Cancellation,
    ) -> Result<TransactionOutcome, TransactionError> {
        if self.uncertain {
            return Err(TransactionError::OutcomeUnknown);
        }
        if request.canonical_request.is_empty()
            || request.canonical_request.len() > MAX_REQUEST_BYTES
            || request.blob_inventory.is_some_and(BlobInventory::is_empty)
        {
            return Err(TransactionError::InvalidRequest);
        }
        if request
            .blob_inventory
            .is_some_and(|inventory| inventory.scope() != self.scope)
        {
            return Err(TransactionError::InvalidRequest);
        }
        let blob_inventory_digest = request
            .blob_inventory
            .map_or(EMPTY_BLOB_INVENTORY_DIGEST, BlobInventory::digest);
        let request_digest =
            transaction_request_digest(request.canonical_request, blob_inventory_digest);
        let accepted_at = clock
            .observe()
            .map_err(|_| TransactionError::RetryableUnavailable)?
            .wall_utc;
        let retry_key = RetryKey {
            principal: request.principal,
            key: request.idempotency_key,
        };
        if let Some(previous) = self.outcomes.get(&retry_key).copied() {
            if accepted_at >= previous.expires_at {
                return Err(TransactionError::IdempotencyExpired);
            }
            return if previous.request_digest == request_digest
                && previous.transaction_id == request.transaction_id
            {
                Ok(previous)
            } else {
                Err(TransactionError::Conflict)
            };
        }
        if self.transactions.contains_key(&request.transaction_id) {
            return Err(TransactionError::Conflict);
        }
        if self.outcomes.len() >= MAX_OUTCOMES_PER_NAMESPACE {
            return Err(TransactionError::ResourceLimit);
        }
        if cancellation.is_cancelled() {
            return Err(TransactionError::Cancelled);
        }
        let revision = match self.journal.frontier() {
            Some(revision) => revision
                .checked_next()
                .map_err(|_| TransactionError::RevisionExhausted)?,
            None => CommitRevision::FIRST,
        };
        let expires_at = expiration(accepted_at, self.retention)?;
        let prepared = self
            .state
            .prepare(request.canonical_request, request.blob_inventory, revision)
            .map_err(map_apply_error)?;
        let result_digest = S::result_digest(&prepared);
        if cancellation.is_cancelled() {
            return Err(TransactionError::Cancelled);
        }
        let outcome = TransactionOutcome {
            transaction_id: request.transaction_id,
            revision,
            request_digest,
            result_digest,
            expires_at,
        };
        let group = encode_group(self.scope, request, accepted_at, outcome)?;
        let logical_event_digest = sha256(&group);
        let commit_input = CommitInput {
            encoded_group: &group,
            logical_event_digest,
        };
        let durable = match request.blob_inventory {
            Some(inventory) if !inventory.is_empty() => {
                self.journal
                    .append_group_with_inventory(filesystem, commit_input, inventory)
            }
            _ => self.journal.append_group(filesystem, commit_input),
        };
        if let Err(error) = durable {
            let error = map_commit_error(error);
            if error == TransactionError::OutcomeUnknown {
                self.uncertain = true;
            }
            return Err(error);
        }
        self.state.publish(prepared);
        self.outcomes.insert(retry_key, outcome);
        self.transactions.insert(request.transaction_id, outcome);
        Ok(outcome)
    }
}

fn replay_group<S: TransactionState>(
    scope: NamespaceRef,
    state: &mut S,
    outcomes: &mut BTreeMap<RetryKey, TransactionOutcome>,
    transactions: &mut BTreeMap<TransactionId, TransactionOutcome>,
    group: RecoveredGroup<'_>,
) -> Result<(), StorageError> {
    if sha256(group.encoded_group) != group.logical_event_digest {
        return Err(StorageError::IntegrityFailure);
    }
    let decoded = decode_group(
        scope,
        group.encoded_group,
        group.revision,
        group.blob_inventory_digest,
        group.blob_inventory,
    )
    .map_err(|_| StorageError::IntegrityFailure)?;
    if outcomes.len() >= MAX_OUTCOMES_PER_NAMESPACE {
        return Err(StorageError::ResourceLimit);
    }
    let prepared = state
        .prepare(decoded.request, decoded.blob_inventory, group.revision)
        .map_err(|_| StorageError::IntegrityFailure)?;
    let result = S::result_digest(&prepared);
    if result != decoded.outcome.result_digest
        || outcomes
            .insert(decoded.retry_key, decoded.outcome)
            .is_some()
        || transactions
            .insert(decoded.outcome.transaction_id, decoded.outcome)
            .is_some()
    {
        return Err(StorageError::IntegrityFailure);
    }
    state.publish(prepared);
    Ok(())
}

#[derive(Debug)]
struct DecodedGroup<'a> {
    retry_key: RetryKey,
    outcome: TransactionOutcome,
    request: &'a [u8],
    blob_inventory: Option<&'a BlobInventory>,
}

fn encode_group(
    scope: NamespaceRef,
    request: TransactionRequest<'_>,
    accepted_at: UtcInstant,
    outcome: TransactionOutcome,
) -> Result<Vec<u8>, TransactionError> {
    let mut bytes = vec![0_u8; GROUP_HEADER_BYTES];
    bytes[..4].copy_from_slice(GROUP_MAGIC);
    bytes[4] = GROUP_MAJOR;
    bytes[5] = GROUP_MINOR;
    bytes[8..24].copy_from_slice(scope.namespace().as_bytes());
    bytes[24..56].copy_from_slice(&request.principal.as_bytes());
    bytes[56..72].copy_from_slice(request.idempotency_key.as_bytes());
    bytes[72..88].copy_from_slice(request.transaction_id.as_bytes());
    bytes[88..96].copy_from_slice(&accepted_at.seconds().to_be_bytes());
    bytes[96..100].copy_from_slice(&accepted_at.nanoseconds().to_be_bytes());
    bytes[100..108].copy_from_slice(&outcome.expires_at.seconds().to_be_bytes());
    bytes[108..112].copy_from_slice(&outcome.expires_at.nanoseconds().to_be_bytes());
    bytes[112..120].copy_from_slice(
        &u64::try_from(request.canonical_request.len())
            .map_err(|_| TransactionError::ResourceLimit)?
            .to_be_bytes(),
    );
    bytes[120..152].copy_from_slice(&outcome.request_digest);
    bytes[152..184].copy_from_slice(&outcome.result_digest);
    bytes
        .try_reserve_exact(request.canonical_request.len())
        .map_err(|_| TransactionError::ResourceLimit)?;
    bytes.extend_from_slice(request.canonical_request);
    Ok(bytes)
}

fn decode_group<'a>(
    scope: NamespaceRef,
    bytes: &'a [u8],
    revision: CommitRevision,
    blob_inventory_digest: [u8; 32],
    blob_inventory: Option<&'a BlobInventory>,
) -> Result<DecodedGroup<'a>, TransactionError> {
    if bytes.len() < GROUP_HEADER_BYTES
        || &bytes[..4] != GROUP_MAGIC
        || bytes[4] != GROUP_MAJOR
        || bytes[5] != GROUP_MINOR
        || bytes[6..8] != [0, 0]
        || bytes[8..24] != *scope.namespace().as_bytes()
        || bytes[184..192].iter().any(|byte| *byte != 0)
    {
        return Err(TransactionError::IntegrityFailure);
    }
    if blob_inventory.is_some_and(|inventory| inventory.scope() != scope) {
        return Err(TransactionError::IntegrityFailure);
    }
    let request_len =
        usize::try_from(read_u64(bytes, 112)?).map_err(|_| TransactionError::IntegrityFailure)?;
    if request_len == 0
        || request_len > MAX_REQUEST_BYTES
        || GROUP_HEADER_BYTES.checked_add(request_len) != Some(bytes.len())
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let request = &bytes[GROUP_HEADER_BYTES..];
    let request_digest = read_array(bytes, 120)?;
    if blob_inventory.map_or(EMPTY_BLOB_INVENTORY_DIGEST, BlobInventory::digest)
        != blob_inventory_digest
        || transaction_request_digest(request, blob_inventory_digest) != request_digest
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let accepted_at = read_instant(bytes, 88, 96)?;
    let expires_at = read_instant(bytes, 100, 108)?;
    let retention_seconds = expires_at.seconds().checked_sub(accepted_at.seconds());
    if expires_at.nanoseconds() != accepted_at.nanoseconds()
        || !matches!(
            retention_seconds,
            Some(value)
                if (i64::from(MIN_RETENTION_DAYS) * SECONDS_PER_DAY
                    ..=i64::from(MAX_RETENTION_DAYS) * SECONDS_PER_DAY)
                    .contains(&value)
                    && value % SECONDS_PER_DAY == 0
        )
    {
        return Err(TransactionError::IntegrityFailure);
    }
    let principal = PrincipalDigest(read_array(bytes, 24)?);
    let idempotency_key = IdempotencyKey::from_bytes(read_array(bytes, 56)?);
    let transaction_id = TransactionId::from_bytes(read_array(bytes, 72)?);
    Ok(DecodedGroup {
        retry_key: RetryKey {
            principal,
            key: idempotency_key,
        },
        outcome: TransactionOutcome {
            transaction_id,
            revision,
            request_digest,
            result_digest: read_array(bytes, 152)?,
            expires_at,
        },
        request,
        blob_inventory,
    })
}

fn expiration(
    accepted_at: UtcInstant,
    retention: RetentionDays,
) -> Result<UtcInstant, TransactionError> {
    let delta = i64::from(retention.get())
        .checked_mul(SECONDS_PER_DAY)
        .ok_or(TransactionError::ResourceLimit)?;
    let seconds = accepted_at
        .seconds()
        .checked_add(delta)
        .ok_or(TransactionError::ResourceLimit)?;
    UtcInstant::new(seconds, accepted_at.nanoseconds()).map_err(|_| TransactionError::ResourceLimit)
}

fn read_instant(
    bytes: &[u8],
    seconds_at: usize,
    nanos_at: usize,
) -> Result<UtcInstant, TransactionError> {
    let seconds = i64::from_be_bytes(read_array(bytes, seconds_at)?);
    let nanos = u32::from_be_bytes(read_array(bytes, nanos_at)?);
    UtcInstant::new(seconds, nanos).map_err(|_| TransactionError::IntegrityFailure)
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, TransactionError> {
    Ok(u64::from_be_bytes(read_array(bytes, offset)?))
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], TransactionError> {
    bytes
        .get(
            offset
                ..offset
                    .checked_add(N)
                    .ok_or(TransactionError::IntegrityFailure)?,
        )
        .ok_or(TransactionError::IntegrityFailure)?
        .try_into()
        .map_err(|_| TransactionError::IntegrityFailure)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn transaction_request_digest(request: &[u8], blob_inventory_digest: [u8; 32]) -> [u8; 32] {
    if blob_inventory_digest == EMPTY_BLOB_INVENTORY_DIGEST {
        return sha256(request);
    }
    let mut hasher = Sha256::new();
    hasher.update(b"USTE transaction request+blob inventory v1");
    hasher.update(
        u64::try_from(request.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    hasher.update(request);
    hasher.update(blob_inventory_digest);
    hasher.finalize().into()
}

const fn map_apply_error(error: ApplyError) -> TransactionError {
    match error {
        ApplyError::Conflict => TransactionError::Conflict,
        ApplyError::InvalidRequest => TransactionError::InvalidRequest,
        ApplyError::ResourceLimit => TransactionError::ResourceLimit,
        ApplyError::UnsupportedPredicate => TransactionError::UnsupportedPredicate,
    }
}

const fn map_commit_error(error: StorageError) -> TransactionError {
    match error {
        StorageError::ResourceLimit => TransactionError::ResourceLimit,
        StorageError::RevisionExhausted => TransactionError::RevisionExhausted,
        _ => TransactionError::OutcomeUnknown,
    }
}

const fn map_open_error(error: StorageError) -> TransactionError {
    match error {
        StorageError::IntegrityFailure => TransactionError::IntegrityFailure,
        other => TransactionError::Storage(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uste_types::{DatabaseId, NamespaceId};

    fn scope() -> NamespaceRef {
        NamespaceRef::new(
            DatabaseId::from_bytes([1; 16]),
            NamespaceId::from_bytes([2; 16]),
        )
    }

    fn golden_group() -> Vec<u8> {
        include_str!("../../../acceptance/r1/txn-group-v1.hex")
            .trim()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let text = core::str::from_utf8(pair).unwrap();
                u8::from_str_radix(text, 16).unwrap()
            })
            .collect()
    }

    fn outcome() -> TransactionOutcome {
        let mut result_digest = [0_u8; 32];
        result_digest[..8].copy_from_slice(&5_i64.to_be_bytes());
        TransactionOutcome {
            transaction_id: TransactionId::from_bytes([5; 16]),
            revision: CommitRevision::FIRST,
            request_digest: sha256(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5]),
            result_digest,
            expires_at: UtcInstant::new(2_592_000, 123).unwrap(),
        }
    }

    #[test]
    fn literal_group_golden_and_decode_agree() {
        let request_bytes = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5];
        let encoded = encode_group(
            scope(),
            TransactionRequest {
                principal: PrincipalDigest::from_bytes([3; 32]),
                idempotency_key: IdempotencyKey::from_bytes([4; 16]),
                transaction_id: TransactionId::from_bytes([5; 16]),
                canonical_request: &request_bytes,
                blob_inventory: None,
            },
            UtcInstant::new(0, 123).unwrap(),
            outcome(),
        )
        .unwrap();
        assert_eq!(encoded, golden_group());

        let decoded = decode_group(
            scope(),
            &encoded,
            CommitRevision::FIRST,
            EMPTY_BLOB_INVENTORY_DIGEST,
            None,
        )
        .unwrap();
        assert_eq!(decoded.request, request_bytes);
        assert_eq!(
            decoded.retry_key.principal,
            PrincipalDigest::from_bytes([3; 32])
        );
        assert_eq!(decoded.retry_key.key, IdempotencyKey::from_bytes([4; 16]));
        assert_eq!(decoded.outcome, outcome());
    }

    #[test]
    fn malformed_group_fields_fail_closed() {
        let valid = golden_group();
        let mut cases = Vec::new();
        cases.push(valid[..GROUP_HEADER_BYTES - 1].to_vec());
        for offset in [0, 4, 5, 6, 8, 96, 108, 112, 120, 184, 191] {
            let mut bytes = valid.clone();
            bytes[offset] ^= 0x80;
            cases.push(bytes);
        }
        let mut zero_length = valid.clone();
        zero_length[112..120].fill(0);
        cases.push(zero_length);
        let mut invalid_nanos = valid.clone();
        invalid_nanos[96..100].copy_from_slice(&1_000_000_000_u32.to_be_bytes());
        cases.push(invalid_nanos);
        let mut non_day_retention = valid.clone();
        non_day_retention[100..108].copy_from_slice(&2_592_001_i64.to_be_bytes());
        cases.push(non_day_retention);
        let mut short_retention = valid.clone();
        short_retention[100..108].copy_from_slice(&2_505_600_i64.to_be_bytes());
        cases.push(short_retention);
        let mut long_retention = valid.clone();
        long_retention[100..108].copy_from_slice(&31_622_400_i64.to_be_bytes());
        cases.push(long_retention);

        for (case, bytes) in cases.iter().enumerate() {
            assert_eq!(
                decode_group(
                    scope(),
                    bytes,
                    CommitRevision::FIRST,
                    EMPTY_BLOB_INVENTORY_DIGEST,
                    None,
                )
                .unwrap_err(),
                TransactionError::IntegrityFailure,
                "malformed case {case} unexpectedly decoded"
            );
        }
    }
}
