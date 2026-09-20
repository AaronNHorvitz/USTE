use super::*;

const CACHE_BYTES: usize = 4 * 1024 * 1024;

fn setup_cached() -> (
    Fs,
    Live,
    PolicyKernel,
    AuthenticatedPrincipal,
    AuthenticatedPrincipal,
    GraphSnapshot,
) {
    let (mut fs, mut live, mut kernel, admin, bob, snapshot) = setup_expansion();
    let quota = QuotaLimits::new(1024 * 1024, 1024 * 1024, 1024 * 1024, 8, 1024).unwrap();
    let mut policy = NamespacePolicy::new(scope(), PolicyVersion::new(4).unwrap(), quota);
    for who in [1, 2] {
        let actions = if who == 1 {
            vec![
                Action::Commit,
                Action::ReadRecord,
                Action::ReadHistory,
                Action::ReadOwnOutcome,
                Action::ManagePolicy,
                Action::ExpandGraph,
                Action::ManageSchema,
            ]
        } else {
            vec![
                Action::Commit,
                Action::ReadRecord,
                Action::ReadHistory,
                Action::ReadOwnOutcome,
                Action::ExpandGraph,
            ]
        };
        let mut grant = NamespaceGrant::new(PermissionSet::from_actions(actions), quota);
        if who == 2 {
            grant
                .deny_record(
                    record(2).record(),
                    PermissionSet::from_actions([Action::ReadRecord]),
                )
                .unwrap();
        }
        policy
            .grant(PrincipalDigest::from_bytes([who; 32]), grant)
            .unwrap();
    }
    let bytes = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        vec![],
        DurablePolicyMutation::Replace {
            expected: PolicyVersion::new(3).unwrap(),
            policy,
        },
    ))
    .unwrap();
    AuthorizedPackedWriter::new(
        &mut live,
        &mut kernel,
        preparation(4 * 1024 * 1024),
        publication(10000),
    )
    .unwrap()
    .commit(
        &mut fs,
        &admin,
        request(8, &bytes),
        &mut clock(8),
        &NeverCancel,
    )
    .unwrap();
    live.rebase_metadata(&mut fs, rebase_limits()).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    (fs, live, kernel, admin, bob, snapshot)
}

fn queries() -> Vec<GraphReadRequest> {
    vec![
        GraphReadRequest::Record { id: record(1) },
        GraphReadRequest::Record { id: record(4) },
        GraphReadRequest::Record { id: record(99) },
        GraphReadRequest::RecordAt {
            id: record(1),
            revision: CommitRevision::FIRST,
        },
        GraphReadRequest::RecordAt {
            id: record(4),
            revision: CommitRevision::new(3).unwrap(),
        },
        adjacent(),
        GraphReadRequest::SupportedBy {
            evidence: record(3),
            maximum: 100,
        },
    ]
}

