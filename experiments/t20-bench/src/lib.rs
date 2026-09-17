//! Deterministic BM-01 fixture semantics and an independent adjacency-array oracle.
//!
//! This experiment intentionally does not call the USTE engine or make performance claims.

#![forbid(unsafe_code)]

pub mod manifest;
pub mod materialization;
pub mod oracle;
pub mod query;
pub mod synthetic;

pub use manifest::Bm01Manifest;
pub use materialization::{
    ACCEPTED_SEED, Bm01Profile, Edge, EntityId, Materializer, RelationshipId, Topology,
};
pub use oracle::{Oracle, OracleError, OracleLimits, OracleOutput};
pub use query::{
    Direction, QueryClass, QuerySet, QuerySpec, measured_queries, query_digest, warmup_queries,
};
