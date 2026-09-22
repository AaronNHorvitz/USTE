use super::*;

fn report() -> Value {
    let mut r = crate::linux_runner::supervision::tests::packed_report();
    r["schema"] = "bm01-linux-packed-lookup-sampling-v1".into();
    let raw = |hits, misses, evictions, bypasses| {
        serde_json::json!({
            "budget_bytes": LOOKUP, "accounted_bytes": 6144, "resident_values": 2,
            "hits": hits, "misses": misses, "evictions": evictions, "oversized_bypasses": bypasses,
        })
    };
    let work = |hits, misses, evictions, bypasses| {
        let mut value = raw(hits, misses, evictions, bypasses);
        value["measurement_scope"] = "positive-lookup-cache-observations".into();
        value["included_in_total_cache"] = true.into();
        value["physical_device_io"] = false.into();
        value["logical_proof_work"] = false.into();
        value
    };
    r["query_cache_configuration"] = serde_json::json!({
        "profile": "packed-pages-positive-lookups-v1", "total_budget_bytes": TOTAL,
        "page_budget_bytes": TOTAL - LOOKUP, "lookup_budget_bytes": LOOKUP,
        "total_accounted_bytes": 31744, "page_accounted_bytes": 25600,
        "logical_accounting_not_rss": true, "page_counter_scope": "packed-pages-only",
        "clear_drops": "all-enabled-uste-partitions", "lookup": raw(9, 8, 1, 2),
    });
    r["warmup_lookup_cache_work"] =
        serde_json::json!({"cache": "uste-empty", "work": work(1, 2, 0, 0)});
    r["samples"][0]["lookup_cache_work"] = serde_json::json!([
        {"cache": "uste-empty", "work": work(3, 6, 1, 2)},
        {"cache": "uste-retained-after-identical-query", "work": work(5, 0, 0, 0)},
    ]);
    r["samples"][0]["cache_work"] = serde_json::json!(
        ["uste-empty", "uste-retained-after-identical-query"].map(|label| serde_json::json!({
            "cache": label, "index_cache_budget_bytes": TOTAL, "index_cache_accounted_bytes": 31744,
            "index_cache_hits": 1, "index_cache_misses": 2, "index_cache_evictions": 3,
        }))
    );
    r
}
fn finish(r: &Value, mode: SampleMode) -> Result<String, LinuxRunnerError> {
    finalize_report(&r.to_string(), Bm01Profile::new(20).unwrap(), 864, mode)
}

#[test]
fn range_supervisor_requires_partition_conservation_and_complete_deltas() {
    let mut r = report();
    let range_budget = 16 * 1024 * 1024;
    let raw = |hits, misses| {
        serde_json::json!({
            "budget_bytes": range_budget, "accounted_bytes": 8192,
            "resident_ranges": 2, "resident_entries": 5,
            "hits": hits, "misses": misses, "evictions": 0, "oversized_bypasses": 0,
        })
    };
    let work = |hits, misses| {
        let mut value = raw(hits, misses);
        value["measurement_scope"] = "complete-range-cache-observations".into();
        value["included_in_total_cache"] = true.into();
        value["physical_device_io"] = false.into();
        value["logical_proof_work"] = false.into();
        value
    };
    r["schema"] = "bm01-linux-packed-range-sampling-v1".into();
    r["query_cache_configuration"]["profile"] = "packed-pages-positive-lookups-ranges-v1".into();
    r["query_cache_configuration"]["page_budget_bytes"] = (TOTAL - LOOKUP - range_budget).into();
    r["query_cache_configuration"]["range_budget_bytes"] = range_budget.into();
    r["query_cache_configuration"]["range"] = raw(7, 4);
    r["query_cache_configuration"]["total_accounted_bytes"] = 39936.into();
    r["warmup_range_cache_work"] = serde_json::json!({"cache": "uste-empty", "work": work(1, 1)});
    r["samples"][0]["range_cache_work"] = serde_json::json!([
        {"cache": "uste-empty", "work": work(2, 3)},
        {"cache": "uste-retained-after-identical-query", "work": work(4, 0)},
    ]);
    for value in r["samples"][0]["cache_work"].as_array_mut().unwrap() {
        value["index_cache_accounted_bytes"] = 39936.into();
    }
    assert!(finish(&r, SampleMode::PackedRange).is_ok());
    let mut wrong = r.clone();
    wrong["samples"][0]["range_cache_work"][1]["work"]["hits"] = 3.into();
    assert!(finish(&wrong, SampleMode::PackedRange).is_err());
    let mut wrong = r;
    wrong["query_cache_configuration"]["range"]["accounted_bytes"] = (range_budget + 1).into();
    assert!(finish(&wrong, SampleMode::PackedRange).is_err());
}

