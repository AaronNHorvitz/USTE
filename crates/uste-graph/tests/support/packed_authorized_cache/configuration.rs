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
