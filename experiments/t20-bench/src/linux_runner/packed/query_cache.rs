//! Explicit query-only comparison configuration; never an additional cache budget.
use super::*;
use uste_graph::PackedGraphReadLimits;
use uste_storage::packed_page_cache::PackedCacheReport;

const TOTAL: usize = 64 * 1024 * 1024;
const LOOKUP: usize = 16 * 1024 * 1024;
const RANGE: usize = 16 * 1024 * 1024;
const WIDE_TOTAL: usize = 256 * 1024 * 1024;
const WIDE_LOOKUP: usize = 128 * 1024 * 1024;
const WIDE_RANGE_LOOKUP: usize = 64 * 1024 * 1024;
const WIDE_RANGE: usize = 128 * 1024 * 1024;
const WIDE_SMALL_RANGE_LOOKUP: usize = 128 * 1024 * 1024;
const WIDE_SMALL_RANGE: usize = 16 * 1024 * 1024;
pub(super) type Reader<'a> = AuthorizedPackedReader<
    'a,
    uste_graph::GraphPackedLiveState,
    Fs,
    RecoveryEnvelope,
    OsEntropy,
    OsEntropy,
>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum QueryCacheMode {
    Pages,
    Positive,
    Range,
    WideSmallRange,
}
impl QueryCacheMode {
    pub(super) fn includes_ranges(self) -> bool {
        matches!(self, Self::Range | Self::WideSmallRange)
    }
    pub(super) fn reader_with_size<'a>(
        self,
        coordinator: &'a Packed,
        policy: &'a PolicyKernel,
        limits: PackedGraphReadLimits,
        wide: bool,
    ) -> Result<Reader<'a>, LinuxRunnerError> {
        let (total, lookup, range, _) = self.configuration(wide);
        match self {
            Self::Pages => {
                AuthorizedPackedReader::new_with_cache_budget(coordinator, policy, limits, total)
            }
            Self::Positive => AuthorizedPackedReader::new_with_lookup_cache_budget(
                coordinator,
                policy,
                limits,
                total,
                lookup,
            ),
            Self::Range | Self::WideSmallRange => {
                AuthorizedPackedReader::new_with_lookup_and_range_cache_budget(
                    coordinator,
                    policy,
                    limits,
                    total,
                    lookup,
                    range,
                )
            }
        }
        .map_err(|_| error("USTE_BM01_PACKED_AUTHORIZATION"))
    }
    #[cfg(test)]
    pub(super) fn report(
        self,
        report: PackedCacheReport,
    ) -> Result<serde_json::Value, LinuxRunnerError> {
        self.report_with_size(report, false)
    }
    fn configuration(self, wide: bool) -> (usize, usize, usize, &'static str) {
        match (self, wide) {
            (Self::Pages, false) => (TOTAL, 0, 0, "packed-pages-v1"),
            (Self::Positive, false) => (TOTAL, LOOKUP, 0, "packed-pages-positive-lookups-v1"),
            (Self::Range, false) => (
                TOTAL,
                LOOKUP,
                RANGE,
                "packed-pages-positive-lookups-ranges-v1",
            ),
            (Self::Pages, true) => (WIDE_TOTAL, 0, 0, "packed-pages-256m-v1"),
            (Self::Positive, true) => (
                WIDE_TOTAL,
                WIDE_LOOKUP,
                0,
                "packed-pages-positive-lookups-256m-v1",
            ),
            (Self::Range, true) => (
                WIDE_TOTAL,
                WIDE_RANGE_LOOKUP,
                WIDE_RANGE,
                "packed-pages-positive-lookups-ranges-256m-v1",
            ),
            (Self::WideSmallRange, true) => (
                WIDE_TOTAL,
                WIDE_SMALL_RANGE_LOOKUP,
                WIDE_SMALL_RANGE,
                "packed-pages-positive-lookups-small-ranges-256m-v1",
            ),
            (Self::WideSmallRange, false) => (
                TOTAL,
                LOOKUP,
                RANGE,
                "packed-pages-positive-lookups-ranges-v1",
            ),
        }
    }
    pub(super) fn report_with_size(
        self,
        report: PackedCacheReport,
        wide: bool,
    ) -> Result<serde_json::Value, LinuxRunnerError> {
        let (total, lookup_budget, range_budget, name) = self.configuration(wide);
        let lookup_accounted = report.lookup.map_or(0, |r| r.accounted_bytes);
        let range_accounted = report.range.map_or(0, |r| r.accounted_bytes);
        let page_accounted = report
            .accounted_bytes
            .checked_sub(lookup_accounted)
            .and_then(|bytes| bytes.checked_sub(range_accounted))
            .ok_or_else(|| error("USTE_BM01_PACKED_CACHE"))?;
        if report.budget_bytes != total
            || report.page_budget_bytes != total - lookup_budget - range_budget
            || report.lookup.is_some() != (lookup_budget != 0)
            || report.lookup.is_some_and(|r| {
                r.budget_bytes != lookup_budget || r.accounted_bytes > r.budget_bytes
            })
            || report.range.is_some() != (range_budget != 0)
            || report.range.is_some_and(|r| {
                r.budget_bytes != range_budget || r.accounted_bytes > r.budget_bytes
            })
            || report.accounted_bytes > total
            || page_accounted > report.page_budget_bytes
        {
            return Err(error("USTE_BM01_PACKED_CACHE"));
        }
        let mut output = serde_json::json!({
            "profile": name, "total_budget_bytes": total,
            "page_budget_bytes": report.page_budget_bytes, "lookup_budget_bytes": lookup_budget,
            "total_accounted_bytes": report.accounted_bytes,
            "page_accounted_bytes": page_accounted,
            "logical_accounting_not_rss": true, "page_counter_scope": "packed-pages-only",
            "clear_drops": "all-enabled-uste-partitions",
            "lookup": report.lookup.map(|r| serde_json::json!({
                "budget_bytes": r.budget_bytes, "accounted_bytes": r.accounted_bytes,
                "resident_values": r.resident_values, "hits": r.hits, "misses": r.misses,
                "evictions": r.evictions, "oversized_bypasses": r.oversized_bypasses,
            })),
        });
        if let Some(range) = report.range {
            output["range_budget_bytes"] = range_budget.into();
            output["range"] = serde_json::json!({
                "budget_bytes": range.budget_bytes, "accounted_bytes": range.accounted_bytes,
                "resident_ranges": range.resident_ranges, "resident_entries": range.resident_entries,
                "hits": range.hits, "misses": range.misses, "evictions": range.evictions,
                "oversized_bypasses": range.oversized_bypasses,
            });
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests;
