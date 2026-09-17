//! Deterministic cold replay and canonical reducer-checkpoint verification.
//!
//! This crate has no filesystem, clock, parser, model or network capability. Persistent encrypted
//! cache publication remains owned by `uste-storage`; checkpoints never replace journal authority.

#![forbid(unsafe_code)]

use uste_storage::{
    BlobId, BlobInventory, BlobReference, CheckpointInput, MAX_CHECKPOINT_BYTES,
    MAX_COMMITTED_BLOBS_PER_JOURNAL, RecoveredCheckpoint,
};
use uste_txn::{
    CheckpointState, CheckpointStateError, CoordinatorRecoverySeed, MAX_OUTCOMES_PER_NAMESPACE,
    PrincipalDigest, TransactionError, TransactionOutcome,
};
use uste_types::{
    CommitRevision, DatabaseId, IdempotencyKey, NamespaceId, NamespaceRef, TransactionId,
    UtcInstant,
};

const COORDINATOR_MAGIC: &[u8; 8] = b"UCCP\0\x01\0\0";
const OUTCOME_BYTES: usize = 148;
const BLOB_OWNER_BYTES: usize = 92;

#[derive(Clone, Copy)]
pub struct ReplayEvent<'a> {
    pub revision: CommitRevision,
    pub canonical_request: &'a [u8],
    pub blob_inventory: Option<&'a BlobInventory>,
    pub expected_result_digest: [u8; 32],
}

impl core::fmt::Debug for ReplayEvent<'_> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("ReplayEvent")
            .field("revision", &self.revision)
            .field("canonical_request", &"[REDACTED]")
            .field("blob_inventory", &self.blob_inventory.is_some())
            .field("expected_result_digest", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ReplayReport {
    pub frontier: Option<CommitRevision>,
    pub applied_events: u64,
    pub logical_state_digest: [u8; 32],
}

impl core::fmt::Debug for ReplayReport {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("ReplayReport")
            .field("frontier", &self.frontier)
            .field("applied_events", &self.applied_events)
            .field("logical_state_digest", &"[REDACTED]")
            .finish()
    }
}

#[derive(Eq, PartialEq)]
pub struct ReducerCheckpoint {
    scope: NamespaceRef,
    revision: CommitRevision,
    reducer_profile: [u8; 32],
    logical_state_digest: [u8; 32],
    payload: Vec<u8>,
}

/// Metadata accompanying a reducer checkpoint stream. It is not a publication receipt and does
/// not make emitted bytes authoritative.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ReducerCheckpointMetadata {
    scope: NamespaceRef,
    revision: CommitRevision,
    reducer_profile: [u8; 32],
    logical_state_digest: [u8; 32],
}

impl core::fmt::Debug for ReducerCheckpointMetadata {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("ReducerCheckpointMetadata")
            .field("scope", &"[REDACTED]")
            .field("revision", &self.revision)
            .field("reducer_profile", &"[REDACTED]")
            .field("logical_state_digest", &"[REDACTED]")
            .finish()
    }
}

impl ReducerCheckpointMetadata {
    #[must_use]
    pub const fn scope(self) -> NamespaceRef {
        self.scope
    }

    #[must_use]
    pub const fn revision(self) -> CommitRevision {
        self.revision
    }

    #[must_use]
    pub const fn reducer_profile(self) -> [u8; 32] {
        self.reducer_profile
    }

    #[must_use]
    pub const fn logical_state_digest(self) -> [u8; 32] {
        self.logical_state_digest
    }
}

impl core::fmt::Debug for ReducerCheckpoint {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("ReducerCheckpoint")
            .field("scope", &"[REDACTED]")
            .field("revision", &self.revision)
            .field("reducer_profile", &"[REDACTED]")
            .field("logical_state_digest", &"[REDACTED]")
            .field("payload_len", &self.payload.len())
            .finish()
    }
}

impl ReducerCheckpoint {
    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.scope
    }

    #[must_use]
    pub const fn revision(&self) -> CommitRevision {
        self.revision
    }

    #[must_use]
    pub const fn reducer_profile(&self) -> &[u8; 32] {
        &self.reducer_profile
    }

    #[must_use]
    pub const fn logical_state_digest(&self) -> &[u8; 32] {
        &self.logical_state_digest
    }

    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// Canonical reducer plus coordinator retry/ownership cache ready for encrypted publication.
pub struct CoordinatorCheckpoint {
    scope: NamespaceRef,
    revision: CommitRevision,
    certificate_digest: [u8; 32],
    reducer_profile: [u8; 32],
    logical_state_digest: [u8; 32],
    payload: Vec<u8>,
}

impl core::fmt::Debug for CoordinatorCheckpoint {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("CoordinatorCheckpoint")
            .field("scope", &"[REDACTED]")
            .field("revision", &self.revision)
            .field("certificate_digest", &"[REDACTED]")
            .field("reducer_profile", &"[REDACTED]")
            .field("logical_state_digest", &"[REDACTED]")
            .field("payload_len", &self.payload.len())
            .finish()
    }
}

