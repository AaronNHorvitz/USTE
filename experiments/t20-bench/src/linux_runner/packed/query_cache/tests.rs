use super::*;
use uste_storage::packed_page_cache::PackedPageCache;

#[test]
fn capacity_comparison_profiles_refuse_cross_size_and_cross_partition_reports() {
    assert_eq!(WIDE_TOTAL, uste_storage::MAX_INDEX_CACHE_BYTES);
    for (mode, wide) in [
        (QueryCacheMode::Pages, false),
        (QueryCacheMode::Positive, false),
        (QueryCacheMode::Range, false),
        (QueryCacheMode::Pages, true),
        (QueryCacheMode::Positive, true),
        (QueryCacheMode::Range, true),
        (QueryCacheMode::WideSmallRange, true),
    ] {
        let (total, lookup, range, name) = mode.configuration(wide);
        let cache = match mode {
            QueryCacheMode::Pages => PackedPageCache::new(total),
            QueryCacheMode::Positive => PackedPageCache::new_with_lookup_budget(total, lookup),
            QueryCacheMode::Range | QueryCacheMode::WideSmallRange => {
                PackedPageCache::new_with_lookup_and_range_budget(total, lookup, range)
            }
        }
        .unwrap();
        let r = cache.report().unwrap();
        let json = mode.report_with_size(r, wide).unwrap();
        assert_eq!(json["profile"], name);
        assert_eq!(json["total_budget_bytes"], total);
        assert_eq!(json["page_budget_bytes"], total - lookup - range);
        assert_eq!(json["lookup_budget_bytes"], lookup);
        assert_eq!(json["total_accounted_bytes"], 0);
        assert!(mode.report_with_size(r, !wide).is_err());
        for other in [
            QueryCacheMode::Pages,
            QueryCacheMode::Positive,
            QueryCacheMode::Range,
            QueryCacheMode::WideSmallRange,
        ] {
            if mode != other && mode.configuration(wide) != other.configuration(wide) {
                assert!(other.report_with_size(r, wide).is_err());
            }
        }
        for variant in 0..3 {
            let mut wrong = r;
            match variant {
                0 => wrong.budget_bytes += 1,
                1 => wrong.page_budget_bytes += 1,
                _ => wrong.accounted_bytes = total + 1,
            }
            assert!(mode.report_with_size(wrong, wide).is_err());
        }
    }
}

#[test]
fn native_range_cache_configuration_reports_its_independent_partition() {
    let mut cache = PackedPageCache::new_with_lookup_and_range_budget(TOTAL, LOOKUP, RANGE)
        .unwrap()
        .report()
        .unwrap();
    let range = cache.range.as_mut().unwrap();
    range.accounted_bytes = 8192;
    range.resident_ranges = 3;
    range.resident_entries = 7;
    range.hits = 5;
    range.misses = 4;
    range.evictions = 2;
    range.oversized_bypasses = 1;
    cache.accounted_bytes = 8192;
    let report = QueryCacheMode::Range.report(cache).unwrap();
    assert_eq!(report["profile"], "packed-pages-positive-lookups-ranges-v1");
    assert_eq!(report["page_budget_bytes"], TOTAL - LOOKUP - RANGE);
    assert_eq!(report["range_budget_bytes"], RANGE);
    assert_eq!(report["range"]["resident_ranges"], 3);
    assert_eq!(report["range"]["resident_entries"], 7);
    assert_eq!(report["range"]["hits"], 5);
    assert_eq!(report["range"]["misses"], 4);
    assert_eq!(report["range"]["evictions"], 2);
    assert_eq!(report["range"]["oversized_bypasses"], 1);
    for variant in 0..4 {
        let mut wrong = cache;
        match variant {
            0 => wrong.range = None,
            1 => wrong.range.as_mut().unwrap().budget_bytes += 1,
            2 => wrong.range.as_mut().unwrap().accounted_bytes = RANGE + 1,
            _ => wrong.accounted_bytes = TOTAL + 1,
        }
        assert!(QueryCacheMode::Range.report(wrong).is_err());
    }
}

#[test]
fn native_query_cache_configuration_is_exact_and_never_double_counts() {
    let pages = PackedPageCache::new(TOTAL).unwrap().report().unwrap();
    let plain = QueryCacheMode::Pages.report(pages).unwrap();
    assert_eq!(plain["profile"], "packed-pages-v1");
    assert!(plain["lookup"].is_null());
    assert!(QueryCacheMode::Positive.report(pages).is_err());
    let mut positive = PackedPageCache::new_with_lookup_budget(TOTAL, LOOKUP)
        .unwrap()
        .report()
        .unwrap();
    assert!(QueryCacheMode::Pages.report(positive).is_err());
    let lookup = positive.lookup.as_mut().unwrap();
    lookup.accounted_bytes = 6144;
    lookup.resident_values = 2;
    lookup.hits = 11;
    lookup.misses = 9;
    lookup.evictions = 3;
    lookup.oversized_bypasses = 4;
    positive.accounted_bytes = 25600 + 6144;
    positive.resident_pages = 1;
    let report = QueryCacheMode::Positive.report(positive).unwrap();
    assert_eq!(report["total_budget_bytes"], TOTAL);
    assert_eq!(report["page_budget_bytes"], TOTAL - LOOKUP);
    assert_eq!(report["lookup_budget_bytes"], LOOKUP);
    assert_eq!(report["total_accounted_bytes"], 31744);
    assert_eq!(report["page_accounted_bytes"], 25600);
    assert_eq!(report["lookup"]["accounted_bytes"], 6144);
    assert_eq!(report["lookup"]["hits"], 11);
    assert_eq!(report["lookup"]["misses"], 9);
    assert_eq!(report["lookup"]["evictions"], 3);
    assert_eq!(report["lookup"]["oversized_bypasses"], 4);
    assert_eq!(report["lookup"]["resident_values"], 2);
    for variant in 0..8 {
        let mut wrong = positive;
        match variant {
            0 => wrong.budget_bytes += 1,
            1 => wrong.page_budget_bytes += 1,
            2 => wrong.lookup = None,
            3 => wrong.lookup.as_mut().unwrap().budget_bytes += 1,
            4 => wrong.lookup.as_mut().unwrap().accounted_bytes = LOOKUP + 1,
            5 => wrong.accounted_bytes = TOTAL + 1,
            6 => wrong.accounted_bytes = 0,
            _ => wrong.accounted_bytes = wrong.page_budget_bytes + 6145,
        }
        assert_eq!(
            QueryCacheMode::Positive.report(wrong).unwrap_err().code(),
            "USTE_BM01_PACKED_CACHE"
        );
    }
}
