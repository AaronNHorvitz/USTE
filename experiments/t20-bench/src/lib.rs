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
pub mod query;
pub mod synthetic;

pub use engine::{
    DevelopmentVerification, engine_mapping_digest, materialization_revision_count,
    verify_development_profile,
};
pub use manifest::Bm01Manifest;
pub use materialization::{
    ACCEPTED_SEED, Bm01Profile, Edge, EntityId, Materializer, RelationshipId, Topology,
};
pub use oracle::{Oracle, OracleError, OracleLimits, OracleOutput};
pub use query::{
    Direction, QueryClass, QuerySet, QuerySpec, measured_queries, query_digest, warmup_queries,
};