impl CoordinatorCheckpoint {
    #[must_use]
    pub fn storage_input(&self) -> CheckpointInput<'_> {
        CheckpointInput {
            scope: self.scope,
            revision: self.revision,
            certificate_digest: self.certificate_digest,
            reducer_profile: self.reducer_profile,
            logical_state_digest: self.logical_state_digest,
            payload: &self.payload,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayError {
    EmptyCheckpointState,
    RevisionGap {
        expected: CommitRevision,
        actual: CommitRevision,
    },
    ReducerRejected,
    ResultDigestMismatch(CommitRevision),
    Checkpoint(CheckpointStateError),
    CheckpointScopeMismatch,
    CheckpointRevisionMismatch,
    CheckpointProfileMismatch,
    CheckpointStateMismatch,
    NonCanonicalCheckpoint,
    EventCountExhausted,
}

impl From<CheckpointStateError> for ReplayError {
    fn from(error: CheckpointStateError) -> Self {
        Self::Checkpoint(error)
    }
}

impl core::fmt::Display for ReplayError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ReplayError {}

/// Replay exact accepted reducer inputs without clocks, workers or external data.
pub fn cold_replay<'a, S>(
    mut state: S,
    events: impl IntoIterator<Item = ReplayEvent<'a>>,
) -> Result<(S, ReplayReport), ReplayError>
where
    S: CheckpointState,
{
    let mut frontier = state.current_checkpoint_revision();
    let mut applied_events = 0_u64;
    for event in events {
        let expected = match frontier {
            Some(revision) => revision
                .checked_next()
                .map_err(|_| ReplayError::EventCountExhausted)?,
            None => CommitRevision::FIRST,
        };
        if event.revision != expected {
            return Err(ReplayError::RevisionGap {
                expected,
                actual: event.revision,
            });
        }
        let prepared = state
            .prepare(
                event.canonical_request,
                event.blob_inventory,
                event.revision,
            )
            .map_err(|_| ReplayError::ReducerRejected)?;
        if S::result_digest(&prepared) != event.expected_result_digest {
            return Err(ReplayError::ResultDigestMismatch(event.revision));
        }
        state.publish(prepared);
        if state.current_checkpoint_revision() != Some(event.revision) {
            return Err(ReplayError::CheckpointRevisionMismatch);
        }
        frontier = Some(event.revision);
        applied_events = applied_events
            .checked_add(1)
            .ok_or(ReplayError::EventCountExhausted)?;
    }
    if state.current_checkpoint_revision() != frontier {
        return Err(ReplayError::CheckpointRevisionMismatch);
    }
    let logical_state_digest = state.current_logical_state_digest()?;
    Ok((
        state,
        ReplayReport {
            frontier,
            applied_events,
            logical_state_digest,
        },
    ))
}

/// Capture reducer-owned canonical bytes. This object is not a durable publication receipt.
pub fn capture_reducer_checkpoint<S>(state: &S) -> Result<ReducerCheckpoint, ReplayError>
where
    S: CheckpointState,
{
    let mut payload = Vec::new();
    let metadata = stream_reducer_checkpoint(state, &mut |bytes| {
        payload
            .try_reserve(bytes.len())
            .map_err(|_| CheckpointStateError::ResourceLimit)?;
        payload.extend_from_slice(bytes);
        Ok(())
    })?;
    Ok(ReducerCheckpoint {
        scope: metadata.scope,
        revision: metadata.revision,
        reducer_profile: metadata.reducer_profile,
        logical_state_digest: metadata.logical_state_digest,
        payload,
    })
}

/// Emit reducer-owned canonical checkpoint bytes without first constructing an owned snapshot or
/// complete payload. The sink controls transport buffering; current format size limits still apply.
pub fn stream_reducer_checkpoint<S>(
    state: &S,
    sink: &mut dyn FnMut(&[u8]) -> Result<(), CheckpointStateError>,
) -> Result<ReducerCheckpointMetadata, ReplayError>
where
    S: CheckpointState,
{
    let revision = state
        .current_checkpoint_revision()
        .ok_or(ReplayError::EmptyCheckpointState)?;
    let metadata = ReducerCheckpointMetadata {
        scope: state.current_checkpoint_scope(),
        revision,
        reducer_profile: S::REDUCER_PROFILE,
        logical_state_digest: state.current_logical_state_digest()?,
    };
    state.encode_current_checkpoint_into(sink)?;
    Ok(metadata)
}

/// Decode, re-encode and compare a reducer cache with its declared metadata.
pub fn verify_reducer_checkpoint<S>(
    expected_scope: NamespaceRef,
    checkpoint: &ReducerCheckpoint,
) -> Result<S, ReplayError>
where
    S: CheckpointState,
{
    if checkpoint.scope != expected_scope {
        return Err(ReplayError::CheckpointScopeMismatch);
    }
    if checkpoint.reducer_profile != S::REDUCER_PROFILE {
        return Err(ReplayError::CheckpointProfileMismatch);
    }
    let state = S::decode_checkpoint(checkpoint.scope, checkpoint.revision, &checkpoint.payload)?;
    if state.current_checkpoint_scope() != checkpoint.scope {
        return Err(ReplayError::CheckpointScopeMismatch);
    }
    if state.current_checkpoint_revision() != Some(checkpoint.revision) {
        return Err(ReplayError::CheckpointRevisionMismatch);
    }
    if state.current_logical_state_digest()? != checkpoint.logical_state_digest {
        return Err(ReplayError::CheckpointStateMismatch);
    }
    if state.encode_current_checkpoint()? != checkpoint.payload {
        return Err(ReplayError::NonCanonicalCheckpoint);
    }
    Ok(state)
}

/// Capture all reducer and coordinator state needed to resume after the checkpoint revision.
pub fn capture_coordinator_checkpoint<S>(
    state: &S,
    anchor: (CommitRevision, [u8; 32]),
    outcomes: impl IntoIterator<Item = (PrincipalDigest, IdempotencyKey, TransactionOutcome)>,
    committed_blob_owners: impl IntoIterator<Item = (BlobReference, PrincipalDigest)>,
) -> Result<CoordinatorCheckpoint, ReplayError>
where
    S: CheckpointState,
{
    let reducer = capture_reducer_checkpoint(state)?;
    if reducer.revision != anchor.0 {
        return Err(ReplayError::CheckpointRevisionMismatch);
    }
    let mut payload = Vec::new();
    coordinator_extend(&mut payload, COORDINATOR_MAGIC)?;
    coordinator_extend(&mut payload, reducer.scope.database().as_bytes())?;
    coordinator_extend(&mut payload, reducer.scope.namespace().as_bytes())?;
    coordinator_extend(&mut payload, &reducer.revision.get().to_be_bytes())?;
    coordinator_extend(&mut payload, &anchor.1)?;
    coordinator_extend(&mut payload, &reducer.reducer_profile)?;
    coordinator_extend(&mut payload, &reducer.logical_state_digest)?;
    coordinator_frame(&mut payload, &reducer.payload)?;

    let outcome_count_at = payload.len();
    coordinator_extend(&mut payload, &[0; 8])?;
    let mut outcome_count = 0_u64;
    let mut previous_outcome = None;
    let mut transaction_ids = Vec::new();
    for (principal, key, outcome) in outcomes {
        let order = (principal, key);
        if previous_outcome.is_some_and(|previous| previous >= order)
            || outcome.revision > reducer.revision
        {
            return Err(ReplayError::Checkpoint(CheckpointStateError::Invalid));
        }
        if usize::try_from(outcome_count).map_err(|_| ReplayError::EventCountExhausted)?
            == MAX_OUTCOMES_PER_NAMESPACE
        {
            return Err(ReplayError::Checkpoint(CheckpointStateError::ResourceLimit));
        }
        previous_outcome = Some(order);
        outcome_count = outcome_count
            .checked_add(1)
            .ok_or(ReplayError::EventCountExhausted)?;
        transaction_ids
            .try_reserve(1)
            .map_err(|_| ReplayError::Checkpoint(CheckpointStateError::ResourceLimit))?;
        transaction_ids.push(outcome.transaction_id);
        coordinator_extend(&mut payload, &principal.as_bytes())?;
        coordinator_extend(&mut payload, key.as_bytes())?;
        coordinator_extend(&mut payload, outcome.transaction_id.as_bytes())?;
        coordinator_extend(&mut payload, &outcome.revision.get().to_be_bytes())?;
        coordinator_extend(&mut payload, &outcome.request_digest)?;
        coordinator_extend(&mut payload, &outcome.result_digest)?;
        coordinator_extend(&mut payload, &outcome.expires_at.seconds().to_be_bytes())?;
        coordinator_extend(
            &mut payload,
            &outcome.expires_at.nanoseconds().to_be_bytes(),
        )?;
    }
    transaction_ids.sort_unstable();
    if transaction_ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ReplayError::Checkpoint(CheckpointStateError::Invalid));
    }
    payload[outcome_count_at..outcome_count_at + 8].copy_from_slice(&outcome_count.to_be_bytes());

    let owner_count_at = payload.len();
    coordinator_extend(&mut payload, &[0; 8])?;
    let mut owner_count = 0_u64;
    let mut previous_owner = None;
    for (reference, principal) in committed_blob_owners {
        if usize::try_from(owner_count).map_err(|_| ReplayError::EventCountExhausted)?
            == MAX_COMMITTED_BLOBS_PER_JOURNAL
        {
            return Err(ReplayError::Checkpoint(CheckpointStateError::ResourceLimit));
        }
        if reference.scope() != reducer.scope {
            return Err(ReplayError::CheckpointScopeMismatch);
        }
        let order = (reference.scope(), reference.id());
        if previous_owner.is_some_and(|previous| previous >= order) {
            return Err(ReplayError::Checkpoint(CheckpointStateError::Invalid));
        }
        previous_owner = Some(order);
        owner_count = owner_count
            .checked_add(1)
            .ok_or(ReplayError::EventCountExhausted)?;
        coordinator_extend(&mut payload, &reference.id().as_bytes())?;
        coordinator_extend(&mut payload, &reference.byte_len().to_be_bytes())?;
        coordinator_extend(&mut payload, &reference.chunk_count().to_be_bytes())?;
        coordinator_extend(&mut payload, &reference.content_digest())?;
        coordinator_extend(&mut payload, &principal.as_bytes())?;
    }
    payload[owner_count_at..owner_count_at + 8].copy_from_slice(&owner_count.to_be_bytes());
    Ok(CoordinatorCheckpoint {
        scope: reducer.scope,
        revision: reducer.revision,
        certificate_digest: anchor.1,
        reducer_profile: reducer.reducer_profile,
        logical_state_digest: reducer.logical_state_digest,
        payload,
    })
}

