//! Bind positive-cache mode, included residency and the complete observation ledger.
use super::*;
use serde_json::{Map, Value};
#[cfg(test)]
mod tests;
type Object = Map<String, Value>;
const TOTAL: u64 = 64 * 1024 * 1024;
const LOOKUP: u64 = 16 * 1024 * 1024;
fn budgets(wide: bool) -> (u64, u64) {
    if wide {
        (256 * 1024 * 1024, 128 * 1024 * 1024)
    } else {
        (TOTAL, LOOKUP)
    }
}
const FIELDS: [&str; 4] = ["hits", "misses", "evictions", "oversized_bypasses"];
fn fail() -> LinuxRunnerError {
    LinuxRunnerError::new("USTE_BM01_SAMPLE_PROTOCOL")
}
fn object(value: Option<&Value>) -> Result<&Object, LinuxRunnerError> {
    value.and_then(Value::as_object).ok_or_else(fail)
}
fn config(root: &Object, positive: bool, wide: bool) -> Result<&Object, LinuxRunnerError> {
    let (total_budget, lookup_budget) = budgets(wide);
    let c = object(root.get("query_cache_configuration"))?;
    expect_string(
        c,
        "profile",
        match (positive, wide) {
            (true, false) => "packed-pages-positive-lookups-v1",
            (false, false) => "packed-pages-v1",
            (true, true) => "packed-pages-positive-lookups-256m-v1",
            (false, true) => "packed-pages-256m-v1",
        },
    )?;
    expect_u64(c, "total_budget_bytes", total_budget)?;
    expect_u64(
        c,
        "lookup_budget_bytes",
        if positive { lookup_budget } else { 0 },
    )?;
    expect_u64(
        c,
        "page_budget_bytes",
        if positive {
            total_budget - lookup_budget
        } else {
            total_budget
        },
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
        || total > total_budget
        || page.checked_add(lookup) != Some(total)
    {
        return Err(fail());
    }
    Ok(c)
}
fn counters(
    value: Option<&Value>,
    labelled: bool,
    budget: u64,
) -> Result<[u64; 4], LinuxRunnerError> {
    let r = object(value)?;
    if labelled {
        expect_string(r, "measurement_scope", "positive-lookup-cache-observations")?;
        expect_bool(r, "included_in_total_cache", true)?;
        expect_bool(r, "physical_device_io", false)?;
        expect_bool(r, "logical_proof_work", false)?;
    }
    expect_u64(r, "budget_bytes", budget)?;
    let accounted = value_u64(r, "accounted_bytes")?;
    let resident = value_u64(r, "resident_values")?;
    if accounted > budget || resident > accounted || (resident == 0) != (accounted == 0) {
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
pub(super) fn validate_pages(root: &Object, wide: bool) -> Result<(), LinuxRunnerError> {
    let c = config(root, false, wide)?;
    let (total_budget, _) = budgets(wide);
    let warmup = state(root.get("warmup_lookup_cache_work"), "uste-empty")?;
    if warmup.get("work") != Some(&Value::Null) {
        return Err(fail());
    }
    let mut latest_total = None;
    for sample in root
        .get("samples")
        .and_then(Value::as_array)
        .ok_or_else(fail)?
    {
        let sample = object(Some(sample))?;
        let pair = pairs(sample)?;
        for (value, label) in pair
            .iter()
            .zip(["uste-empty", "uste-retained-after-identical-query"])
        {
            if state(Some(value), label)?.get("work") != Some(&Value::Null) {
                return Err(fail());
            }
        }
        if wide {
            let pages = sample
                .get("cache_work")
                .and_then(Value::as_array)
                .ok_or_else(fail)?;
            if pages.len() != 2 {
                return Err(fail());
            }
            for (page, label) in pages
                .iter()
                .zip(["uste-empty", "uste-retained-after-identical-query"])
            {
                let page = state(Some(page), label)?;
                expect_u64(page, "index_cache_budget_bytes", total_budget)?;
                let accounted = value_u64(page, "index_cache_accounted_bytes")?;
                if accounted > total_budget {
                    return Err(fail());
                }
                for field in [
                    "index_cache_hits",
                    "index_cache_misses",
                    "index_cache_evictions",
                ] {
                    value_u64(page, field)?;
                }
                latest_total = Some(accounted);
            }
        }
    }
    if wide && latest_total != Some(value_u64(c, "total_accounted_bytes")?) {
        return Err(fail());
    }
    Ok(())
}
pub(super) fn validate(root: &Object, wide: bool) -> Result<(), LinuxRunnerError> {
    let (total_budget, lookup_budget) = budgets(wide);
    let c = config(root, true, wide)?;
    let expected = counters(c.get("lookup"), false, lookup_budget)?;
    let mut total = counters(
        state(root.get("warmup_lookup_cache_work"), "uste-empty")?.get("work"),
        true,
        lookup_budget,
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
            let part = counters(work, true, lookup_budget)?;
            for (sum, part) in total.iter_mut().zip(part) {
                *sum = sum.checked_add(part).ok_or_else(fail)?;
            }
            let page = state(Some(page), label)?;
            expect_u64(page, "index_cache_budget_bytes", total_budget)?;
            let accounted = value_u64(page, "index_cache_accounted_bytes")?;
            let lookup_accounted = value_u64(object(work)?, "accounted_bytes")?;
            let page_accounted = accounted.checked_sub(lookup_accounted).ok_or_else(fail)?;
            if accounted > total_budget || page_accounted > total_budget - lookup_budget {
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

pub(super) fn validate_range(root: &Object, wide: bool) -> Result<(), LinuxRunnerError> {
    let (total, lookup_budget, range_budget) = if wide {
        (256 * 1024 * 1024, 64 * 1024 * 1024, 128 * 1024 * 1024)
    } else {
        (TOTAL, LOOKUP, 16 * 1024 * 1024)
    };
    let config = object(root.get("query_cache_configuration"))?;
    expect_string(
        config,
        "profile",
        if wide {
            "packed-pages-positive-lookups-ranges-256m-v1"
        } else {
            "packed-pages-positive-lookups-ranges-v1"
        },
    )?;
    for (field, value) in [
        ("total_budget_bytes", total),
        ("lookup_budget_bytes", lookup_budget),
        ("range_budget_bytes", range_budget),
        ("page_budget_bytes", total - lookup_budget - range_budget),
    ] {
        expect_u64(config, field, value)?;
    }
    let expected_lookup = counters(config.get("lookup"), false, lookup_budget)?;
    let expected_range = range_counters(config.get("range"), false, range_budget)?;
    let page_accounted = value_u64(config, "page_accounted_bytes")?;
    let lookup_accounted = value_u64(object(config.get("lookup"))?, "accounted_bytes")?;
    let range_accounted = value_u64(object(config.get("range"))?, "accounted_bytes")?;
    let total_accounted = value_u64(config, "total_accounted_bytes")?;
    if page_accounted > total - lookup_budget - range_budget
        || page_accounted
            .checked_add(lookup_accounted)
            .and_then(|value| value.checked_add(range_accounted))
            != Some(total_accounted)
        || total_accounted > total
    {
        return Err(fail());
    }
    let mut lookup_total = counters(
        state(root.get("warmup_lookup_cache_work"), "uste-empty")?.get("work"),
        true,
        lookup_budget,
    )?;
    let mut range_total = range_counters(
        state(root.get("warmup_range_cache_work"), "uste-empty")?.get("work"),
        true,
        range_budget,
    )?;
    let samples = root
        .get("samples")
        .and_then(Value::as_array)
        .ok_or_else(fail)?;
    for sample in samples {
        let sample = object(Some(sample))?;
        for (field, budget, total_counters, parser) in [
            ("lookup_cache_work", lookup_budget, &mut lookup_total, false),
            ("range_cache_work", range_budget, &mut range_total, true),
        ] {
            let pair = sample
                .get(field)
                .and_then(Value::as_array)
                .filter(|values| values.len() == 2)
                .ok_or_else(fail)?;
            for (value, label) in pair
                .iter()
                .zip(["uste-empty", "uste-retained-after-identical-query"])
            {
                let work = state(Some(value), label)?.get("work");
                let part = if parser {
                    range_counters(work, true, budget)?
                } else {
                    counters(work, true, budget)?
                };
                for (sum, value) in total_counters.iter_mut().zip(part) {
                    *sum = sum.checked_add(value).ok_or_else(fail)?;
                }
            }
        }
    }
    if lookup_total != expected_lookup || range_total != expected_range {
        return Err(fail());
    }
    Ok(())
}

fn range_counters(
    value: Option<&Value>,
    labelled: bool,
    budget: u64,
) -> Result<[u64; 4], LinuxRunnerError> {
    let report = object(value)?;
    if labelled {
        expect_string(
            report,
            "measurement_scope",
            "complete-range-cache-observations",
        )?;
        expect_bool(report, "included_in_total_cache", true)?;
        expect_bool(report, "physical_device_io", false)?;
        expect_bool(report, "logical_proof_work", false)?;
    }
    expect_u64(report, "budget_bytes", budget)?;
    let accounted = value_u64(report, "accounted_bytes")?;
    let ranges = value_u64(report, "resident_ranges")?;
    let entries = value_u64(report, "resident_entries")?;
    if accounted > budget
        || ranges > accounted
        || entries > accounted
        || (ranges == 0) != (accounted == 0)
    {
        return Err(fail());
    }
    Ok([
        value_u64(report, FIELDS[0])?,
        value_u64(report, FIELDS[1])?,
        value_u64(report, FIELDS[2])?,
        value_u64(report, FIELDS[3])?,
    ])
}
