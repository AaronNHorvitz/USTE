//! Logical records used by the independent reference model.

use core::fmt;
use core::num::NonZeroU64;

use uste_types::{BoundedString, CommitRevision, RecordRef, UtcInstant, Value};

/// Per-record logical version. Zero is never a visible version.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RecordVersion(NonZeroU64);

impl RecordVersion {
    /// Version assigned when a record is created.
    pub const FIRST: Self = Self(NonZeroU64::MIN);

    /// Construct a nonzero record version.
    pub const fn new(value: u64) -> Result<Self, RecordVersionError> {
        match NonZeroU64::new(value) {
            Some(value) => Ok(Self(value)),
            None => Err(RecordVersionError::Zero),
        }
    }

    /// Return the numeric version.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }

    pub(crate) const fn checked_next(self) -> Result<Self, RecordVersionError> {
        match self.get().checked_add(1) {
            Some(value) => Self::new(value),
            None => Err(RecordVersionError::Exhausted),
        }
    }
}

/// Failure to construct or advance a record version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordVersionError {
    /// Zero is reserved for absence.
    Zero,
    /// The version has no successor.
    Exhausted,
}

impl fmt::Display for RecordVersionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => formatter.write_str("record version zero is reserved"),
            Self::Exhausted => formatter.write_str("record version sequence is exhausted"),
        }
    }
}

impl std::error::Error for RecordVersionError {}

/// One endpoint of a half-open valid-time interval. Its start/end role supplies its direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntervalBound {
    /// The endpoint is explicitly unbounded.
    Unbounded,
    /// The endpoint is the given normalized UTC instant.
    Bounded(UtcInstant),
}

/// Valid time for an assertion. Unknown is deliberately distinct from an unbounded interval.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidTime {
    /// The source does not establish when the assertion is valid.
    Unknown,
    /// A half-open `[start, end)` interval with independently tagged endpoints.
    HalfOpen {
        /// Inclusive lower endpoint, or explicit negative infinity.
        start: IntervalBound,
        /// Exclusive upper endpoint, or explicit positive infinity.
        end: IntervalBound,
    },
}

impl ValidTime {
    /// Test membership while preserving the distinction between unknown and a known miss.
    #[must_use]
    pub fn contains(self, instant: UtcInstant) -> Option<bool> {
        match self {
            Self::Unknown => None,
            Self::HalfOpen { start, end } => {
                let after_start = match start {
                    IntervalBound::Unbounded => true,
                    IntervalBound::Bounded(start) => instant >= start,
                };
                let before_end = match end {
                    IntervalBound::Unbounded => true,
                    IntervalBound::Bounded(end) => instant < end,
                };
                Some(after_start && before_end)
            }
        }
    }

    pub(crate) fn is_valid(self) -> bool {
        match self {
            Self::Unknown => true,
            Self::HalfOpen {
                start: IntervalBound::Bounded(start),
                end: IntervalBound::Bounded(end),
            } => start < end,
            Self::HalfOpen { .. } => true,
        }
    }
}

/// Assertion lifecycle states in the accepted format-1.0 profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssertionStatus {
    /// Awaiting a decision.
    Proposed,
    /// Accepted by an authorized decision.
    Accepted,
    /// Rejected proposal; terminal.
    Rejected,
    /// Accepted assertion later disputed; terminal in-place.
    Disputed,
    /// Replaced by another assertion; terminal in-place.
    Superseded,
    /// Withdrawn accepted assertion; terminal in-place.
    Retracted,
    /// Accepted assertion whose declared lifetime ended; terminal in-place.
    Expired,
}

/// A requested assertion action. Correction creates another record; purge is not a lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssertionAction {
    Accept,
    Reject,
    Dispute,
    Supersede,
    Retract,
    Expire,
    Correct,
    Purge,
}

/// Visibility state for an entity record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntityLifecycle {
    Active,
    Deleted,
}

