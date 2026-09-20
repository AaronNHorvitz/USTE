//! Explicit query-only comparison configuration; never an additional cache budget.
use super::*;
use uste_graph::PackedGraphReadLimits;
use uste_storage::packed_page_cache::PackedCacheReport;

const TOTAL: usize = 64 * 1024 * 1024;
const LOOKUP: usize = 16 * 1024 * 1024;
pub(super) type Reader<'a> = AuthorizedPackedReader<
    'a,
    uste_graph::GraphPackedLiveState,
    Fs,
    RecoveryEnvelope,
    OsEntropy,
    OsEntropy,
>;

#[derive(Clone, Copy)]
pub(super) enum QueryCacheMode {
    Pages,
    Positive,
}
impl QueryCacheMode {
    pub(super) fn reader<'a>(
        self,
        coordinator: &'a Packed,
        policy: &'a PolicyKernel,
        limits: PackedGraphReadLimits,
    ) -> Result<Reader<'a>, LinuxRunnerError> {
        match self {
            Self::Pages => {
                AuthorizedPackedReader::new_with_cache_budget(coordinator, policy, limits, TOTAL)
            }
            Self::Positive => AuthorizedPackedReader::new_with_lookup_cache_budget(
                coordinator,
                policy,
                limits,
                TOTAL,
                LOOKUP,
            ),
        }
        .map_err(|_| error("USTE_BM01_PACKED_AUTHORIZATION"))
    }
    pub(super) fn report(
        self,
        report: PackedCacheReport,
    ) -> Result<serde_json::Value, LinuxRunnerError> {
        let (name, lookup_budget) = match self {
            Self::Pages => ("packed-pages-v1", 0),
            Self::Positive => ("packed-pages-positive-lookups-v1", LOOKUP),
        };
        let lookup_accounted = report.lookup.map_or(0, |r| r.accounted_bytes);
        let page_accounted = report
            .accounted_bytes
            .checked_sub(lookup_accounted)
            .ok_or_else(|| error("USTE_BM01_PACKED_CACHE"))?;
        if report.budget_bytes != TOTAL
            || report.page_budget_bytes != TOTAL - lookup_budget
            || report.lookup.is_some() != (lookup_budget != 0)
            || report.lookup.is_some_and(|r| {
                r.budget_bytes != lookup_budget || r.accounted_bytes > r.budget_bytes
            })
            || report.accounted_bytes > TOTAL
            || page_accounted > report.page_budget_bytes
        {
            return Err(error("USTE_BM01_PACKED_CACHE"));
        }
        Ok(serde_json::json!({
            "profile": name, "total_budget_bytes": TOTAL,
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
        }))
    }
}

#[cfg(test)]
mod tests;
