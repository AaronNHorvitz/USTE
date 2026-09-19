//! Fixed-size cached-primitive diagnostics, distinct from adapter/device traffic.
use super::*;
use uste_storage::IndexReadTelemetry;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct IndexWork([u64; 6]);

impl From<IndexReadTelemetry> for IndexWork {
    fn from(value: IndexReadTelemetry) -> Self {
        Self([
            value.completed_operations,
            value.failed_operations,
            value.work.pages_read,
            value.work.cache_hits,
            value.work.fragments_visited,
            value.work.result_bytes,
        ])
    }
}

impl IndexWork {
    pub(super) fn delta(self, before: Self) -> Result<Self, LinuxRunnerError> {
        let mut result = Self::default();
        for (i, value) in result.0.iter_mut().enumerate() {
            *value = self.0[i]
                .checked_sub(before.0[i])
                .ok_or_else(|| error("USTE_BM01_INDEX_COUNTER"))?;
        }
        Ok(result)
    }

    pub(super) fn accumulate(&mut self, other: Self) -> Result<(), LinuxRunnerError> {
        let mut result = *self;
        for (i, value) in result.0.iter_mut().enumerate() {
            *value = value
                .checked_add(other.0[i])
                .ok_or_else(|| error("USTE_BM01_INDEX_COUNTER"))?;
        }
        *self = result;
        Ok(())
    }

    pub(super) fn json(self) -> serde_json::Value {
        serde_json::json!({
            "measurement_scope": "cached-exact-predecessor-prefix-primitives",
            "complete_authenticated_index_io": false,
            "physical_device_io": false,
            "includes_work_before_primitive_errors": true,
            // Keep the existing counter name, but disclose the sparse-search probe work.
            "fragment_work_semantics": "enumerated-fragments-plus-sparse-key-probes; excludes-page-validation",
            "completed_operations": self.0[0], "failed_operations": self.0[1],
            "authenticated_pages_loaded": self.0[2], "cache_page_hits": self.0[3],
            "enumerated_fragments": self.0[4], "primitive_result_bytes": self.0[5],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_work_checks_every_field_and_aggregates_atomically() {
        let work = IndexWork([1, 2, 3, 4, 5, 6]);
        let mut total = work;
        total.accumulate(work).unwrap();
        assert_eq!(total.delta(work).unwrap(), work);
        for field in 0..6 {
            let mut too_large = work;
            too_large.0[field] = u64::MAX;
            let before = too_large;
            assert!(too_large.accumulate(work).is_err());
            assert_eq!(too_large, before);
            assert!(work.delta(too_large).is_err());
        }
        assert_eq!(work.json()["complete_authenticated_index_io"], false);
        assert_eq!(work.json()["physical_device_io"], false);
        assert_eq!(work.json()["enumerated_fragments"], 5);
        assert_eq!(
            work.json()["fragment_work_semantics"],
            "enumerated-fragments-plus-sparse-key-probes; excludes-page-validation"
        );
    }
}