#[test]
fn authorized_packed_vault_work_is_exact_privileged_and_not_cache_proof_work() {
    let (mut fs, live, kernel, admin, bob, _) = setup_cached();
    let reader = AuthorizedPackedReader::new_with_cache_budget(
        &live,
        &kernel,
        expansion_limits(ample()),
        CACHE_BYTES,
    )
    .unwrap();
    let before = live.vault_decrypt_report().unwrap();
    assert!(before.successful_calls > 0); // This owner's bootstrap/admission work is retained.
    let request = adjacent();
    let output = reader
        .read(&mut fs, &admin, &request, &NeverCancel)
        .unwrap();
    let cold = live.vault_decrypt_report().unwrap();
    let cache = reader.cache_report(&admin).unwrap().unwrap();
    assert!(cache.misses > 0);
    assert_eq!(
        cold.successful_calls - before.successful_calls,
        cache.misses
    );
    assert_eq!(
        cold.authenticated_encoded_bytes - before.authenticated_encoded_bytes,
        cache.misses * 20545
    );
    assert_eq!(
        cold.returned_plaintext_bytes - before.returned_plaintext_bytes,
        cache.misses * 16384
    );
    assert_eq!(cold.failed_calls, before.failed_calls);
    assert_eq!(
        reader
            .read(&mut fs, &admin, &request, &NeverCancel)
            .unwrap(),
        output
    );
    assert_eq!(live.vault_decrypt_report().unwrap(), cold);
    let foreign = PolicyKernel::new().authenticate(&mut Identity, &1).unwrap();
    let absent = kernel.authenticate(&mut Identity, &9).unwrap();
    for principal in [&foreign, &absent] {
        assert!(matches!(
            reader.read(&mut fs, principal, &request, &NeverCancel),
            Err(AuthorizedReadError::Authorization(
                AuthorizedError::Unauthorized
            ))
        ));
    }
    assert!(
        reader
            .read(
                &mut fs,
                &bob,
                &GraphReadRequest::Record { id: record(2) },
                &NeverCancel
            )
            .is_err()
    );
    reader.clear_cache(&admin).unwrap();
    assert_eq!(live.vault_decrypt_report().unwrap(), cold);
    let uncached = AuthorizedPackedReader::new(&live, &kernel, expansion_limits(ample())).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        uncached
            .read(&mut fs, &admin, &request, &NeverCancel)
            .unwrap(),
        output
    );
    let after = live.vault_decrypt_report().unwrap();
    assert_eq!(
        after.successful_calls - cold.successful_calls,
        fs.operation_count(FsOp::ReadAt)
    );
    assert!(after.successful_calls > cold.successful_calls);
    assert_eq!(live.vault_decrypt_report().unwrap(), after); // Shared owner, not per-reader.
}

#[test]
fn authorized_packed_revocation_denies_reads_before_vault_work() {
    let (mut fs, mut live, mut kernel, admin, _, _) = setup_cached();
    let quota = QuotaLimits::new(1024 * 1024, 1024 * 1024, 1024 * 1024, 8, 1024).unwrap();
    let mut policy = NamespacePolicy::new(scope(), PolicyVersion::new(5).unwrap(), quota);
    policy
        .grant(
            PrincipalDigest::from_bytes([1; 32]),
            NamespaceGrant::new(
                PermissionSet::from_actions([Action::Commit, Action::ManagePolicy]),
                quota,
            ),
        )
        .unwrap();
    let bytes = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        vec![],
        DurablePolicyMutation::Replace {
            expected: PolicyVersion::new(4).unwrap(),
            policy,
        },
    ))
    .unwrap();
    AuthorizedPackedWriter::new(
        &mut live,
        &mut kernel,
        preparation(4 * 1024 * 1024),
        publication(10000),
    )
    .unwrap()
    .commit(
        &mut fs,
        &admin,
        request(9, &bytes),
        &mut clock(9),
        &NeverCancel,
    )
    .unwrap();
    live.rebase_metadata(&mut fs, rebase_limits()).unwrap();
    let before = live.vault_decrypt_report().unwrap();
    let reader = AuthorizedPackedReader::new(&live, &kernel, read_limits()).unwrap();
    assert!(matches!(
        reader.read(
            &mut fs,
            &admin,
            &GraphReadRequest::Record { id: record(1) },
            &NeverCancel
        ),
        Err(AuthorizedReadError::Authorization(
            AuthorizedError::Unauthorized
        ))
    ));
    assert_eq!(live.vault_decrypt_report().unwrap(), before);
}