/// Decode an authenticated storage candidate into a seed that `CommitCoordinator::open_seeded`
/// will independently compare with journal metadata and its exact certificate anchor.
pub fn decode_coordinator_checkpoint<S>(
    checkpoint: &RecoveredCheckpoint,
) -> Result<CoordinatorRecoverySeed<S>, ReplayError>
where
    S: CheckpointState,
{
    if checkpoint.payload().len() > MAX_CHECKPOINT_BYTES {
        return Err(ReplayError::Checkpoint(CheckpointStateError::ResourceLimit));
    }
    let mut cursor = CoordinatorCursor::new(checkpoint.payload());
    if cursor.read_array::<8>()? != *COORDINATOR_MAGIC {
        return Err(ReplayError::Checkpoint(
            CheckpointStateError::UnsupportedProfile,
        ));
    }
    let scope = NamespaceRef::new(
        DatabaseId::from_bytes(cursor.read_array()?),
        NamespaceId::from_bytes(cursor.read_array()?),
    );
    let revision = cursor.read_revision()?;
    let certificate_digest = cursor.read_array()?;
    let reducer_profile = cursor.read_array()?;
    let logical_state_digest = cursor.read_array()?;
    if scope != checkpoint.scope()
        || revision != checkpoint.revision()
        || certificate_digest != *checkpoint.certificate_digest()
        || reducer_profile != *checkpoint.reducer_profile()
        || logical_state_digest != *checkpoint.logical_state_digest()
    {
        return Err(ReplayError::CheckpointStateMismatch);
    }
    let reducer_bytes = cursor.read_frame()?;
    let mut reducer_payload = Vec::new();
    reducer_payload
        .try_reserve_exact(reducer_bytes.len())
        .map_err(|_| ReplayError::Checkpoint(CheckpointStateError::ResourceLimit))?;
    reducer_payload.extend_from_slice(reducer_bytes);
    let reducer = ReducerCheckpoint {
        scope,
        revision,
        reducer_profile,
        logical_state_digest,
        payload: reducer_payload,
    };
    let state = verify_reducer_checkpoint::<S>(scope, &reducer)?;

    let outcome_count = cursor.read_bounded_count(OUTCOME_BYTES)?;
    if outcome_count > MAX_OUTCOMES_PER_NAMESPACE {
        return Err(ReplayError::Checkpoint(CheckpointStateError::ResourceLimit));
    }
    let mut outcomes = Vec::new();
    let mut previous_outcome = None;
    for _ in 0..outcome_count {
        let principal = PrincipalDigest::from_bytes(cursor.read_array()?);
        let key = IdempotencyKey::from_bytes(cursor.read_array()?);
        let order = (principal, key);
        if previous_outcome.is_some_and(|previous| previous >= order) {
            return Err(ReplayError::Checkpoint(CheckpointStateError::Invalid));
        }
        previous_outcome = Some(order);
        let transaction_id = TransactionId::from_bytes(cursor.read_array()?);
        let outcome_revision = cursor.read_revision()?;
        let request_digest = cursor.read_array()?;
        let result_digest = cursor.read_array()?;
        let expires_at = UtcInstant::new(cursor.read_i64()?, cursor.read_u32()?)
            .map_err(|_| ReplayError::Checkpoint(CheckpointStateError::Invalid))?;
        outcomes
            .try_reserve(1)
            .map_err(|_| ReplayError::Checkpoint(CheckpointStateError::ResourceLimit))?;
        outcomes.push((
            principal,
            key,
            TransactionOutcome {
                transaction_id,
                revision: outcome_revision,
                request_digest,
                result_digest,
                expires_at,
            },
        ));
    }

    let owner_count = cursor.read_bounded_count(BLOB_OWNER_BYTES)?;
    if owner_count > MAX_COMMITTED_BLOBS_PER_JOURNAL {
        return Err(ReplayError::Checkpoint(CheckpointStateError::ResourceLimit));
    }
    let mut owners = Vec::new();
    let mut previous_owner = None;
    for _ in 0..owner_count {
        let id = BlobId::from_bytes(cursor.read_array()?);
        let byte_len = cursor.read_u64()?;
        let chunk_count = cursor.read_u32()?;
        let content_digest = cursor.read_array()?;
        let principal = PrincipalDigest::from_bytes(cursor.read_array()?);
        let reference = BlobReference::new(scope, id, byte_len, chunk_count, content_digest)
            .map_err(|_| ReplayError::Checkpoint(CheckpointStateError::Invalid))?;
        let order = (reference.scope(), reference.id());
        if previous_owner.is_some_and(|previous| previous >= order) {
            return Err(ReplayError::Checkpoint(CheckpointStateError::Invalid));
        }
        previous_owner = Some(order);
        owners
            .try_reserve(1)
            .map_err(|_| ReplayError::Checkpoint(CheckpointStateError::ResourceLimit))?;
        owners.push((reference, principal));
    }
    if !cursor.is_empty() {
        return Err(ReplayError::NonCanonicalCheckpoint);
    }
    CoordinatorRecoverySeed::from_authenticated_checkpoint(checkpoint, state, outcomes, owners)
        .map_err(coordinator_seed_error)
}