#[test]
fn small_range_supervisor_binds_distinct_wide_partition() {
    let mut r = report();
    let total = 256 * 1024 * 1024_u64;
    let lookup = 128 * 1024 * 1024_u64;
    let range = 16 * 1024 * 1024_u64;
    let raw_range = serde_json::json!({
        "budget_bytes": range, "accounted_bytes": 8192,
        "resident_ranges": 2, "resident_entries": 5,
        "hits": 7, "misses": 4, "evictions": 0, "oversized_bypasses": 0,
    });
    let range_work = |hits, misses| {
        serde_json::json!({
            "budget_bytes": range, "accounted_bytes": 8192,
            "resident_ranges": 2, "resident_entries": 5,
            "hits": hits, "misses": misses, "evictions": 0, "oversized_bypasses": 0,
            "measurement_scope": "complete-range-cache-observations",
            "included_in_total_cache": true, "physical_device_io": false,
            "logical_proof_work": false,
        })
    };
    let lookup_raw = |hits, misses, evictions, bypasses| {
        serde_json::json!({
            "budget_bytes": lookup, "accounted_bytes": 6144, "resident_values": 2,
            "hits": hits, "misses": misses, "evictions": evictions,
            "oversized_bypasses": bypasses,
        })
    };
    let lookup_work = |hits, misses, evictions, bypasses| {
        serde_json::json!({
            "budget_bytes": lookup, "accounted_bytes": 6144, "resident_values": 2,
            "hits": hits, "misses": misses, "evictions": evictions,
            "oversized_bypasses": bypasses,
            "measurement_scope": "positive-lookup-cache-observations",
            "included_in_total_cache": true, "physical_device_io": false,
            "logical_proof_work": false,
        })
    };
    r["schema"] = "bm01-linux-packed-wide-small-range-sampling-v1".into();
    r["query_cache_configuration"] = serde_json::json!({
        "profile": "packed-pages-positive-lookups-small-ranges-256m-v1",
        "total_budget_bytes": total, "page_budget_bytes": total - lookup - range,
        "lookup_budget_bytes": lookup, "range_budget_bytes": range,
        "total_accounted_bytes": 39936, "page_accounted_bytes": 25600,
        "logical_accounting_not_rss": true, "page_counter_scope": "packed-pages-only",
        "clear_drops": "all-enabled-uste-partitions",
        "lookup": lookup_raw(9, 8, 1, 2), "range": raw_range,
    });
    r["warmup_lookup_cache_work"] = serde_json::json!({
        "cache": "uste-empty", "work": lookup_work(1, 2, 0, 0)
    });
    r["warmup_range_cache_work"] = serde_json::json!({
        "cache": "uste-empty", "work": range_work(1, 1)
    });
    r["samples"][0]["lookup_cache_work"] = serde_json::json!([
        {"cache": "uste-empty", "work": lookup_work(3, 6, 1, 2)},
        {"cache": "uste-retained-after-identical-query", "work": lookup_work(5, 0, 0, 0)},
    ]);
    r["samples"][0]["range_cache_work"] = serde_json::json!([
        {"cache": "uste-empty", "work": range_work(2, 3)},
        {"cache": "uste-retained-after-identical-query", "work": range_work(4, 0)},
    ]);
    for value in r["samples"][0]["cache_work"].as_array_mut().unwrap() {
        value["index_cache_budget_bytes"] = total.into();
        value["index_cache_accounted_bytes"] = 39936.into();
    }
    assert!(finish(&r, SampleMode::PackedWideSmallRange).is_ok());
    assert!(finish(&r, SampleMode::PackedWideRange).is_err());
    let mut wrong = r;
    wrong["query_cache_configuration"]["page_budget_bytes"] = (128 * 1024 * 1024_u64).into();
    assert!(finish(&wrong, SampleMode::PackedWideSmallRange).is_err());
}

