//! Deterministic cold replay and canonical reducer-checkpoint verification.
//!
//! This crate has no filesystem, clock, parser, model or network capability. Persistent encrypted
//! cache publication remains owned by `uste-storage`; checkpoints never replace journal authority.

#![forbid(unsafe_code)]

use uste_storage::BlobInventory;
use uste_txn::{CheckpointState, CheckpointStateError};
use uste_types::{CommitRevision, NamespaceRef};

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
    let mut frontier = S::checkpoint_revision(&state.snapshot());
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
        if S::checkpoint_revision(&state.snapshot()) != Some(event.revision) {
            return Err(ReplayError::CheckpointRevisionMismatch);
        }
        frontier = Some(event.revision);
        applied_events = applied_events
            .checked_add(1)
            .ok_or(ReplayError::EventCountExhausted)?;
    }
    let snapshot = state.snapshot();
    if S::checkpoint_revision(&snapshot) != frontier {
        return Err(ReplayError::CheckpointRevisionMismatch);
    }
    Ok((
        state,
        ReplayReport {
            frontier,
            applied_events,
            logical_state_digest: S::logical_state_digest(&snapshot)?,
        },
    ))
}

/// Capture reducer-owned canonical bytes. This object is not a durable publication receipt.
pub fn capture_reducer_checkpoint<S>(state: &S) -> Result<ReducerCheckpoint, ReplayError>
where
    S: CheckpointState,
{
    let snapshot = state.snapshot();
    let revision = S::checkpoint_revision(&snapshot).ok_or(ReplayError::EmptyCheckpointState)?;
    Ok(ReducerCheckpoint {
        scope: S::checkpoint_scope(&snapshot),
        revision,
        reducer_profile: S::REDUCER_PROFILE,
        logical_state_digest: S::logical_state_digest(&snapshot)?,
        payload: S::encode_checkpoint(&snapshot)?,
    })
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
    let snapshot = state.snapshot();
    if S::checkpoint_scope(&snapshot) != checkpoint.scope {
        return Err(ReplayError::CheckpointScopeMismatch);
    }
    if S::checkpoint_revision(&snapshot) != Some(checkpoint.revision) {
        return Err(ReplayError::CheckpointRevisionMismatch);
    }
    if S::logical_state_digest(&snapshot)? != checkpoint.logical_state_digest {
        return Err(ReplayError::CheckpointStateMismatch);
    }
    if S::encode_checkpoint(&snapshot)? != checkpoint.payload {
        return Err(ReplayError::NonCanonicalCheckpoint);
    }
    Ok(state)
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
