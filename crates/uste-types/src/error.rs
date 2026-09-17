//! Structured validation and canonical-codec errors.

use core::fmt;

/// A bounded value violated an invariant before encoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationError {
    /// A byte or string value exceeded its v1 inline-byte cap.
    InlineValueTooLarge {
        /// Observed byte length.
        actual: usize,
        /// Maximum admitted byte length.
        maximum: usize,
    },
    /// A list or map exceeded its v1 entry cap.
    CollectionTooLarge {
        /// Observed entry count.
        actual: usize,
        /// Maximum admitted entry count.
        maximum: usize,
    },
    /// A nested list/map exceeded the v1 depth cap.
    NestingTooDeep {
        /// Observed container depth.
        actual: usize,
        /// Maximum admitted container depth.
        maximum: usize,
    },
    /// One generic value tree exceeded the aggregate node cap.
    TooManyValueNodes {
        /// Observed node count, stopped at the first violating node.
        actual: usize,
        /// Maximum admitted node count.
        maximum: usize,
    },
    /// A canonical map contained the same UTF-8 key more than once.
    DuplicateMapKey,
    /// A POSIX instant was outside the selected seconds/nanoseconds range.
    InvalidInstant,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InlineValueTooLarge { actual, maximum } => {
                write!(formatter, "inline value length {actual} exceeds {maximum}")
            }
            Self::CollectionTooLarge { actual, maximum } => {
                write!(formatter, "collection length {actual} exceeds {maximum}")
            }
            Self::NestingTooDeep { actual, maximum } => {
                write!(formatter, "container depth {actual} exceeds {maximum}")
            }
            Self::TooManyValueNodes { actual, maximum } => {
                write!(formatter, "value node count {actual} exceeds {maximum}")
            }
            Self::DuplicateMapKey => formatter.write_str("canonical map contains a duplicate key"),
            Self::InvalidInstant => formatter.write_str("instant is outside posix-utc-v1"),
        }
    }
}

impl std::error::Error for ValidationError {}

/// A validated value could not fit in a canonical v1 frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EncodeError {
    /// The complete payload exceeded the fixed cap for its record kind.
    PayloadTooLarge {
        /// Computed payload length, or `usize::MAX` after arithmetic overflow.
        actual: usize,
        /// Maximum admitted payload length.
        maximum: usize,
    },
    /// The value tree exceeded the aggregate canonical node cap.
    TooManyValueNodes {
        /// Observed node count, stopped at the first violating node.
        actual: usize,
        /// Maximum admitted node count.
        maximum: usize,
    },
    /// The process allocator could not reserve the bounded output buffer.
    ResourceLimit,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PayloadTooLarge { actual, maximum } => {
                write!(
                    formatter,
                    "encoded payload length {actual} exceeds {maximum}"
                )
            }
            Self::TooManyValueNodes { actual, maximum } => {
                write!(formatter, "value node count {actual} exceeds {maximum}")
            }
            Self::ResourceLimit => formatter.write_str("bounded output allocation failed"),
        }
    }
}

impl std::error::Error for EncodeError {}

