//! Deterministic BM-01 fixture semantics and an independent adjacency-array oracle.
//!
//! The bounded development verifier calls the production encrypted/authorized/durable engine but
//! intentionally does not make performance or BM-01 qualification claims.

#![forbid(unsafe_code)]

pub mod engine;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub mod linux_runner;
pub mod manifest;
pub mod materialization;
pub mod oracle;
pub mod oracle_bundle;
pub mod oracle_summary;
pub mod query;
pub mod recovery_materialization;
pub mod synthetic;

pub use engine::{
    DevelopmentVerification, DiskDevelopmentVerification, engine_mapping_digest,
    materialization_revision_count, verify_development_profile, verify_disk_development_profile,
};
pub use manifest::Bm01Manifest;
pub use materialization::{
    ACCEPTED_SEED, Bm01Profile, Edge, EntityId, Materializer, RelationshipId, Topology,
};
pub use oracle::{Oracle, OracleError, OracleLimits, OracleOutput};
pub use oracle_bundle::{
    MAX_ORACLE_BUNDLE_BYTES, ORACLE_BUNDLE_PROFILE, OracleBundle, QUALIFYING_ORACLE_BUNDLE_DIGEST,
};
pub use oracle_summary::{
    MAX_ORACLE_SUMMARY_BYTES, ORACLE_SUMMARY_PROFILE, OracleExpectation, OracleExpectedOutcome,
    OracleSummary, QUALIFYING_ORACLE_SUMMARY_DIGEST, QUALIFYING_WARMUP_SUMMARY_DIGEST,
    RESULT_SIZE_PROFILE, WARMUP_SUMMARY_PROFILE,
};
pub use query::{
    Direction, QueryClass, QuerySet, QuerySpec, measured_queries, query_digest, warmup_queries,
};
