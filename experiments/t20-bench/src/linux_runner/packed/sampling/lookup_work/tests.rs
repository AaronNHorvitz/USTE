use super::*;

fn report() -> PackedLookupCacheReport {
    PackedLookupCacheReport {
        budget_bytes: 16 * 1024 * 1024,
        ..Default::default()
    }
}
fn counter(report: &mut PackedLookupCacheReport, field: usize) -> &mut u64 {
    match field {
        0 => &mut report.hits,
        1 => &mut report.misses,
        2 => &mut report.evictions,
        _ => &mut report.oversized_bypasses,
    }
}

#[test]
fn positive_sampling_work_matches_independent_deltas_and_latest_residency() {
    let mut work = LookupWork::default();
    let mut current = report();
    let mut expected = [0; 4];
    for round in 0..1000_u64 {
        let before = current;
        for (field, expected) in expected.iter_mut().enumerate() {
            let increase = (round + field as u64) % 13;
            *counter(&mut current, field) += increase;
            *expected += increase;
        }
        // Cache clearing resets gauges, never monotone observations.
        current.resident_values = (round % 4) as usize;
        current.accounted_bytes = if current.resident_values == 0 {
            0
        } else {
            4096 + 1024 * current.resident_values
        };
        work.add(Some(before), Some(current)).unwrap();
        let actual = work.report.as_mut().unwrap();
        for (field, expected) in expected.iter().enumerate() {
            assert_eq!(*counter(actual, field), *expected);
        }
        assert_eq!(actual.resident_values, current.resident_values);
        assert_eq!(actual.accounted_bytes, current.accounted_bytes);
    }
    let json = work.json(CacheState::Empty).unwrap();
    assert_eq!(json["work"]["hits"], expected[0]);
    assert_eq!(json["work"]["included_in_total_cache"], true);
    assert_eq!(json["work"]["physical_device_io"], false);
    assert_eq!(json["work"]["logical_proof_work"], false);
}

#[test]
fn positive_sampling_work_all_counter_failures_are_atomic() {
    for field in 0..4 {
        let mut before = report();
        let mut after = report();
        *counter(&mut before, field) = 1;
        let mut work = LookupWork::default();
        let initial = work;
        assert!(work.add(Some(before), Some(after)).is_err());
        assert_eq!(work, initial);
        *counter(&mut before, field) = 0;
        *counter(&mut after, field) = u64::MAX;
        work.add(Some(before), Some(after)).unwrap();
        let full = work;
        *counter(&mut after, field) = 1;
        assert!(work.add(Some(before), Some(after)).is_err());
        assert_eq!(work, full);
    }
}

#[test]
fn positive_sampling_work_refuses_mode_budget_and_gauge_substitution() {
    let mut work = LookupWork::default();
    assert!(work.json(CacheState::Empty).is_err());
    work.add(None, None).unwrap();
    assert!(work.json(CacheState::Empty).unwrap()["work"].is_null());
    let disabled = work;
    assert!(work.add(Some(report()), Some(report())).is_err());
    assert_eq!(work, disabled);
    let mut work = LookupWork::default();
    work.add(Some(report()), Some(report())).unwrap();
    let enabled = work;
    for pair in [(None, None), (None, Some(report())), (Some(report()), None)] {
        assert!(work.add(pair.0, pair.1).is_err());
        assert_eq!(work, enabled);
    }
    for variant in 0..6 {
        let mut wrong = report();
        match variant {
            0 => wrong.budget_bytes = 0,
            1 => wrong.budget_bytes += 1,
            2 => wrong.accounted_bytes = wrong.budget_bytes + 1,
            3 => wrong.resident_values = 1,
            4 => wrong.accounted_bytes = 1,
            _ => {
                wrong.accounted_bytes = 1;
                wrong.resident_values = 2;
            }
        }
        assert!(work.add(Some(report()), Some(wrong)).is_err());
        assert_eq!(work, enabled);
    }
}