#[test]
fn authorized_packed_cached_point_history_work_limits_do_not_depend_on_warmth() {
    let (mut fs, live, kernel, admin, _, _) = setup_cached();
    for historical in [false, true] {
        let request = if historical {
            GraphReadRequest::RecordAt {
                id: record(4),
                revision: CommitRevision::new(2).unwrap(),
            }
        } else {
            GraphReadRequest::Record { id: record(4) }
        };
        fs.arm(FaultPlan::default()).unwrap();
        let expected = AuthorizedPackedReader::new(&live, &kernel, read_limits())
            .unwrap()
            .read(&mut fs, &admin, &request, &NeverCancel)
            .unwrap();
        let GraphReadOutput::Record(Some(value)) = &expected else {
            panic!("present record")
        };
        let bytes = encode_stored_record(value).unwrap().len() as u64;
        let pages = fs.operation_count(FsOp::ReadAt);
        let mut exact = read_limits();
        if historical {
            exact.historical.maximum_pages = pages;
            exact.historical.maximum_encoded_bytes = pages * 20545;
            exact.historical.maximum_returned_bytes = bytes + 24;
            exact.historical.maximum_candidates = 2;
        } else {
            exact.current.maximum_pages = pages;
            exact.current.maximum_encoded_bytes = pages * 20545;
            exact.current.maximum_value_bytes = bytes;
        }
        let reader =
            AuthorizedPackedReader::new_with_cache_budget(&live, &kernel, exact, CACHE_BYTES)
                .unwrap();
        for _ in 0..2 {
            assert_eq!(
                reader
                    .read(&mut fs, &admin, &request, &NeverCancel)
                    .unwrap(),
                expected
            );
        }
        for dimension in 0..3 {
            let mut narrow = exact;
            if historical {
                match dimension {
                    0 => narrow.historical.maximum_pages -= 1,
                    1 => narrow.historical.maximum_encoded_bytes -= 1,
                    _ => narrow.historical.maximum_returned_bytes -= 1,
                }
            } else {
                match dimension {
                    0 => narrow.current.maximum_pages -= 1,
                    1 => narrow.current.maximum_encoded_bytes -= 1,
                    _ => narrow.current.maximum_value_bytes -= 1,
                }
            }
            let reader =
                AuthorizedPackedReader::new_with_cache_budget(&live, &kernel, narrow, CACHE_BYTES)
                    .unwrap();
            let expected = AuthorizedPackedReader::new(&live, &kernel, narrow)
                .unwrap()
                .read(&mut fs, &admin, &request, &NeverCancel)
                .err()
                .unwrap();
            for _ in 0..2 {
                assert_eq!(
                    reader
                        .read(&mut fs, &admin, &request, &NeverCancel)
                        .err()
                        .unwrap(),
                    expected
                );
            }
        }
    }
}

