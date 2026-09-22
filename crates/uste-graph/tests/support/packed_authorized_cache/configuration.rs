use super::*;

#[test]
fn authorized_positive_configuration_is_explicit_bounded_and_privileged() {
    let (mut fs, live, kernel, admin, bob, _) = setup_cached();
    let minimum = uste_storage::MIN_INDEX_CACHE_BYTES + 8192;
    for (total, lookup) in [
        (minimum, 0),
        (minimum, 8191),
        (minimum - 1, 8192),
        (minimum, minimum + 1),
        (uste_storage::MAX_INDEX_CACHE_BYTES + 1, 8192),
        (usize::MAX, usize::MAX),
    ] {
        assert!(matches!(
            AuthorizedPackedReader::new_with_lookup_cache_budget(
                &live,
                &kernel,
                read_limits(),
                total,
                lookup,
            ),
            Err(AuthorizedError::ResourceLimit)
        ));
    }
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
    let request = GraphReadRequest::Record { id: record(4) };
    let expected = AuthorizedPackedReader::new(&live, &kernel, read_limits())
        .unwrap()
        .read(&mut fs, &admin, &request, &NeverCancel)
        .unwrap();
    for total in [minimum, CACHE_BYTES] {
        let reader = AuthorizedPackedReader::new_with_lookup_cache_budget(
            &live,
            &kernel,
            read_limits(),
            total,
            8192,
        )
        .unwrap();
        let report = reader.cache_report(&admin).unwrap().unwrap();
        assert_eq!(report.budget_bytes, total);
        assert_eq!(report.page_budget_bytes, total - 8192);
        assert_eq!(report.lookup.unwrap().budget_bytes, 8192);
        assert_eq!(report.accounted_bytes, 0);
        assert!(reader.cache_report(&bob).is_err());
        assert!(reader.clear_cache(&bob).is_err());
        assert_eq!(
            reader
                .read(&mut fs, &admin, &request, &NeverCancel)
                .unwrap(),
            expected
        );
        let crypto = live.vault_decrypt_report().unwrap();
        fs.arm(FaultPlan::default()).unwrap();
        assert_eq!(
            reader
                .read(&mut fs, &admin, &request, &NeverCancel)
                .unwrap(),
            expected
        );
        assert_eq!(live.vault_decrypt_report().unwrap(), crypto);
        assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
        let report = reader.cache_report(&admin).unwrap().unwrap();
        assert!(report.lookup.unwrap().resident_values > 0);
        assert!(report.lookup.unwrap().hits > 0);
        assert!(report.accounted_bytes <= total);
        reader.clear_cache(&admin).unwrap();
        let cleared = reader.cache_report(&admin).unwrap().unwrap();
        assert_eq!(cleared.lookup.unwrap().resident_values, 0);
        assert_eq!(cleared.accounted_bytes, 0);
        assert_eq!(cleared.lookup.unwrap().hits, report.lookup.unwrap().hits);
    }
}

#[test]
fn authorized_range_configuration_is_explicit_bounded_privileged_and_warm() {
    let (mut fs, live, kernel, admin, bob, _) = setup_cached();
    let minimum = uste_storage::MIN_INDEX_CACHE_BYTES + 8192 + 8192;
    for (total, lookup, range) in [
        (minimum, 0, 8192),
        (minimum, 8192, 0),
        (minimum - 1, 8192, 8192),
        (minimum, 8192, minimum),
        (uste_storage::MAX_INDEX_CACHE_BYTES + 1, 8192, 8192),
    ] {
        assert!(matches!(
            AuthorizedPackedReader::new_with_lookup_and_range_cache_budget(
                &live,
                &kernel,
                expansion_limits(ample()),
                total,
                lookup,
                range,
            ),
            Err(AuthorizedError::ResourceLimit)
        ));
    }
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
    let reader = AuthorizedPackedReader::new_with_lookup_and_range_cache_budget(
        &live,
        &kernel,
        expansion_limits(ample()),
        CACHE_BYTES,
        64 * 1024,
        256 * 1024,
    )
    .unwrap();
    let initial = reader.cache_report(&admin).unwrap().unwrap();
    assert_eq!(
        initial.page_budget_bytes,
        CACHE_BYTES - 64 * 1024 - 256 * 1024
    );
    assert_eq!(initial.lookup.unwrap().budget_bytes, 64 * 1024);
    assert_eq!(initial.range.unwrap().budget_bytes, 256 * 1024);
    assert!(reader.cache_report(&bob).is_err());
    let request = adjacent();
    let output = reader
        .read(&mut fs, &admin, &request, &NeverCancel)
        .unwrap();
    let cold = reader.cache_report(&admin).unwrap().unwrap();
    assert!(cold.range.unwrap().resident_ranges > 0);
    assert!(cold.range.unwrap().resident_entries > 0);
    let crypto = live.vault_decrypt_report().unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        reader
            .read(&mut fs, &admin, &request, &NeverCancel)
            .unwrap(),
        output
    );
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
    assert_eq!(live.vault_decrypt_report().unwrap(), crypto);
    let warm = reader.cache_report(&admin).unwrap().unwrap();
    assert!(warm.range.unwrap().hits > cold.range.unwrap().hits);
    assert_eq!(
        warm.range.unwrap().resident_ranges,
        cold.range.unwrap().resident_ranges
    );
    assert!(warm.accounted_bytes <= warm.budget_bytes);
    reader.clear_cache(&admin).unwrap();
    let cleared = reader.cache_report(&admin).unwrap().unwrap();
    assert_eq!(cleared.range.unwrap().resident_ranges, 0);
    assert_eq!(cleared.range.unwrap().resident_entries, 0);
    assert_eq!(cleared.accounted_bytes, 0);
    assert_eq!(cleared.range.unwrap().hits, warm.range.unwrap().hits);
}
