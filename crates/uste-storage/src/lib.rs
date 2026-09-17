//! Narrow host-capability contracts and deterministic storage fault models.
//!
//! This crate does not yet provide the supported Linux filesystem implementation or a journal.
//! T-13 will bind these contracts to reviewed handle-relative platform operations. The in-memory
//! adapter and fault scripts here are deterministic verification tools, not durability claims.

#![forbid(unsafe_code)]

mod adapter;
pub mod fault;
pub mod memory;

pub use adapter::{
    AdapterError, AdapterErrorKind, Clock, ClockObservation, EntryName, EntryNameError,
    FileMetadata, FileSystem, MAX_ENTRY_NAME_BYTES, RandomSource, RestartableFileSystem,
    read_exact_at, write_all_at,
};
