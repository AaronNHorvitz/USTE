//! Independent, deliberately simple specification oracle for USTE state transitions.
//!
//! This crate is test infrastructure, not a production reducer. It uses ordered in-memory maps,
//! clones state before every transaction, and retains complete logical snapshots so later storage
//! and graph implementations can be checked without sharing their algorithms.

#![forbid(unsafe_code)]

mod generator;
mod model;
mod record;
mod spatial;

pub use generator::{
    GeneratedHistory, GenerationError, MAX_GENERATED_ENTITY_TRANSACTIONS,
    MAX_GENERATED_GRAPH_GROUPS, generate_entity_history, generate_graph_history,
};
pub use model::{
    CommitReceipt, Expected, MAX_TRANSACTION_OPERATIONS, MAX_TRANSACTION_REFERENCES, Model,
    ModelError, Operation, Predicate, Transaction,
};
pub use record::{
    AssertionAction, AssertionRecord, AssertionStatus, DeletePolicy, EntityLifecycle, EntityRecord,
    EvidenceRecord, IntervalBound, NewAssertion, NewEntity, NewEvidence, NewRecord,
    NewRelationship, Record, RecordVersion, RecordVersionError, RelationshipRecord, ValidTime,
};
pub use spatial::{ReferenceFrame, ReferenceFrameError, ReferenceFrameHistory};
