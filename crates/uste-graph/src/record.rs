//! Public graph record and mutation vocabulary.

use core::{fmt, num::NonZeroU64};

use uste_policy::{NamespacePolicy, PolicyVersion};
use uste_types::{BoundedString, CommitRevision, NamespaceRef, RecordRef, UtcInstant, Value};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RecordVersion(NonZeroU64);

impl RecordVersion {
    pub const FIRST: Self = Self(NonZeroU64::MIN);

    pub const fn new(value: u64) -> Result<Self, RecordVersionError> {
        match NonZeroU64::new(value) {
            Some(value) => Ok(Self(value)),
            None => Err(RecordVersionError::Zero),
        }
    }

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordVersionError {
    Zero,
    Exhausted,
}

impl fmt::Display for RecordVersionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for RecordVersionError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntervalBound {
    Unbounded,
    Bounded(UtcInstant),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidTime {
    Unknown,
    HalfOpen {
        start: IntervalBound,
        end: IntervalBound,
    },
}

impl ValidTime {
    #[must_use]
    pub fn contains(self, instant: UtcInstant) -> Option<bool> {
        match self {
            Self::Unknown => None,
            Self::HalfOpen { start, end } => Some(
                matches!(start, IntervalBound::Unbounded)
                    || matches!(start, IntervalBound::Bounded(value) if instant >= value),
            )
            .map(|after_start| {
                after_start
                    && (matches!(end, IntervalBound::Unbounded)
                        || matches!(end, IntervalBound::Bounded(value) if instant < value))
            }),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssertionStatus {
    Proposed,
    Accepted,
    Rejected,
    Disputed,
    Superseded,
    Retracted,
    Expired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssertionAction {
    Accept,
    Reject,
    Dispute,
    Supersede,
    Retract,
    Expire,
    Correct,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntityLifecycle {
    Active,
    Deleted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeletePolicy {
    Reject,
    CascadeAndRetract { maximum_affected: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewEntity {
    pub id: RecordRef,
    pub entity_type: BoundedString,
    pub schema_version: u64,
    pub properties: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewEvidence {
    pub id: RecordRef,
    pub digest: [u8; 32],
    pub locator: BoundedString,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewAssertion {
    pub id: RecordRef,
    pub subject: RecordRef,
    pub predicate: BoundedString,
    pub object: Value,
    pub evidence: Vec<RecordRef>,
    pub valid_time: ValidTime,
}

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityRecord {
    pub id: RecordRef,
    pub version: RecordVersion,
    pub lifecycle: EntityLifecycle,
    pub entity_type: BoundedString,
    pub schema_version: u64,
    pub properties: Value,
    pub created_revision: CommitRevision,
    pub modified_revision: CommitRevision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceRecord {
    pub id: RecordRef,
    pub version: RecordVersion,
    pub digest: [u8; 32],
    pub locator: BoundedString,
    pub created_revision: CommitRevision,
}

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

    #[must_use]
    pub const fn modified_revision(&self) -> CommitRevision {
        match self {
            Self::Entity(record) => record.modified_revision,
            Self::Evidence(record) => record.created_revision,
            Self::Assertion(record) => record.modified_revision,
            Self::Relationship(record) => record.modified_revision,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Predicate {
    RecordAbsent(RecordRef),
    RecordVisible(RecordRef),
    RecordVersion {
        record: RecordRef,
        version: RecordVersion,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Expected {
    Absent,
    Version(RecordVersion),
    ReadView {
        revision: CommitRevision,
        predicate: Predicate,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Operation {
    Create {
        expected: Expected,
        record: NewRecord,
    },
    ReplaceEntity {
        target: RecordRef,
        expected: Expected,
        properties: Value,
    },
    ActOnAssertion {
        target: RecordRef,
        expected: Expected,
        action: AssertionAction,
        correction: Option<NewAssertion>,
        correction_expected: Option<Expected>,
    },
    ActOnRelationship {
        target: RecordRef,
        expected: Expected,
        action: AssertionAction,
        correction: Option<NewRelationship>,
        correction_expected: Option<Expected>,
    },
    DeleteEntity {
        target: RecordRef,
        expected: Expected,
        policy: DeletePolicy,
        /// Exact sorted dependency set authorized for cascade mutation; empty for `Reject`.
        affected: Vec<RecordRef>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DurablePolicyMutation {
    Install {
        policy: NamespacePolicy,
    },
    Replace {
        expected: PolicyVersion,
        policy: NamespacePolicy,
    },
}

impl Operation {
    #[must_use]
    pub const fn target(&self) -> RecordRef {
        match self {
            Self::Create { record, .. } => record.id(),
            Self::ReplaceEntity { target, .. }
            | Self::ActOnAssertion { target, .. }
            | Self::ActOnRelationship { target, .. }
            | Self::DeleteEntity { target, .. } => *target,
        }
    }

    #[must_use]
    pub const fn expected(&self) -> &Expected {
        match self {
            Self::Create { expected, .. }
            | Self::ReplaceEntity { expected, .. }
            | Self::ActOnAssertion { expected, .. }
            | Self::ActOnRelationship { expected, .. }
            | Self::DeleteEntity { expected, .. } => expected,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphTransaction {
    scope: NamespaceRef,
    operations: Vec<Operation>,
    policy_mutation: Option<DurablePolicyMutation>,
}

impl GraphTransaction {
    #[must_use]
    pub fn new(scope: NamespaceRef, operations: Vec<Operation>) -> Self {
        Self {
            scope,
            operations,
            policy_mutation: None,
        }
    }

    #[must_use]
    pub fn with_policy_mutation(
        scope: NamespaceRef,
        operations: Vec<Operation>,
        policy_mutation: DurablePolicyMutation,
    ) -> Self {
        Self {
            scope,
            operations,
            policy_mutation: Some(policy_mutation),
        }
    }

    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.scope
    }

    #[must_use]
    pub fn operations(&self) -> &[Operation] {
        &self.operations
    }

    #[must_use]
    pub const fn policy_mutation(&self) -> Option<&DurablePolicyMutation> {
        self.policy_mutation.as_ref()
    }
}
