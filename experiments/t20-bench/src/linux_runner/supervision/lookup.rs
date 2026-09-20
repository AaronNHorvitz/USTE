//! Bind positive-cache mode, included residency and the complete observation ledger.
use super::*;
use serde_json::{Map, Value};
#[cfg(test)]
mod tests;
type Object = Map<String, Value>;
const TOTAL: u64 = 64 * 1024 * 1024;
const LOOKUP: u64 = 16 * 1024 * 1024;
const FIELDS: [&str; 4] = ["hits", "misses", "evictions", "oversized_bypasses"];
fn fail() -> LinuxRunnerError {
    LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL")
}
fn object(value: Option<&Value>) -> Result<&Object, LinuxRunnerError> {
    value.and_then(Value::as_object).ok_or_else(fail)
}
fn config(root: &Object, positive: bool) -> Result<&Object, LinuxRunnerError> {
    let c = object(root.get("query_cache_configuration"))?;
    expect_string(
        c,
        "profile",
        if positive {
            "packed-pages-positive-lookups-v1"
        } else {
            "packed-pages-v1"
        },
    )?;
    expect_u64(c, "total_budget_bytes", TOTAL)?;
    expect_u64(c, "lookup_budget_bytes", if positive { LOOKUP } else { 0 })?;
    expect_u64(
        c,
        "page_budget_bytes",
        if positive { TOTAL - LOOKUP } else { TOTAL },
    )?;
    expect_bool(c, "logical_accounting_not_rss", true)?;
    expect_string(c, "page_counter_scope", "packed-pages-only")?;
    expect_string(c, "clear_drops", "all-enabled-uste-partitions")?;
    let page = value_u64(c, "page_accounted_bytes")?;
    let total = value_u64(c, "total_accounted_bytes")?;
    let lookup = if positive {
        value_u64(object(c.get("lookup"))?, "accounted_bytes")?
    } else {
        if c.get("lookup") != Some(&Value::Null) {
            return Err(fail());
        }
        0
    };
    if page > value_u64(c, "page_budget_bytes")?
        || total > TOTAL
        || page.checked_add(lookup) != Some(total)
    {
        return Err(fail());
    }
    Ok(c)
}
fn counters(value: Option<&Value>, labelled: bool) -> Result<[u64; 4], LinuxRunnerError> {
    let r = object(value)?;
    if labelled {
        expect_string(r, "measurement_scope", "positive-lookup-cache-observations")?;
        expect_bool(r, "included_in_total_cache", true)?;
        expect_bool(r, "physical_device_io", false)?;
        expect_bool(r, "logical_proof_work", false)?;
    }
    expect_u64(r, "budget_bytes", LOOKUP)?;
    let accounted = value_u64(r, "accounted_bytes")?;
    let resident = value_u64(r, "resident_values")?;
    if accounted > LOOKUP || resident > accounted || (resident == 0) != (accounted == 0) {
        return Err(fail());
    }
    Ok([
        value_u64(r, FIELDS[0])?,
        value_u64(r, FIELDS[1])?,
        value_u64(r, FIELDS[2])?,
        value_u64(r, FIELDS[3])?,
    ])
}
fn state<'a>(value: Option<&'a Value>, name: &str) -> Result<&'a Object, LinuxRunnerError> {
    let s = object(value)?;
    expect_string(s, "cache", name)?;
    Ok(s)
}
fn pairs(sample: &Object) -> Result<&[Value], LinuxRunnerError> {
    let values = sample
        .get("lookup_cache_work")
        .and_then(Value::as_array)
        .ok_or_else(fail)?;
    if values.len() != 2 {
        return Err(fail());
    }
    Ok(values.as_slice())
}
pub(super) fn validate_pages(root: &Object) -> Result<(), LinuxRunnerError> {
    config(root, false)?;
    let warmup = state(root.get("warmup_lookup_cache_work"), "uste-empty")?;
    if warmup.get("work") != Some(&Value::Null) {
        return Err(fail());
    }
    for sample in root
        .get("samples")
        .and_then(Value::as_array)
        .ok_or_else(fail)?
    {
        let pair = pairs(object(Some(sample))?)?;
        for (value, label) in pair
            .iter()
            .zip(["uste-empty", "uste-retained-after-identical-query"])
        {
            if state(Some(value), label)?.get("work") != Some(&Value::Null) {
                return Err(fail());
            }
        }
    }
    Ok(())
}
pub(super) fn validate(root: &Object) -> Result<(), LinuxRunnerError> {
    let c = config(root, true)?;
    let expected = counters(c.get("lookup"), false)?;
    let mut total = counters(
        state(root.get("warmup_lookup_cache_work"), "uste-empty")?.get("work"),
        true,
    )?;
    let mut latest = None;
    let mut latest_total = None;
    for sample in root
        .get("samples")
        .and_then(Value::as_array)
        .ok_or_else(fail)?
    {
        let sample = object(Some(sample))?;
        let pair = pairs(sample)?;
        let pages = sample
            .get("cache_work")
            .and_then(Value::as_array)
            .ok_or_else(fail)?;
        if pages.len() != 2 {
            return Err(fail());
        }
        for ((value, page), label) in pair
            .iter()
            .zip(pages)
            .zip(["uste-empty", "uste-retained-after-identical-query"])
        {
            let work = state(Some(value), label)?.get("work");
            let part = counters(work, true)?;
            for (sum, part) in total.iter_mut().zip(part) {
                *sum = sum.checked_add(part).ok_or_else(fail)?;
            }
            let page = state(Some(page), label)?;
            expect_u64(page, "index_cache_budget_bytes", TOTAL)?;
            let accounted = value_u64(page, "index_cache_accounted_bytes")?;
            let lookup_accounted = value_u64(object(work)?, "accounted_bytes")?;
            let page_accounted = accounted.checked_sub(lookup_accounted).ok_or_else(fail)?;
            if accounted > TOTAL || page_accounted > TOTAL - LOOKUP {
                return Err(fail());
            }
            for field in [
                "index_cache_hits",
                "index_cache_misses",
                "index_cache_evictions",
            ] {
                value_u64(page, field)?;
            }
            latest = work;
            latest_total = Some(accounted);
        }
    }
    if total != expected || latest_total != Some(value_u64(c, "total_accounted_bytes")?) {
        return Err(fail());
    }
    let last = object(latest)?;
    let final_lookup = object(c.get("lookup"))?;
    for field in ["accounted_bytes", "resident_values"] {
        if value_u64(last, field)? != value_u64(final_lookup, field)? {
            return Err(fail());
        }
    }
    Ok(())
}
