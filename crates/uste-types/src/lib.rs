//! Stable, bounded value and identity types shared by USTE engine components.
//!
//! T-09 owns the production `uste-v1` public API. T-08 establishes only the crate and its
//! safe-Rust boundary.

#![forbid(unsafe_code)]

mod codec;
mod error;
mod id;
mod instant;
mod revision;
pub mod spatial;
mod value;

pub use codec::{
    BorrowedMapValue, FORMAT_MAJOR, FORMAT_MINOR, MAGIC, MAX_VALUE_PAYLOAD, VALUE_RECORD_KIND,
    decode_borrowed_map_value, decode_map_value, decode_value, encode_value, encoded_len,
};
pub use error::{DecodeError, EncodeError, IdentityParseError, ValidationError};
pub use id::{
    DatabaseId, IdempotencyKey, NamespaceId, NamespaceRef, RecordId, RecordRef, SourceEventId,
    SourceEventRef, TransactionId, TransactionRef,
};
pub use instant::{MAX_EPOCH_SECONDS, MIN_EPOCH_SECONDS, UtcInstant};
pub use revision::{CommitRevision, CommitRevisionError};
pub use value::{
    BoundedBytes, BoundedList, BoundedString, CanonicalMap, MAX_COLLECTION_ENTRIES,
    MAX_INLINE_BYTES, MAX_NESTING_DEPTH, MAX_VALUE_NODES, Value,
};
