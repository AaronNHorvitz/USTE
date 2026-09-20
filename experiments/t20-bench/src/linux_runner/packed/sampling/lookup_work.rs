//! Fixed, checked positive-cache deltas; residency is already included in the total cache.
use super::{CacheState, LinuxRunnerError, error};
use uste_storage::packed_page_cache::PackedLookupCacheReport;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct LookupWork {
    observed: bool,
    report: Option<PackedLookupCacheReport>,
}
impl LookupWork {
    pub(super) fn add(
        &mut self,
        before: Option<PackedLookupCacheReport>,
        after: Option<PackedLookupCacheReport>,
    ) -> Result<(), LinuxRunnerError> {
        let fail = || error("USTE_BM01_LOOKUP_COUNTER");
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
                || report.resident_values > report.accounted_bytes
                || (report.resident_values == 0) != (report.accounted_bytes == 0)
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
        let updated = PackedLookupCacheReport {
            hits: delta(before.hits, after.hits, prior.hits)?,
            misses: delta(before.misses, after.misses, prior.misses)?,
            evictions: delta(before.evictions, after.evictions, prior.evictions)?,
            oversized_bypasses: delta(
                before.oversized_bypasses,
                after.oversized_bypasses,
                prior.oversized_bypasses,
            )?,
            ..after
        };
        self.report = Some(updated);
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
                "measurement_scope": "positive-lookup-cache-observations",
                "included_in_total_cache": true, "physical_device_io": false,
                "logical_proof_work": false,
                "budget_bytes": r.budget_bytes, "accounted_bytes": r.accounted_bytes,
                "resident_values": r.resident_values, "hits": r.hits, "misses": r.misses,
                "evictions": r.evictions, "oversized_bypasses": r.oversized_bypasses,
            })),
        }))
    }
}

#[cfg(test)]
mod tests;