/// Explicit behavior when deleting an entity with current graph references.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeletePolicy {
    /// Reject when current relationships or assertions refer to the entity.
    Reject,
    /// Atomically retract accepted relationships and assertions, within the given bound.
    CascadeAndRetract {
        /// Maximum number of dependent records the operation may mutate.
        maximum_affected: usize,
    },
}

/// Input for a new entity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewEntity {
    pub id: RecordRef,
    pub properties: Value,
}

/// Input for immutable evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewEvidence {
    pub id: RecordRef,
    pub digest: [u8; 32],
    pub locator: BoundedString,
}

/// Input for a proposed assertion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewAssertion {
    pub id: RecordRef,
    pub subject: RecordRef,
    pub predicate: BoundedString,
    pub object: Value,
    pub evidence: Vec<RecordRef>,
    pub valid_time: ValidTime,
}

/// Input for a directed relationship.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewRelationship {
    pub id: RecordRef,
    pub from: RecordRef,
    pub to: RecordRef,
    pub relationship_type: BoundedString,
    pub properties: Value,
    pub evidence: Vec<RecordRef>,
    pub valid_time: ValidTime,
}

/// Record creation variants admitted by the oracle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NewRecord {
    Entity(NewEntity),
    Evidence(NewEvidence),
    Assertion(NewAssertion),
    Relationship(NewRelationship),
}

impl NewRecord {
    #[must_use]
    pub const fn id(&self) -> RecordRef {
        match self {
            Self::Entity(record) => record.id,
            Self::Evidence(record) => record.id,
            Self::Assertion(record) => record.id,
            Self::Relationship(record) => record.id,
        }
    }
}

/// Versioned entity state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityRecord {
    pub id: RecordRef,
    pub version: RecordVersion,
    pub lifecycle: EntityLifecycle,
    pub properties: Value,
    pub created_revision: CommitRevision,
    pub modified_revision: CommitRevision,
}

/// Immutable evidence state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceRecord {
    pub id: RecordRef,
    pub version: RecordVersion,
    pub digest: [u8; 32],
    pub locator: BoundedString,
    pub created_revision: CommitRevision,
}

/// Versioned assertion state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssertionRecord {
    pub id: RecordRef,
    pub version: RecordVersion,
    pub subject: RecordRef,
    pub predicate: BoundedString,
    pub object: Value,
    pub evidence: Vec<RecordRef>,
    pub status: AssertionStatus,
    pub valid_time: ValidTime,
    pub correction_of: Option<RecordRef>,
    pub recorded_revision: CommitRevision,
    pub modified_revision: CommitRevision,
}

/// Versioned directed relationship state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationshipRecord {
    pub id: RecordRef,
    pub version: RecordVersion,
    pub from: RecordRef,
    pub to: RecordRef,
    pub relationship_type: BoundedString,
    pub properties: Value,
    pub evidence: Vec<RecordRef>,
    pub status: AssertionStatus,
    pub valid_time: ValidTime,
    pub correction_of: Option<RecordRef>,
    pub recorded_revision: CommitRevision,
    pub modified_revision: CommitRevision,
}

/// A logical record held by the oracle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Record {
    Entity(EntityRecord),
    Evidence(EvidenceRecord),
    Assertion(AssertionRecord),
    Relationship(RelationshipRecord),
}

impl Record {
    #[must_use]
    pub const fn id(&self) -> RecordRef {
        match self {
            Self::Entity(record) => record.id,
            Self::Evidence(record) => record.id,
            Self::Assertion(record) => record.id,
            Self::Relationship(record) => record.id,
        }
    }

    #[must_use]
    pub const fn version(&self) -> RecordVersion {
        match self {
            Self::Entity(record) => record.version,
            Self::Evidence(record) => record.version,
            Self::Assertion(record) => record.version,
            Self::Relationship(record) => record.version,
        }
    }
}
