//! Entropy capability separated from cryptographic operations for deterministic fault tests.

use core::fmt;

/// Redacted entropy-source failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntropyFailure;

impl fmt::Display for EntropyFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("entropy unavailable")
    }
}

impl std::error::Error for EntropyFailure {}

/// Narrow entropy capability. Implementations must either fill the entire slice or fail.
pub trait EntropySource {
    fn fill(&mut self, output: &mut [u8]) -> Result<(), EntropyFailure>;
}

/// Operating-system CSPRNG adapter selected by `crypto-v1`.
#[derive(Clone, Copy, Debug, Default)]
pub struct OsEntropy;

impl EntropySource for OsEntropy {
    fn fill(&mut self, output: &mut [u8]) -> Result<(), EntropyFailure> {
        getrandom::fill(output).map_err(|_| EntropyFailure)
    }
}
