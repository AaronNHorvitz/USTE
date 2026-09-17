//! Canonical normalized-instant primitive for `posix-utc-v1`.

use crate::ValidationError;

/// Earliest admitted POSIX floor second: 0001-01-01T00:00:00Z.
pub const MIN_EPOCH_SECONDS: i64 = -62_135_596_800;

/// Latest admitted POSIX floor second: 9999-12-31T23:59:59Z.
pub const MAX_EPOCH_SECONDS: i64 = 253_402_300_799;

/// A normalized POSIX UTC instant with nanosecond precision.
///
/// Negative fractional instants use floor seconds. For example, half a second before the Unix
/// epoch is `seconds = -1, nanoseconds = 500_000_000`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UtcInstant {
    seconds: i64,
    nanoseconds: u32,
}

impl UtcInstant {
    /// Construct an instant within the format-1.0 calendar range.
    pub const fn new(seconds: i64, nanoseconds: u32) -> Result<Self, ValidationError> {
        if seconds < MIN_EPOCH_SECONDS || seconds > MAX_EPOCH_SECONDS || nanoseconds > 999_999_999 {
            return Err(ValidationError::InvalidInstant);
        }
        Ok(Self {
            seconds,
            nanoseconds,
        })
    }

    /// Floor POSIX epoch seconds.
    #[must_use]
    pub const fn seconds(self) -> i64 {
        self.seconds
    }

    /// Nanoseconds after the floor second.
    #[must_use]
    pub const fn nanoseconds(self) -> u32 {
        self.nanoseconds
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_EPOCH_SECONDS, MIN_EPOCH_SECONDS, UtcInstant};

    #[test]
    fn instant_endpoints_and_negative_fraction_are_exact() {
        assert!(UtcInstant::new(MIN_EPOCH_SECONDS, 0).is_ok());
        assert!(UtcInstant::new(MAX_EPOCH_SECONDS, 999_999_999).is_ok());
        assert!(UtcInstant::new(MIN_EPOCH_SECONDS - 1, 0).is_err());
        assert!(UtcInstant::new(MAX_EPOCH_SECONDS + 1, 0).is_err());
        assert!(UtcInstant::new(0, 1_000_000_000).is_err());

        let half_before_epoch = UtcInstant::new(-1, 500_000_000).expect("valid instant");
        assert!(half_before_epoch < UtcInstant::new(0, 0).expect("epoch"));
    }
}