#[test]
fn positive_supervisor_requires_exact_configuration_and_complete_observation_ledger() {
    let r = report();
    let final_report: Value =
        serde_json::from_str(&finish(&r, SampleMode::PackedLookup).unwrap()).unwrap();
    assert_eq!(final_report["query_deadline_enforced"], true);
    assert_eq!(final_report["complete_authenticated_io"], false);
    for mode in [SampleMode::Legacy, SampleMode::Disk, SampleMode::Packed] {
        assert!(finish(&r, mode).is_err());
    }
    for pointer in [
        "/query_cache_configuration",
        "/query_cache_configuration/profile",
        "/query_cache_configuration/total_budget_bytes",
        "/query_cache_configuration/page_budget_bytes",
        "/query_cache_configuration/lookup_budget_bytes",
        "/query_cache_configuration/total_accounted_bytes",
        "/query_cache_configuration/page_accounted_bytes",
        "/query_cache_configuration/logical_accounting_not_rss",
        "/query_cache_configuration/page_counter_scope",
        "/query_cache_configuration/clear_drops",
        "/query_cache_configuration/lookup",
        "/query_cache_configuration/lookup/hits",
        "/query_cache_configuration/lookup/resident_values",
        "/warmup_lookup_cache_work",
        "/warmup_lookup_cache_work/cache",
        "/warmup_lookup_cache_work/work",
        "/samples/0/lookup_cache_work",
        "/samples/0/lookup_cache_work/0/cache",
        "/samples/0/lookup_cache_work/1/work",
        "/samples/0/lookup_cache_work/1/work/measurement_scope",
        "/samples/0/lookup_cache_work/1/work/included_in_total_cache",
        "/samples/0/lookup_cache_work/1/work/physical_device_io",
        "/samples/0/lookup_cache_work/1/work/logical_proof_work",
        "/samples/0/lookup_cache_work/1/work/oversized_bypasses",
        "/samples/0/cache_work",
        "/samples/0/cache_work/0/cache",
        "/samples/0/cache_work/1/index_cache_accounted_bytes",
        "/samples/0/cache_work/1/index_cache_misses",
    ] {
        let mut wrong = r.clone();
        *wrong.pointer_mut(pointer).unwrap() = Value::Null;
        assert!(
            finish(&wrong, SampleMode::PackedLookup).is_err(),
            "{pointer}"
        );
    }
}

#[test]
fn positive_supervisor_refuses_overflow_mismatched_gauges_and_fabricated_claims() {
    let r = report();
    for (pointer, value) in [
        (
            "/query_cache_configuration/lookup/hits",
            serde_json::json!(10),
        ),
        (
            "/query_cache_configuration/lookup/accounted_bytes",
            serde_json::json!(0),
        ),
        (
            "/samples/0/lookup_cache_work/1/work/resident_values",
            serde_json::json!(1),
        ),
        (
            "/samples/0/lookup_cache_work/1/work/accounted_bytes",
            serde_json::json!(LOOKUP + 1),
        ),
        (
            "/samples/0/cache_work/1/index_cache_accounted_bytes",
            serde_json::json!(TOTAL + 1),
        ),
        (
            "/warmup_lookup_cache_work/work/hits",
            serde_json::json!(u64::MAX),
        ),
        (
            "/samples/0/lookup_cache_work/1/work/physical_device_io",
            true.into(),
        ),
        (
            "/samples/0/lookup_cache_work/1/work/logical_proof_work",
            true.into(),
        ),
        (
            "/samples/0/lookup_cache_work/1/work/included_in_total_cache",
            false.into(),
        ),
        ("/query_deadline_enforced", true.into()),
        ("/complete_authenticated_io", true.into()),
        ("/budget_evaluation", "passed".into()),
    ] {
        let mut wrong = r.clone();
        *wrong.pointer_mut(pointer).unwrap() = value;
        assert!(
            finish(&wrong, SampleMode::PackedLookup).is_err(),
            "{pointer}"
        );
    }
}

#[test]
fn page_only_supervisor_accepts_legacy_reports_but_refuses_positive_relabelling() {
    let legacy = crate::linux_runner::supervision::tests::packed_report();
    assert!(finish(&legacy, SampleMode::Packed).is_ok());
    let mut r = report();
    r["schema"] = "bm01-linux-packed-sampling-v1".into();
    assert!(finish(&r, SampleMode::Packed).is_err());
    let c = &mut r["query_cache_configuration"];
    c["profile"] = "packed-pages-v1".into();
    c["page_budget_bytes"] = TOTAL.into();
    c["lookup_budget_bytes"] = 0.into();
    c["lookup"] = Value::Null;
    c["page_accounted_bytes"] = 31744.into();
    r["warmup_lookup_cache_work"]["work"] = Value::Null;
    for i in 0..2 {
        r["samples"][0]["lookup_cache_work"][i]["work"] = Value::Null;
    }
    assert!(finish(&r, SampleMode::Packed).is_ok());
    assert!(finish(&r, SampleMode::PackedLookup).is_err());
}