fn coordinator_seed_error(error: TransactionError) -> ReplayError {
    match error {
        TransactionError::ResourceLimit => {
            ReplayError::Checkpoint(CheckpointStateError::ResourceLimit)
        }
        _ => ReplayError::Checkpoint(CheckpointStateError::Invalid),
    }
}

fn coordinator_frame(output: &mut Vec<u8>, value: &[u8]) -> Result<(), ReplayError> {
    let length = u64::try_from(value.len())
        .map_err(|_| ReplayError::Checkpoint(CheckpointStateError::ResourceLimit))?;
    coordinator_extend(output, &length.to_be_bytes())?;
    coordinator_extend(output, value)
}

fn coordinator_extend(output: &mut Vec<u8>, value: &[u8]) -> Result<(), ReplayError> {
    let next = output
        .len()
        .checked_add(value.len())
        .ok_or(ReplayError::Checkpoint(CheckpointStateError::ResourceLimit))?;
    if next > MAX_CHECKPOINT_BYTES {
        return Err(ReplayError::Checkpoint(CheckpointStateError::ResourceLimit));
    }
    output
        .try_reserve(value.len())
        .map_err(|_| ReplayError::Checkpoint(CheckpointStateError::ResourceLimit))?;
    output.extend_from_slice(value);
    Ok(())
}