#[test]
fn authorized_packed_cached_reference_warmth_and_maintenance_are_independent_of_visibility() {
    let (mut fs, live, kernel, admin, bob, snapshot) = setup_cached();
    let reader = AuthorizedPackedReader::new_with_cache_budget(
        &live,
        &kernel,
        expansion_limits(ample()),
        CACHE_BYTES,
    )
    .unwrap();
    let uncached = AuthorizedPackedReader::new(&live, &kernel, expansion_limits(ample())).unwrap();
    assert_eq!(uncached.cache_report(&admin).unwrap(), None);
    uncached.clear_cache(&admin).unwrap();
    for bad in [
        uste_storage::MIN_INDEX_CACHE_BYTES - 1,
        uste_storage::MAX_INDEX_CACHE_BYTES + 1,
    ] {
        assert!(matches!(
            AuthorizedPackedReader::new_with_cache_budget(&live, &kernel, read_limits(), bad),
            Err(AuthorizedError::ResourceLimit)
        ));
    }
    for request in queries() {
        reader.clear_cache(&admin).unwrap();
        for principal in [&admin, &bob] {
            let expected = <GraphState as AuthorizedReadState>::read_authorized(
                &snapshot,
                &request,
                &mut |action, target| kernel.authorize(principal, action, target).is_ok(),
            )
            .unwrap();
            assert_eq!(
                uncached
                    .read(&mut fs, principal, &request, &NeverCancel)
                    .unwrap(),
                expected
            );
            assert_eq!(
                reader
                    .read(&mut fs, principal, &request, &NeverCancel)
                    .unwrap(),
                expected
            );
            fs.arm(FaultPlan::default()).unwrap();
            assert_eq!(
                reader
                    .read(&mut fs, principal, &request, &NeverCancel)
                    .unwrap(),
                expected
            );
            assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
        }
    }
    let before = reader.cache_report(&admin).unwrap().unwrap();
    assert!(before.hits > 0 && before.misses > 0);
    assert!(before.accounted_bytes <= CACHE_BYTES);
    let foreign = PolicyKernel::new().authenticate(&mut Identity, &1).unwrap();
    let absent = kernel.authenticate(&mut Identity, &9).unwrap();
    for principal in [&foreign, &absent] {
        assert!(matches!(
            reader.read(&mut fs, principal, &adjacent(), &NeverCancel),
            Err(AuthorizedReadError::Authorization(
                AuthorizedError::Unauthorized
            ))
        ));
        assert!(reader.cache_report(principal).is_err());
        assert!(reader.clear_cache(principal).is_err());
    }
    assert!(matches!(
        reader.cache_report(&bob),
        Err(AuthorizedError::Unauthorized)
    ));
    assert!(matches!(
        reader.clear_cache(&bob),
        Err(AuthorizedError::Unauthorized)
    ));
    for (principal, request, cancel) in [
        (&bob, GraphReadRequest::Record { id: record(2) }, false),
        (&admin, adjacent(), true),
    ] {
        if cancel {
            assert!(reader.read(&mut fs, principal, &request, &Cancel).is_err());
        } else {
            assert!(matches!(
                reader.read(&mut fs, principal, &request, &NeverCancel),
                Err(AuthorizedReadError::Authorization(
                    AuthorizedError::Unauthorized
                ))
            ));
        }
    }
    assert_eq!(reader.cache_report(&admin).unwrap().unwrap(), before);
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
    struct Once(std::cell::Cell<usize>);
    impl uste_txn::Cancellation for Once {
        fn is_cancelled(&self) -> bool {
            let previous = self.0.get();
            self.0.set(previous + 1);
            previous == 3
        }
    }
    reader
        .read(&mut fs, &admin, &adjacent(), &NeverCancel)
        .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert!(matches!(
        reader.read(&mut fs, &admin, &adjacent(), &Once(std::cell::Cell::new(0))),
        Err(AuthorizedReadError::Authorization(
            AuthorizedError::Transaction(TransactionError::Cancelled)
        ))
    ));
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
    let before = reader.cache_report(&admin).unwrap().unwrap();
    reader.clear_cache(&admin).unwrap();
    let cleared = reader.cache_report(&admin).unwrap().unwrap();
    assert_eq!((cleared.resident_pages, cleared.accounted_bytes), (0, 0));
    assert_eq!(
        (cleared.hits, cleared.misses, cleared.evictions),
        (before.hits, before.misses, before.evictions)
    );
}

#[test]
fn authorized_packed_cached_expansion_exact_work_and_narrower_limits_match_uncached() {
    let (mut fs, live, kernel, admin, _, _) = setup_cached();
    let expected = AuthorizedPackedReader::new(&live, &kernel, expansion_limits(ample()))
        .unwrap()
        .read(&mut fs, &admin, &adjacent(), &NeverCancel)
        .unwrap();
    let exact = [89, 1_828_505, 10, 3326, 10];
    let reader = AuthorizedPackedReader::new_with_cache_budget(
        &live,
        &kernel,
        expansion_limits(exact),
        CACHE_BYTES,
    )
    .unwrap();
    for _ in 0..2 {
        assert_eq!(
            reader
                .read(&mut fs, &admin, &adjacent(), &NeverCancel)
                .unwrap(),
            expected
        );
    }
    for dimension in 0..5 {
        let mut narrow = exact;
        narrow[dimension] -= 1;
        let reader = AuthorizedPackedReader::new_with_cache_budget(
            &live,
            &kernel,
            expansion_limits(narrow),
            CACHE_BYTES,
        )
        .unwrap();
        let uncached =
            AuthorizedPackedReader::new(&live, &kernel, expansion_limits(narrow)).unwrap();
        let expected = uncached
            .read(&mut fs, &admin, &adjacent(), &NeverCancel)
            .err()
            .unwrap();
        for _ in 0..2 {
            assert_eq!(
                reader
                    .read(&mut fs, &admin, &adjacent(), &NeverCancel)
                    .err()
                    .unwrap(),
                expected
            );
        }
    }
}

