//! Fixed, checked complete-range cache deltas; residency is included in the total cache.
use super::{CacheState, LinuxRunnerError, error};
use uste_storage::packed_page_cache::PackedRangeCacheReport;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct RangeWork {
    observed: bool,
    report: Option<PackedRangeCacheReport>,
}
impl RangeWork {
    pub(super) fn add(
        &mut self,
        before: Option<PackedRangeCacheReport>,
        after: Option<PackedRangeCacheReport>,
    ) -> Result<(), LinuxRunnerError> {
        let fail = || error("USTE_BM01_RANGE_COUNTER");
        let (before, after) = match (before, after) {
            (None, None) if self.report.is_none() => {
                self.observed = true;
                return Ok(());
            }
            (Some(before), Some(after)) if !self.observed || self.report.is_some() => {
                (before, after)
            }
            _ => return Err(fail()),
        };
        for report in [before, after] {
            if !(8192..=uste_storage::MAX_INDEX_CACHE_BYTES).contains(&report.budget_bytes)
                || report.accounted_bytes > report.budget_bytes
                || report.resident_ranges > report.accounted_bytes
                || report.resident_entries > report.accounted_bytes
                || (report.resident_ranges == 0) != (report.accounted_bytes == 0)
            {
                return Err(fail());
            }
        }
        if before.budget_bytes != after.budget_bytes
            || self
                .report
                .is_some_and(|r| r.budget_bytes != after.budget_bytes)
        {
            return Err(fail());
        }
        let prior = self.report.unwrap_or_default();
        let delta = |old: u64, new: u64, sum: u64| {
            new.checked_sub(old)
                .and_then(|value| sum.checked_add(value))
                .ok_or_else(fail)
        };
        self.report = Some(PackedRangeCacheReport {
            hits: delta(before.hits, after.hits, prior.hits)?,
            misses: delta(before.misses, after.misses, prior.misses)?,
            evictions: delta(before.evictions, after.evictions, prior.evictions)?,
            oversized_bypasses: delta(
                before.oversized_bypasses,
                after.oversized_bypasses,
                prior.oversized_bypasses,
            )?,
            ..after
        });
        self.observed = true;
        Ok(())
    }
    pub(super) fn json(self, state: CacheState) -> Result<serde_json::Value, LinuxRunnerError> {
        if !self.observed {
            return Err(error("USTE_BM01_SAMPLE_OBSERVATIONS"));
        }
        Ok(serde_json::json!({
            "cache": state.name(),
            "work": self.report.map(|r| serde_json::json!({
                "measurement_scope": "complete-range-cache-observations",
                "included_in_total_cache": true, "physical_device_io": false,
                "logical_proof_work": false,
                "budget_bytes": r.budget_bytes, "accounted_bytes": r.accounted_bytes,
                "resident_ranges": r.resident_ranges, "resident_entries": r.resident_entries,
                "hits": r.hits, "misses": r.misses, "evictions": r.evictions,
                "oversized_bypasses": r.oversized_bypasses,
            })),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report(hits: u64, misses: u64, accounted: usize) -> PackedRangeCacheReport {
        PackedRangeCacheReport {
            budget_bytes: 8192,
            accounted_bytes: accounted,
            resident_ranges: usize::from(accounted != 0),
            resident_entries: usize::from(accounted != 0) * 2,
            hits,
            misses,
            evictions: 0,
            oversized_bypasses: 0,
        }
    }
    #[test]
    fn range_work_deltas_residency_and_absence_are_exact() {
        let mut work = RangeWork::default();
        work.add(Some(report(1, 2, 512)), Some(report(4, 5, 768)))
            .unwrap();
        work.add(Some(report(4, 5, 768)), Some(report(6, 9, 768)))
            .unwrap();
        let json = work.json(CacheState::Retained).unwrap();
        assert_eq!(json["work"]["hits"], 5);
        assert_eq!(json["work"]["misses"], 7);
        assert_eq!(json["work"]["accounted_bytes"], 768);
        assert_eq!(json["work"]["resident_ranges"], 1);
        assert_eq!(json["work"]["resident_entries"], 2);
        let mut absent = RangeWork::default();
        absent.add(None, None).unwrap();
        assert!(absent.json(CacheState::Empty).unwrap()["work"].is_null());
        assert!(RangeWork::default().json(CacheState::Empty).is_err());
    }

    #[test]
    fn range_work_refuses_mixed_partitions_regression_and_invalid_residency() {
        let mut work = RangeWork::default();
        assert!(work.add(None, Some(report(0, 0, 0))).is_err());
        assert!(
            work.add(Some(report(2, 0, 0)), Some(report(1, 0, 0)))
                .is_err()
        );
        let mut invalid = report(0, 0, 0);
        invalid.resident_ranges = 1;
        assert!(work.add(Some(invalid), Some(invalid)).is_err());
    }
}
