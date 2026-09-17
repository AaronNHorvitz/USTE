//! Handle-relative storage capabilities, encrypted journal/recovery and deterministic fault models.
//!
//! T-13 includes the qualified x86_64 Linux adapter and format-1.0 commit-certificate journal. The
//! in-memory adapter and fault scripts remain verification tools rather than host durability claims.

#![forbid(unsafe_code)]

mod adapter;
pub mod fault;
pub mod journal;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub mod linux;
pub mod memory;

pub use adapter::{
    AdapterError, AdapterErrorKind, Clock, ClockObservation, EntryName, EntryNameError,
    FileMetadata, FileSystem, MAX_ENTRY_NAME_BYTES, OwnershipFileSystem, RandomSource,
    RestartableFileSystem, read_exact_at, write_all_at,
};