#[test]
fn authorized_packed_cached_small_budgets_preserve_results_under_eviction() {
    let (mut fs, live, kernel, admin, bob, snapshot) = setup_cached();
    for budget in [uste_storage::MIN_INDEX_CACHE_BYTES, 64 * 1024] {
        let reader = AuthorizedPackedReader::new_with_cache_budget(
            &live,
            &kernel,
            expansion_limits(ample()),
            budget,
        )
        .unwrap();
        for _ in 0..3 {
            for principal in [&admin, &bob] {
                for request in queries() {
                    let expected = <GraphState as AuthorizedReadState>::read_authorized(
                        &snapshot,
                        &request,
                        &mut |action, target| kernel.authorize(principal, action, target).is_ok(),
                    )
                    .unwrap();
                    assert_eq!(
                        reader
                            .read(&mut fs, principal, &request, &NeverCancel)
                            .unwrap(),
                        expected
                    );
                    let report = reader.cache_report(&admin).unwrap().unwrap();
                    assert!(report.accounted_bytes <= budget);
                }
            }
        }
        assert!(reader.cache_report(&admin).unwrap().unwrap().evictions > 0);
    }
    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
}

#[test]
fn authorized_packed_cached_late_mutation_requires_clear_to_reauthenticate() {
    use uste_storage::FileSystem;
    for family in [2_u8, 3, 4, 5, 6] {
        let (mut fs, live, kernel, admin, _, _) = setup_cached();
        let request = match family {
            2 => GraphReadRequest::Record { id: record(1) },
            3 => GraphReadRequest::RecordAt {
                id: record(1),
                revision: CommitRevision::FIRST,
            },
            6 => GraphReadRequest::SupportedBy {
                evidence: record(3),
                maximum: 100,
            },
            _ => adjacent(),
        };
        let reader = AuthorizedPackedReader::new_with_cache_budget(
            &live,
            &kernel,
            expansion_limits(ample()),
            CACHE_BYTES,
        )
        .unwrap();
        let expected = reader
            .read(&mut fs, &admin, &request, &NeverCancel)
            .unwrap();
        let physical = live.state().unwrap().current_base().unwrap().families()
            [usize::from(family - 1)]
        .root
        .unwrap()
        .resolve(
            scope(),
            GRAPH_PACKED_PROFILE_V1,
            family,
            CommitRevision::new(8).unwrap(),
        )
        .unwrap();
        let directory = fs
            .open_directory(&fs.root(), &EntryName::new("packed-graph-bridge").unwrap())
            .unwrap();
        let name = EntryName::new(format!(
            "pack-{}",
            physical
                .object
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ))
        .unwrap();
        let file = fs.open_existing(&directory, &name).unwrap();
        let offset = physical.page * 20545 + 137;
        let mut byte = [0];
        assert_eq!(fs.read_at(&file, offset, &mut byte).unwrap(), 1);
        byte[0] ^= 1;
        fs.write_at(&file, offset, &byte).unwrap();
        let before_failure = live.vault_decrypt_report().unwrap();
        assert_eq!(
            reader
                .read(&mut fs, &admin, &request, &NeverCancel)
                .unwrap(),
            expected
        );
        assert_eq!(live.vault_decrypt_report().unwrap(), before_failure);
        assert!(
            AuthorizedPackedReader::new(&live, &kernel, expansion_limits(ample()))
                .unwrap()
                .read(&mut fs, &admin, &request, &NeverCancel)
                .is_err()
        );
        assert_eq!(
            live.vault_decrypt_report().unwrap().failed_calls,
            before_failure.failed_calls + 1
        );
        reader.clear_cache(&admin).unwrap();
        assert!(
            reader
                .read(&mut fs, &admin, &request, &NeverCancel)
                .is_err()
        );
        assert_eq!(
            live.vault_decrypt_report().unwrap().failed_calls,
            before_failure.failed_calls + 2
        );
        byte[0] ^= 1;
        fs.write_at(&file, offset, &byte).unwrap();
        assert_eq!(
            reader
                .read(&mut fs, &admin, &request, &NeverCancel)
                .unwrap(),
            expected
        );
    }
}