struct CoordinatorCursor<'a> {
    remaining: &'a [u8],
}

impl<'a> CoordinatorCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { remaining: bytes }
    }

    fn read(&mut self, length: usize) -> Result<&'a [u8], ReplayError> {
        let (value, remaining) = self
            .remaining
            .split_at_checked(length)
            .ok_or(ReplayError::Checkpoint(CheckpointStateError::Invalid))?;
        self.remaining = remaining;
        Ok(value)
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], ReplayError> {
        self.read(N)?
            .try_into()
            .map_err(|_| ReplayError::Checkpoint(CheckpointStateError::Invalid))
    }

    fn read_u64(&mut self) -> Result<u64, ReplayError> {
        Ok(u64::from_be_bytes(self.read_array()?))
    }

    fn read_i64(&mut self) -> Result<i64, ReplayError> {
        Ok(i64::from_be_bytes(self.read_array()?))
    }

    fn read_u32(&mut self) -> Result<u32, ReplayError> {
        Ok(u32::from_be_bytes(self.read_array()?))
    }

    fn read_revision(&mut self) -> Result<CommitRevision, ReplayError> {
        CommitRevision::new(self.read_u64()?)
            .map_err(|_| ReplayError::Checkpoint(CheckpointStateError::Invalid))
    }

    fn read_frame(&mut self) -> Result<&'a [u8], ReplayError> {
        let length = usize::try_from(self.read_u64()?)
            .map_err(|_| ReplayError::Checkpoint(CheckpointStateError::ResourceLimit))?;
        self.read(length)
    }

    fn read_bounded_count(&mut self, minimum_entry_bytes: usize) -> Result<usize, ReplayError> {
        let count = usize::try_from(self.read_u64()?)
            .map_err(|_| ReplayError::Checkpoint(CheckpointStateError::ResourceLimit))?;
        if count > self.remaining.len() / minimum_entry_bytes {
            return Err(ReplayError::Checkpoint(CheckpointStateError::Invalid));
        }
        Ok(count)
    }

    const fn is_empty(&self) -> bool {
        self.remaining.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uste_txn::{ApplyError, TransactionState};
    use uste_types::{DatabaseId, NamespaceId};

    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    struct Counter {
        revision: Option<CommitRevision>,
        value: u64,
    }

    impl TransactionState for Counter {
        type Prepared = Self;
        type Snapshot = Self;

        fn prepare(
            &self,
            canonical_request: &[u8],
            blob_inventory: Option<&BlobInventory>,
            revision: CommitRevision,
        ) -> Result<Self::Prepared, ApplyError> {
            if blob_inventory.is_some() || canonical_request.len() != 8 {
                return Err(ApplyError::InvalidRequest);
            }
            let delta = u64::from_be_bytes(canonical_request.try_into().unwrap());
            Ok(Self {
                revision: Some(revision),
                value: self
                    .value
                    .checked_add(delta)
                    .ok_or(ApplyError::ResourceLimit)?,
            })
        }

        fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
            let mut digest = [0; 32];
            digest[..8].copy_from_slice(&prepared.value.to_be_bytes());
            digest[8..16].copy_from_slice(
                &prepared
                    .revision
                    .expect("prepared revision")
                    .get()
                    .to_be_bytes(),
            );
            digest
        }

        fn publish(&mut self, prepared: Self::Prepared) {
            *self = prepared;
        }

        fn snapshot(&self) -> Self::Snapshot {
            self.clone()
        }
    }

    impl CheckpointState for Counter {
        const REDUCER_PROFILE: [u8; 32] = [0x43; 32];

        fn checkpoint_scope(_snapshot: &Self::Snapshot) -> NamespaceRef {
            scope()
        }

        fn checkpoint_revision(snapshot: &Self::Snapshot) -> Option<CommitRevision> {
            snapshot.revision
        }

        fn logical_state_digest(
            snapshot: &Self::Snapshot,
        ) -> Result<[u8; 32], CheckpointStateError> {
            if snapshot.revision.is_none() {
                return Ok([0x47; 32]);
            }
            if snapshot.value == u64::MAX {
                Err(CheckpointStateError::ResourceLimit)
            } else {
                Ok(Self::result_digest(snapshot))
            }
        }

        fn encode_checkpoint(snapshot: &Self::Snapshot) -> Result<Vec<u8>, CheckpointStateError> {
            let mut encoded = Vec::with_capacity(16);
            encoded.extend_from_slice(
                &snapshot
                    .revision
                    .ok_or(CheckpointStateError::Invalid)?
                    .get()
                    .to_be_bytes(),
            );
            encoded.extend_from_slice(&snapshot.value.to_be_bytes());
            Ok(encoded)
        }

        fn decode_checkpoint(
            _scope: NamespaceRef,
            revision: CommitRevision,
            encoded: &[u8],
        ) -> Result<Self, CheckpointStateError> {
            if encoded.len() != 16
                || u64::from_be_bytes(encoded[..8].try_into().unwrap()) != revision.get()
            {
                return Err(CheckpointStateError::Invalid);
            }
            Ok(Self {
                revision: Some(revision),
                value: u64::from_be_bytes(encoded[8..].try_into().unwrap()),
            })
        }
    }

    #[derive(Clone, Debug, Default)]
    struct BorrowedCounter(Counter);

    impl TransactionState for BorrowedCounter {
        type Prepared = Self;
        type Snapshot = Counter;

        fn prepare(
            &self,
            canonical_request: &[u8],
            blob_inventory: Option<&BlobInventory>,
            revision: CommitRevision,
        ) -> Result<Self::Prepared, ApplyError> {
            self.0
                .prepare(canonical_request, blob_inventory, revision)
                .map(Self)
        }

        fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
            Counter::result_digest(&prepared.0)
        }

        fn publish(&mut self, prepared: Self::Prepared) {
            *self = prepared;
        }

        fn snapshot(&self) -> Self::Snapshot {
            panic!("borrow-aware replay must not request an owned snapshot")
        }
    }

    impl CheckpointState for BorrowedCounter {
        const REDUCER_PROFILE: [u8; 32] = [0x44; 32];

        fn checkpoint_scope(_snapshot: &Self::Snapshot) -> NamespaceRef {
            scope()
        }

        fn checkpoint_revision(snapshot: &Self::Snapshot) -> Option<CommitRevision> {
            snapshot.revision
        }

        fn logical_state_digest(
            snapshot: &Self::Snapshot,
        ) -> Result<[u8; 32], CheckpointStateError> {
            Counter::logical_state_digest(snapshot)
        }

        fn encode_checkpoint(snapshot: &Self::Snapshot) -> Result<Vec<u8>, CheckpointStateError> {
            Counter::encode_checkpoint(snapshot)
        }

        fn decode_checkpoint(
            scope: NamespaceRef,
            revision: CommitRevision,
            encoded: &[u8],
        ) -> Result<Self, CheckpointStateError> {
            Counter::decode_checkpoint(scope, revision, encoded).map(Self)
        }

        fn current_checkpoint_scope(&self) -> NamespaceRef {
            scope()
        }

        fn current_checkpoint_revision(&self) -> Option<CommitRevision> {
            self.0.revision
        }

        fn current_logical_state_digest(&self) -> Result<[u8; 32], CheckpointStateError> {
            Counter::logical_state_digest(&self.0)
        }

        fn encode_current_checkpoint(&self) -> Result<Vec<u8>, CheckpointStateError> {
            Counter::encode_checkpoint(&self.0)
        }

        fn encode_current_checkpoint_into(
            &self,
            sink: &mut dyn FnMut(&[u8]) -> Result<(), CheckpointStateError>,
        ) -> Result<(), CheckpointStateError> {
            Counter::encode_checkpoint_into(&self.0, sink)
        }
    }

    #[derive(Clone, Debug, Default)]
    struct SnapshotStreamCounter(Counter);

    impl TransactionState for SnapshotStreamCounter {
        type Prepared = Self;
        type Snapshot = Counter;

        fn prepare(
            &self,
            canonical_request: &[u8],
            blob_inventory: Option<&BlobInventory>,
            revision: CommitRevision,
        ) -> Result<Self::Prepared, ApplyError> {
            self.0
                .prepare(canonical_request, blob_inventory, revision)
                .map(Self)
        }

        fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
            Counter::result_digest(&prepared.0)
        }

        fn publish(&mut self, prepared: Self::Prepared) {
            *self = prepared;
        }

        fn snapshot(&self) -> Self::Snapshot {
            self.0.clone()
        }
    }

    impl CheckpointState for SnapshotStreamCounter {
        const REDUCER_PROFILE: [u8; 32] = [0x45; 32];

        fn checkpoint_scope(_snapshot: &Self::Snapshot) -> NamespaceRef {
            scope()
        }

        fn checkpoint_revision(snapshot: &Self::Snapshot) -> Option<CommitRevision> {
            snapshot.revision
        }

        fn logical_state_digest(
            snapshot: &Self::Snapshot,
        ) -> Result<[u8; 32], CheckpointStateError> {
            Counter::logical_state_digest(snapshot)
        }

        fn encode_checkpoint(_snapshot: &Self::Snapshot) -> Result<Vec<u8>, CheckpointStateError> {
            panic!("current stream default must use the snapshot streaming hook")
        }

        fn encode_checkpoint_into(
            snapshot: &Self::Snapshot,
            sink: &mut dyn FnMut(&[u8]) -> Result<(), CheckpointStateError>,
        ) -> Result<(), CheckpointStateError> {
            sink(&snapshot.value.to_be_bytes())
        }

        fn decode_checkpoint(
            _scope: NamespaceRef,
            _revision: CommitRevision,
            _encoded: &[u8],
        ) -> Result<Self, CheckpointStateError> {
            Err(CheckpointStateError::UnsupportedProfile)
        }
    }

    #[derive(Clone)]
    struct WrongScope(Counter);

    impl TransactionState for WrongScope {
        type Prepared = Self;
        type Snapshot = Self;

        fn prepare(
            &self,
            canonical_request: &[u8],
            blob_inventory: Option<&BlobInventory>,
            revision: CommitRevision,
        ) -> Result<Self::Prepared, ApplyError> {
            self.0
                .prepare(canonical_request, blob_inventory, revision)
                .map(Self)
        }

        fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
            Counter::result_digest(&prepared.0)
        }

        fn publish(&mut self, prepared: Self::Prepared) {
            *self = prepared;
        }

        fn snapshot(&self) -> Self::Snapshot {
            self.clone()
        }
    }

    impl CheckpointState for WrongScope {
        const REDUCER_PROFILE: [u8; 32] = Counter::REDUCER_PROFILE;

        fn checkpoint_scope(_snapshot: &Self::Snapshot) -> NamespaceRef {
            NamespaceRef::new(
                DatabaseId::from_bytes([1; 16]),
                NamespaceId::from_bytes([3; 16]),
            )
        }

        fn checkpoint_revision(snapshot: &Self::Snapshot) -> Option<CommitRevision> {
            snapshot.0.revision
        }

        fn logical_state_digest(
            snapshot: &Self::Snapshot,
        ) -> Result<[u8; 32], CheckpointStateError> {
            Counter::logical_state_digest(&snapshot.0)
        }

        fn encode_checkpoint(snapshot: &Self::Snapshot) -> Result<Vec<u8>, CheckpointStateError> {
            Counter::encode_checkpoint(&snapshot.0)
        }

        fn decode_checkpoint(
            scope: NamespaceRef,
            revision: CommitRevision,
            encoded: &[u8],
        ) -> Result<Self, CheckpointStateError> {
            Counter::decode_checkpoint(scope, revision, encoded).map(Self)
        }
    }

    fn scope() -> NamespaceRef {
        NamespaceRef::new(
            DatabaseId::from_bytes([1; 16]),
            NamespaceId::from_bytes([2; 16]),
        )
    }

    fn event<'a>(revision: u64, before: u64, bytes: &'a [u8; 8]) -> ReplayEvent<'a> {
        let delta = u64::from_be_bytes(*bytes);
        let prepared = Counter {
            revision: Some(CommitRevision::new(revision).unwrap()),
            value: before + delta,
        };
        ReplayEvent {
            revision: CommitRevision::new(revision).unwrap(),
            canonical_request: bytes,
            blob_inventory: None,
            expected_result_digest: Counter::result_digest(&prepared),
        }
    }

    #[test]
    fn cold_replay_and_checkpoint_round_trip_are_exact() {
        let first_bytes = 4_u64.to_be_bytes();
        let second_bytes = 7_u64.to_be_bytes();
        let first = event(1, 0, &first_bytes);
        let second = event(2, 4, &second_bytes);
        let (state, report) = cold_replay(Counter::default(), [first, second]).unwrap();
        assert_eq!(report.frontier.unwrap().get(), 2);
        assert_eq!(report.applied_events, 2);
        assert_eq!(state.value, 11);
        let rendered = format!("{report:?}");
        assert!(rendered.contains("[REDACTED]"));
        assert!(!rendered.contains(&format!("{:?}", report.logical_state_digest)));
        let checkpoint = capture_reducer_checkpoint(&state).unwrap();
        assert_eq!(
            verify_reducer_checkpoint::<Counter>(scope(), &checkpoint).unwrap(),
            state
        );
    }

    #[test]
    fn replay_and_capture_use_borrowed_current_state_without_snapshot_clones() {
        let bytes = 9_u64.to_be_bytes();
        let prepared = BorrowedCounter(Counter {
            revision: Some(CommitRevision::FIRST),
            value: 9,
        });
        let event = ReplayEvent {
            revision: CommitRevision::FIRST,
            canonical_request: &bytes,
            blob_inventory: None,
            expected_result_digest: BorrowedCounter::result_digest(&prepared),
        };
        let (state, report) = cold_replay(BorrowedCounter::default(), [event]).unwrap();
        assert_eq!(report.frontier, Some(CommitRevision::FIRST));
        assert_eq!(report.applied_events, 1);
        let checkpoint = capture_reducer_checkpoint(&state).unwrap();
        assert_eq!(checkpoint.revision(), CommitRevision::FIRST);
        assert_eq!(
            checkpoint.payload(),
            &Counter::encode_checkpoint(&state.0).unwrap()
        );
        let mut streamed = Vec::new();
        let metadata = stream_reducer_checkpoint(&state, &mut |bytes| {
            streamed.extend_from_slice(bytes);
            Ok(())
        })
        .unwrap();
        assert_eq!(metadata.revision(), checkpoint.revision());
        assert_eq!(metadata.scope(), checkpoint.scope());
        assert_eq!(metadata.reducer_profile(), *checkpoint.reducer_profile());
        assert_eq!(
            metadata.logical_state_digest(),
            *checkpoint.logical_state_digest()
        );
        assert_eq!(streamed, checkpoint.payload());
    }

    #[test]
    fn current_stream_default_delegates_to_the_snapshot_streaming_hook() {
        let state = SnapshotStreamCounter(Counter {
            revision: Some(CommitRevision::FIRST),
            value: 27,
        });
        let mut streamed = Vec::new();
        let metadata = stream_reducer_checkpoint(&state, &mut |bytes| {
            streamed.extend_from_slice(bytes);
            Ok(())
        })
        .unwrap();
        assert_eq!(metadata.revision(), CommitRevision::FIRST);
        assert_eq!(streamed, 27_u64.to_be_bytes());
    }

    #[test]
    fn empty_replay_reports_a_deterministic_genesis_digest() {
        let (state, report) = cold_replay(Counter::default(), []).unwrap();
        assert_eq!(state, Counter::default());
        assert_eq!(report.frontier, None);
        assert_eq!(report.applied_events, 0);
        assert_eq!(report.logical_state_digest, [0x47; 32]);
    }

    #[test]
    fn gaps_and_result_mismatches_never_publish() {
        let bytes = 1_u64.to_be_bytes();
        let gap = event(2, 0, &bytes);
        assert!(matches!(
            cold_replay(Counter::default(), [gap]),
            Err(ReplayError::RevisionGap { .. })
        ));
        let mut wrong = event(1, 0, &bytes);
        wrong.expected_result_digest = [9; 32];
        assert_eq!(
            cold_replay(Counter::default(), [wrong]),
            Err(ReplayError::ResultDigestMismatch(CommitRevision::FIRST))
        );
    }

    #[test]
    fn fallible_logical_digest_never_panics_replay_or_capture() {
        let bytes = u64::MAX.to_be_bytes();
        assert!(matches!(
            cold_replay(Counter::default(), [event(1, 0, &bytes)]),
            Err(ReplayError::Checkpoint(CheckpointStateError::ResourceLimit))
        ));
        let state = Counter {
            revision: Some(CommitRevision::FIRST),
            value: u64::MAX,
        };
        assert_eq!(
            capture_reducer_checkpoint(&state),
            Err(ReplayError::Checkpoint(CheckpointStateError::ResourceLimit))
        );
    }

    #[test]
    fn coordinator_capture_rejects_future_outcomes_and_duplicate_transaction_ids() {
        let revision = CommitRevision::new(2).unwrap();
        let state = Counter {
            revision: Some(revision),
            value: 7,
        };
        let outcome = |owner: u8, outcome_revision: CommitRevision| {
            (
                PrincipalDigest::from_bytes([owner; 32]),
                IdempotencyKey::from_bytes([owner; 16]),
                TransactionOutcome {
                    transaction_id: TransactionId::from_bytes([0x71; 16]),
                    revision: outcome_revision,
                    request_digest: [owner; 32],
                    result_digest: [owner.wrapping_add(1); 32],
                    expires_at: UtcInstant::new(86_400, 0).unwrap(),
                },
            )
        };
        let no_owners = || core::iter::empty::<(BlobReference, PrincipalDigest)>();
        assert_eq!(
            capture_coordinator_checkpoint(
                &state,
                (revision, [0x72; 32]),
                [outcome(1, CommitRevision::new(3).unwrap())],
                no_owners(),
            )
            .unwrap_err(),
            ReplayError::Checkpoint(CheckpointStateError::Invalid)
        );
        assert_eq!(
            capture_coordinator_checkpoint(
                &state,
                (revision, [0x72; 32]),
                [outcome(1, CommitRevision::FIRST), outcome(2, revision),],
                no_owners(),
            )
            .unwrap_err(),
            ReplayError::Checkpoint(CheckpointStateError::Invalid)
        );
    }

    #[test]
    fn every_published_revision_and_decoded_scope_are_verified() {
        let bytes = 99_u64.to_be_bytes();
        let prepared_with_wrong_revision = Counter {
            revision: Some(CommitRevision::new(2).unwrap()),
            value: 99,
        };
        let wrong_revision_event = ReplayEvent {
            revision: CommitRevision::FIRST,
            canonical_request: &bytes,
            blob_inventory: None,
            expected_result_digest: Counter::result_digest(&prepared_with_wrong_revision),
        };
        #[derive(Clone, Default)]
        struct PublishesWrongRevision(Counter);
        impl TransactionState for PublishesWrongRevision {
            type Prepared = Counter;
            type Snapshot = Counter;

            fn prepare(
                &self,
                canonical_request: &[u8],
                blob_inventory: Option<&BlobInventory>,
                revision: CommitRevision,
            ) -> Result<Self::Prepared, ApplyError> {
                let mut prepared = self
                    .0
                    .prepare(canonical_request, blob_inventory, revision)?;
                prepared.revision = Some(revision.checked_next().unwrap());
                Ok(prepared)
            }

            fn result_digest(prepared: &Self::Prepared) -> [u8; 32] {
                Counter::result_digest(prepared)
            }

            fn publish(&mut self, prepared: Self::Prepared) {
                self.0 = prepared;
            }

            fn snapshot(&self) -> Self::Snapshot {
                self.0.clone()
            }
        }
        impl CheckpointState for PublishesWrongRevision {
            const REDUCER_PROFILE: [u8; 32] = Counter::REDUCER_PROFILE;

            fn checkpoint_scope(_snapshot: &Self::Snapshot) -> NamespaceRef {
                scope()
            }

            fn checkpoint_revision(snapshot: &Self::Snapshot) -> Option<CommitRevision> {
                snapshot.revision
            }

            fn logical_state_digest(
                snapshot: &Self::Snapshot,
            ) -> Result<[u8; 32], CheckpointStateError> {
                Counter::logical_state_digest(snapshot)
            }

            fn encode_checkpoint(
                snapshot: &Self::Snapshot,
            ) -> Result<Vec<u8>, CheckpointStateError> {
                Counter::encode_checkpoint(snapshot)
            }

            fn decode_checkpoint(
                scope: NamespaceRef,
                revision: CommitRevision,
                encoded: &[u8],
            ) -> Result<Self, CheckpointStateError> {
                Counter::decode_checkpoint(scope, revision, encoded).map(Self)
            }
        }
        assert!(matches!(
            cold_replay(PublishesWrongRevision::default(), [wrong_revision_event]),
            Err(ReplayError::CheckpointRevisionMismatch)
        ));

        let first_bytes = 1_u64.to_be_bytes();
        let state = cold_replay(Counter::default(), [event(1, 0, &first_bytes)])
            .unwrap()
            .0;
        let checkpoint = capture_reducer_checkpoint(&state).unwrap();
        assert!(matches!(
            verify_reducer_checkpoint::<WrongScope>(scope(), &checkpoint),
            Err(ReplayError::CheckpointScopeMismatch)
        ));
    }
}
