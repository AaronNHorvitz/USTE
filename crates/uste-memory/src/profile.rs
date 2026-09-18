//! Frozen M1 admission and measurement profile.

/// Versioned numeric contract for the bounded M1 local-memory pilot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PilotProfile {
    pub profile_name: &'static str,
    pub schema_version: u16,
    pub maximum_commits: u64,
    pub maximum_source_versions: usize,
    pub maximum_facts: usize,
    pub maximum_source_bytes: u64,
    pub maximum_source_bytes_per_version: u64,
    pub maximum_text_bytes_per_version: usize,
    pub maximum_fact_field_bytes: usize,
    pub maximum_request_bytes: usize,
    pub maximum_logical_state_bytes: usize,
    pub maximum_staged_uploads: usize,
    pub maximum_staged_upload_bytes: u64,
    pub maximum_query_terms: usize,
    pub maximum_query_candidates: usize,
    pub maximum_query_results: usize,
    pub maximum_query_output_bytes: usize,
    pub maximum_concurrent_readers: usize,
    pub maximum_rss_bytes: u64,
    pub maximum_cold_recovery_millis: u64,
    pub maximum_warm_query_micros_p99: u64,
    pub minimum_ingest_bytes_per_second: u64,
}

/// `memory-pilot-v1` is intentionally much smaller than the full USTE limits.
pub const PILOT_PROFILE: PilotProfile = PilotProfile {
    profile_name: "memory-pilot-v1",
    schema_version: 1,
    maximum_commits: 4_096,
    maximum_source_versions: 256,
    maximum_facts: 2_048,
    maximum_source_bytes: 32 * 1024 * 1024,
    maximum_source_bytes_per_version: 1024 * 1024,
    maximum_text_bytes_per_version: 64 * 1024,
    maximum_fact_field_bytes: 4 * 1024,
    maximum_request_bytes: 128 * 1024,
    maximum_logical_state_bytes: 16 * 1024 * 1024,
    maximum_staged_uploads: 8,
    maximum_staged_upload_bytes: 2 * 1024 * 1024,
    maximum_query_terms: 8,
    maximum_query_candidates: 2_048,
    maximum_query_results: 32,
    maximum_query_output_bytes: 64 * 1024,
    maximum_concurrent_readers: 8,
    maximum_rss_bytes: 512 * 1024 * 1024,
    maximum_cold_recovery_millis: 15_000,
    maximum_warm_query_micros_p99: 100_000,
    minimum_ingest_bytes_per_second: 1024 * 1024,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PilotProfileError {
    InvalidProfile,
}

impl PilotProfile {
    /// Reject an internally inconsistent or silently widened profile.
    pub const fn validate(self) -> Result<(), PilotProfileError> {
        if self.profile_name.is_empty()
            || self.schema_version != 1
            || self.maximum_commits == 0
            || self.maximum_source_versions == 0
            || self.maximum_facts == 0
            || self.maximum_source_bytes_per_version == 0
            || self.maximum_source_bytes < self.maximum_source_bytes_per_version
            || self.maximum_text_bytes_per_version == 0
            || self.maximum_text_bytes_per_version as u64 > self.maximum_source_bytes_per_version
            || self.maximum_fact_field_bytes == 0
            || self.maximum_request_bytes == 0
            || self.maximum_request_bytes > self.maximum_logical_state_bytes
            || self.maximum_staged_uploads == 0
            || self.maximum_staged_uploads > uste_storage::MAX_CONCURRENT_UPLOADS
            || self.maximum_staged_upload_bytes < self.maximum_source_bytes_per_version
            || self.maximum_query_terms == 0
            || self.maximum_query_candidates < self.maximum_query_results
            || self.maximum_query_results == 0
            || self.maximum_query_output_bytes == 0
            || self.maximum_concurrent_readers == 0
            || self.maximum_rss_bytes < 8 * self.maximum_logical_state_bytes as u64
            || self.maximum_cold_recovery_millis == 0
            || self.maximum_warm_query_micros_p99 == 0
            || self.minimum_ingest_bytes_per_second == 0
        {
            Err(PilotProfileError::InvalidProfile)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PILOT_PROFILE, PilotProfileError};

    #[test]
    fn frozen_profile_is_self_consistent() {
        assert_eq!(PILOT_PROFILE.validate(), Ok(()));
        assert_eq!(PILOT_PROFILE.profile_name, "memory-pilot-v1");
        assert_eq!(PILOT_PROFILE.maximum_rss_bytes, 512 * 1024 * 1024);
    }

    #[test]
    fn widened_or_inconsistent_profile_is_rejected() {
        let mut invalid = PILOT_PROFILE;
        invalid.maximum_staged_uploads = uste_storage::MAX_CONCURRENT_UPLOADS + 1;
        assert_eq!(invalid.validate(), Err(PilotProfileError::InvalidProfile));

        invalid = PILOT_PROFILE;
        invalid.maximum_rss_bytes = invalid.maximum_logical_state_bytes as u64;
        assert_eq!(invalid.validate(), Err(PilotProfileError::InvalidProfile));
    }
}
