//! Bounded source-backed memory profile built on USTE's journal, blob and policy boundaries.
//!
//! This crate is an experimental derived-index profile. It does not make USTE an authoritative
//! source store and does not provide physical-erasure, larger-than-memory or production claims.

#![forbid(unsafe_code)]

mod profile;

pub use profile::{PILOT_PROFILE, PilotProfile, PilotProfileError};