/// An input byte sequence was not one canonical v1 value frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodeError {
    /// Input ended before the declared structure was complete.
    UnexpectedEof,
    /// The four-byte frame magic was not `USTE`.
    InvalidMagic,
    /// The record kind or format version is not understood.
    UnsupportedVersion {
        /// Encountered record-kind byte.
        record_kind: u8,
        /// Encountered format major.
        major: u8,
        /// Encountered format minor.
        minor: u8,
    },
    /// A length/count/integer ULEB128 was not the shortest possible encoding.
    NonMinimalInteger,
    /// A ULEB128 value exceeded its target representation.
    IntegerOverflow,
    /// A declared payload was larger than its record-kind cap.
    PayloadTooLarge {
        /// Declared payload length, saturated to `usize::MAX` if necessary.
        actual: usize,
        /// Maximum admitted payload length.
        maximum: usize,
    },
    /// A generic value used an unassigned tag.
    InvalidValueTag(u8),
    /// A signed integer used an invalid sign/magnitude combination.
    InvalidSignedInteger,
    /// A string or map key was not well-formed UTF-8.
    InvalidUtf8,
    /// A bytes/string length exceeded the inline cap.
    InlineValueTooLarge {
        /// Declared byte length.
        actual: usize,
        /// Maximum admitted byte length.
        maximum: usize,
    },
    /// A list/map count exceeded the entry cap.
    CollectionTooLarge {
        /// Declared entry count.
        actual: usize,
        /// Maximum admitted entry count.
        maximum: usize,
    },
    /// Container nesting exceeded the v1 cap.
    NestingTooDeep {
        /// Observed container depth.
        actual: usize,
        /// Maximum admitted container depth.
        maximum: usize,
    },
    /// One generic value tree exceeded the aggregate node cap.
    TooManyValueNodes {
        /// Minimum node count proven by the input.
        actual: usize,
        /// Maximum admitted node count.
        maximum: usize,
    },
    /// The process allocator could not reserve a bounded decoded object.
    ResourceLimit,
    /// Map keys were duplicated or not in unsigned UTF-8 byte order.
    MapKeysOutOfOrder,
    /// A decoded instant violated the seconds/nanoseconds range.
    InvalidInstant,
    /// A scoped reference contained the wrong identity-kind tag.
    InvalidIdentityTag {
        /// Required identity-kind tag.
        expected: u8,
        /// Encountered identity-kind tag.
        actual: u8,
    },
    /// Canonical content was followed by undeclared bytes.
    TrailingBytes,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => formatter.write_str("canonical input ended unexpectedly"),
            Self::InvalidMagic => formatter.write_str("canonical frame magic is invalid"),
            Self::UnsupportedVersion {
                record_kind,
                major,
                minor,
            } => write!(
                formatter,
                "unsupported canonical frame kind/version {record_kind:#04x}/{major}.{minor}"
            ),
            Self::NonMinimalInteger => formatter.write_str("integer encoding is not minimal"),
            Self::IntegerOverflow => formatter.write_str("integer encoding overflows its type"),
            Self::PayloadTooLarge { actual, maximum } => {
                write!(
                    formatter,
                    "declared payload length {actual} exceeds {maximum}"
                )
            }
            Self::InvalidValueTag(tag) => write!(formatter, "invalid value tag {tag:#04x}"),
            Self::InvalidSignedInteger => formatter.write_str("invalid signed integer encoding"),
            Self::InvalidUtf8 => formatter.write_str("string bytes are not valid UTF-8"),
            Self::InlineValueTooLarge { actual, maximum } => {
                write!(formatter, "inline value length {actual} exceeds {maximum}")
            }
            Self::CollectionTooLarge { actual, maximum } => {
                write!(formatter, "collection length {actual} exceeds {maximum}")
            }
            Self::NestingTooDeep { actual, maximum } => {
                write!(formatter, "container depth {actual} exceeds {maximum}")
            }
            Self::TooManyValueNodes { actual, maximum } => {
                write!(formatter, "value node count {actual} exceeds {maximum}")
            }
            Self::ResourceLimit => formatter.write_str("bounded decoded allocation failed"),
            Self::MapKeysOutOfOrder => {
                formatter.write_str("map keys are duplicate or out of canonical order")
            }
            Self::InvalidInstant => formatter.write_str("instant is outside posix-utc-v1"),
            Self::InvalidIdentityTag { expected, actual } => write!(
                formatter,
                "identity tag {actual:#04x} does not match expected {expected:#04x}"
            ),
            Self::TrailingBytes => formatter.write_str("canonical frame has trailing bytes"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Text parsing failed for a strongly typed identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityParseError;

impl fmt::Display for IdentityParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("identity must use its exact prefix and 32 lowercase hex digits")
    }
}

impl std::error::Error for IdentityParseError {}
