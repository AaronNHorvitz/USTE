//! Pinned, capability-free time normalization and replay-stable source envelopes.
//!
//! Normalization never reads the host clock, environment, filesystem or network. Named-zone
//! resolution is performed exclusively against the embedded `tzdb-2026c` profile. Accepted
//! envelopes retain the normalized pair, so decoding and replay never resolve a zone again.

#![forbid(unsafe_code)]

mod codec;
mod model;
mod normalize;

pub use codec::{decode_envelope, encode_envelope, envelope_from_value, envelope_to_value};
pub use model::{
    AuthorizedAssumption, CALENDAR_PROFILE, ClockUncertainty, ENVELOPE_PROFILE, EpochUnit,
    FORMAT_PROFILE, FoldChoice, LocalPresentation, MAX_ASSUMPTION_BYTES, MAX_ASSUMPTIONS,
    MAX_LOCATOR_BYTES, MAX_ORIGINAL_BYTES, MAX_ZONE_BYTES, NORMALIZATION_PROFILE, PARSER_PROFILE,
    Resolution, ResolutionReason, ResolutionStatus, SourceDescriptor, SourcePrecision,
    TZDB_PROFILE_DIGEST, TZDB_VERSION, TimeError, TimeInput, TimeInterpretation, TimeScale,
    TimestampEnvelope, TimestampRole, TimezoneProfile,
};
pub use normalize::{TimeNormalizer, format_utc};
