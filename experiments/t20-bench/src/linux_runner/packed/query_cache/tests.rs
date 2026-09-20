use super::*;
use uste_storage::packed_page_cache::PackedPageCache;

#[test]
fn capacity_comparison_profiles_refuse_cross_size_and_cross_partition_reports() {
    assert_eq!(WIDE_TOTAL, uste_storage::MAX_INDEX_CACHE_BYTES);
    for wide in [false, true] {
        for mode in [QueryCacheMode::Pages, QueryCacheMode::Positive] {
            let (total, lookup, name) = mode.configuration(wide);
            let cache = if lookup == 0 {
                PackedPageCache::new(total)
            } else {
                PackedPageCache::new_with_lookup_budget(total, lookup)
            }
            .unwrap();
            let r = cache.report().unwrap();
            let json = mode.report_with_size(r, wide).unwrap();
            assert_eq!(json["profile"], name);
            assert_eq!(json["total_budget_bytes"], total);
            assert_eq!(json["page_budget_bytes"], total - lookup);
            assert_eq!(json["lookup_budget_bytes"], lookup);
            assert_eq!(json["total_accounted_bytes"], 0);
            assert!(mode.report_with_size(r, !wide).is_err());
            let other = if lookup == 0 {
                QueryCacheMode::Positive
            } else {
                QueryCacheMode::Pages
            };
            assert!(other.report_with_size(r, wide).is_err());
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
