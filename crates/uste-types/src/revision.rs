//! Monotonic committed-revision identity.

use core::fmt;
use core::num::NonZeroU64;

/// A committed database revision. Zero is reserved for the pre-commit state.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CommitRevision(NonZeroU64);

impl CommitRevision {
    /// First committed revision.
    pub const FIRST: Self = Self(NonZeroU64::MIN);

    /// Construct a committed revision, rejecting the reserved zero value.
    pub const fn new(value: u64) -> Result<Self, CommitRevisionError> {
        match NonZeroU64::new(value) {
            Some(value) => Ok(Self(value)),
            None => Err(CommitRevisionError::Zero),
        }
    }

    /// Return the underlying monotonic sequence number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }

    /// Advance exactly once, failing closed rather than wrapping at `u64::MAX`.
    pub const fn checked_next(self) -> Result<Self, CommitRevisionError> {
        match self.get().checked_add(1) {
            Some(value) => Self::new(value),
            None => Err(CommitRevisionError::Exhausted),
        }
    }
}

/// Invalid construction or advancement of a committed revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitRevisionError {
    /// Revision zero denotes the state before any commit and is not a commit identity.
    Zero,
    /// The 64-bit revision sequence has no successor.
    Exhausted,
}

impl fmt::Display for CommitRevisionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => formatter.write_str("commit revision zero is reserved"),
            Self::Exhausted => formatter.write_str("commit revision sequence is exhausted"),
        }
    }
}

impl std::error::Error for CommitRevisionError {}

#[cfg(test)]
mod tests {
    use super::{CommitRevision, CommitRevisionError};

    #[test]
    fn zero_is_reserved_and_successor_never_wraps() {
        assert_eq!(CommitRevision::new(0), Err(CommitRevisionError::Zero));
        assert_eq!(CommitRevision::FIRST.get(), 1);
        assert_eq!(CommitRevision::FIRST.checked_next().map(|r| r.get()), Ok(2));
        assert_eq!(
            CommitRevision::new(u64::MAX)
                .expect("nonzero")
                .checked_next(),
            Err(CommitRevisionError::Exhausted)
        );
    }
}