fn wide_report(positive: bool) -> Value {
    let mut r = report();
    let (total, lookup) = budgets(true);
    r["schema"] = if positive {
        "bm01-linux-packed-wide-lookup-sampling-v1"
    } else {
        "bm01-linux-packed-wide-sampling-v1"
    }
    .into();
    let c = &mut r["query_cache_configuration"];
    c["profile"] = if positive {
        "packed-pages-positive-lookups-256m-v1"
    } else {
        "packed-pages-256m-v1"
    }
    .into();
    c["total_budget_bytes"] = total.into();
    c["page_budget_bytes"] = (if positive { total - lookup } else { total }).into();
    c["lookup_budget_bytes"] = (if positive { lookup } else { 0 }).into();
    if positive {
        c["lookup"]["budget_bytes"] = lookup.into();
    } else {
        c["lookup"] = Value::Null;
        c["page_accounted_bytes"] = 31744.into();
    }
    for pointer in [
        "/warmup_lookup_cache_work/work",
        "/samples/0/lookup_cache_work/0/work",
        "/samples/0/lookup_cache_work/1/work",
    ] {
        let value = r.pointer_mut(pointer).unwrap();
        if positive {
            value["budget_bytes"] = lookup.into();
        } else {
            *value = Value::Null;
        }
    }
    for page in r["samples"][0]["cache_work"].as_array_mut().unwrap() {
        page["index_cache_budget_bytes"] = total.into();
    }
    r
}

#[test]
fn wide_supervisor_binds_both_capacities_and_refuses_schema_relabelling() {
    for (positive, mode) in [
        (false, SampleMode::PackedWide),
        (true, SampleMode::PackedWideLookup),
    ] {
        let r = wide_report(positive);
        let finalized: Value = serde_json::from_str(&finish(&r, mode).unwrap()).unwrap();
        assert_eq!(finalized["query_deadline_enforced"], true);
        assert_eq!(finalized["budget_evaluation"], "not-performed");
        for other in [
            SampleMode::Legacy,
            SampleMode::Disk,
            SampleMode::Packed,
            SampleMode::PackedLookup,
        ] {
            assert!(finish(&r, other).is_err());
        }
        assert!(
            finish(
                &r,
                if positive {
                    SampleMode::PackedWide
                } else {
                    SampleMode::PackedWideLookup
                }
            )
            .is_err()
        );
        let mut downgraded = r.clone();
        downgraded["schema"] = if positive {
            "bm01-linux-packed-lookup-sampling-v1"
        } else {
            "bm01-linux-packed-sampling-v1"
        }
        .into();
        assert!(
            finish(
                &downgraded,
                if positive {
                    SampleMode::PackedLookup
                } else {
                    SampleMode::Packed
                }
            )
            .is_err()
        );
        for pointer in [
            "/query_cache_configuration",
            "/warmup_lookup_cache_work",
            "/samples/0/lookup_cache_work",
            "/samples/0/cache_work",
            "/samples/0/cache_work/0/cache",
            "/samples/0/cache_work/1/index_cache_budget_bytes",
            "/samples/0/cache_work/1/index_cache_accounted_bytes",
            "/samples/0/cache_work/1/index_cache_evictions",
        ] {
            let mut wrong = r.clone();
            *wrong.pointer_mut(pointer).unwrap() = Value::Null;
            assert!(finish(&wrong, mode).is_err(), "{pointer}");
        }
        let mut wrong = r.clone();
        wrong["samples"][0]["cache_work"][0]["index_cache_budget_bytes"] = TOTAL.into();
        assert!(finish(&wrong, mode).is_err());
        let mut wrong = r.clone();
        wrong["query_cache_configuration"]["total_accounted_bytes"] = 31743.into();
        wrong["query_cache_configuration"]["page_accounted_bytes"] =
            (if positive { 25599 } else { 31743 }).into();
        assert!(finish(&wrong, mode).is_err());
    }
}

#[test]
fn wide_lookup_ledger_refuses_counter_overflow_old_budget_and_terminal_gauge_substitution() {
    let r = wide_report(true);
    for (pointer, value) in [
        (
            "/warmup_lookup_cache_work/work/hits",
            serde_json::json!(u64::MAX),
        ),
        (
            "/samples/0/lookup_cache_work/0/work/budget_bytes",
            serde_json::json!(LOOKUP),
        ),
        (
            "/samples/0/lookup_cache_work/1/work/resident_values",
            serde_json::json!(1),
        ),
        (
            "/samples/0/cache_work/1/index_cache_accounted_bytes",
            serde_json::json!(budgets(true).0 + 1),
        ),
    ] {
        let mut wrong = r.clone();
        *wrong.pointer_mut(pointer).unwrap() = value;
        assert!(
            finish(&wrong, SampleMode::PackedWideLookup).is_err(),
            "{pointer}"
        );
    }
}