#[test]
fn authorized_packed_cached_read_faults_return_no_output_and_recover_cold() {
    let operations = [FsOp::OpenExisting, FsOp::Metadata, FsOp::ReadAt];
    let mut cases = 0;
    for request in queries() {
        let (mut fs, live, kernel, admin, _, _) = setup_cached();
        let reader = AuthorizedPackedReader::new_with_cache_budget(
            &live,
            &kernel,
            expansion_limits(ample()),
            CACHE_BYTES,
        )
        .unwrap();
        let expected = reader
            .read(&mut fs, &admin, &request, &NeverCancel)
            .unwrap();
        let counts = operations.map(|operation| fs.operation_count(operation));
        fs.arm(
            FaultPlan::new([FaultPoint {
                operation: FsOp::ReadAt,
                occurrence: 1,
                action: FaultAction::Error(uste_storage::AdapterErrorKind::Io),
            }])
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            reader
                .read(&mut fs, &admin, &request, &NeverCancel)
                .unwrap(),
            expected
        );
        assert_eq!(fs.pending_faults(), 1);
        reader.clear_cache(&admin).unwrap();
        assert!(
            reader
                .read(&mut fs, &admin, &request, &NeverCancel)
                .is_err()
        );
        assert_eq!(fs.pending_faults(), 0);
        for (operation, count) in operations.into_iter().zip(counts) {
            for occurrence in 1..=count {
                for action in [
                    FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                    FaultAction::CrashBefore,
                    FaultAction::CrashAfter,
                ] {
                    let (mut fs, live, kernel, admin, _, _) = setup_cached();
                    fs.arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                    let reader = AuthorizedPackedReader::new_with_cache_budget(
                        &live,
                        &kernel,
                        expansion_limits(ample()),
                        CACHE_BYTES,
                    )
                    .unwrap();
                    assert!(
                        reader
                            .read(&mut fs, &admin, &request, &NeverCancel)
                            .is_err()
                    );
                    assert_eq!(fs.pending_faults(), 0);
                    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
                    if matches!(action, FaultAction::Error(_)) {
                        // An ordinary read error may retain complete authenticated pages, never
                        // a partial result. Retrying the query must prove the complete result.
                        assert_eq!(
                            reader
                                .read(&mut fs, &admin, &request, &NeverCancel)
                                .unwrap(),
                            expected
                        );
                    }
                    drop(reader);
                    drop(live);
                    let (mut fs, recovered) = recover(reopen(fs, 8, 2_050_000), suffix_limits());
                    let (live, _) = recovered.unwrap();
                    assert_eq!(
                        AuthorizedPackedReader::new_with_cache_budget(
                            &live,
                            &kernel,
                            expansion_limits(ample()),
                            CACHE_BYTES
                        )
                        .unwrap()
                        .read(&mut fs, &admin, &request, &NeverCancel)
                        .unwrap(),
                        expected
                    );
                    cases += 1;
                }
            }
        }
    }
    assert!(cases > 0);
    eprintln!("authorized packed cached read fault cases: {cases}");
}
