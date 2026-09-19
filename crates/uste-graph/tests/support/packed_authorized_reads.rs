use super::*;
#[path = "packed_authorized_expansion.rs"]
mod expansion;
use uste_graph::{GraphReadOutput, GraphReadRequest, PackedGraphReadLimits};
use uste_txn::{AuthorizedPackedReader, AuthorizedReadError, AuthorizedReadState};

fn read_limits() -> PackedGraphReadLimits {
    PackedGraphReadLimits {
        current: preparation_limits().lookup,
        historical: export_limits(),
        expansion: None,
    }
}

#[test]
fn authorized_packed_points_history_scope_and_stale_policy_refuse_before_io() {
    let (mut fs, live, kernel, admin, _) = setup(); // This policy grants no ReadHistory.
    let reader = AuthorizedPackedReader::new(&live, &kernel, read_limits()).unwrap();
    fs.arm(
        FaultPlan::new([FaultPoint {
            operation: FsOp::ReadAt,
            occurrence: 1,
            action: FaultAction::Error(uste_storage::AdapterErrorKind::Io),
        }])
        .unwrap(),
    )
    .unwrap();
    let other = NamespaceRef::new(scope().database(), NamespaceId::from_bytes([99; 16]));
    for request in [
        GraphReadRequest::RecordAt {
            id: record(1),
            revision: CommitRevision::FIRST,
        },
        GraphReadRequest::Record {
            id: RecordRef::new(other.database(), other.namespace(), record(1).record()),
        },
    ] {
        assert!(matches!(
            reader.read(&mut fs, &admin, &request, &NeverCancel),
            Err(AuthorizedReadError::Authorization(
                AuthorizedError::Unauthorized
            ))
        ));
    }
    let mut stale = PolicyKernel::new();
    stale
        .install_initial_policy(NamespacePolicy::new(
            scope(),
            PolicyVersion::new(2).unwrap(),
            QuotaLimits::new(100, 100, 100, 1, 1).unwrap(),
        ))
        .unwrap();
    assert!(matches!(
        AuthorizedPackedReader::new(&live, &stale, read_limits()),
        Err(AuthorizedError::InvalidPolicy)
    ));
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
    assert_eq!(fs.pending_faults(), 1);
    assert!(
        reader
            .read(
                &mut fs,
                &admin,
                &GraphReadRequest::Record { id: record(1) },
                &NeverCancel
            )
            .is_err()
    );
    assert_eq!(fs.pending_faults(), 0);
}
fn setup_reads() -> (
    Fs,
    Live,
    PolicyKernel,
    AuthenticatedPrincipal,
    AuthenticatedPrincipal,
) {
    let (mut fs, mut live, mut kernel, admin, bob) = setup();
    let quota = QuotaLimits::new(1024 * 1024, 1024 * 1024, 1024 * 1024, 8, 1024).unwrap();
    let mut policy = NamespacePolicy::new(scope(), PolicyVersion::new(3).unwrap(), quota);
    for who in [1, 2] {
        let mut grant = NamespaceGrant::new(
            PermissionSet::from_actions([
                Action::Commit,
                Action::ReadRecord,
                Action::ReadHistory,
                Action::ReadOwnOutcome,
                Action::ManagePolicy,
                Action::ExpandGraph,
            ]),
            quota,
        );
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
            expected: PolicyVersion::new(2).unwrap(),
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
        request(5, &bytes),
        &mut clock(5),
        &NeverCancel,
    )
    .unwrap();
    live.rebase_metadata(&mut fs, rebase_limits()).unwrap();
    (fs, live, kernel, admin, bob)
}

#[test]
fn authorized_packed_points_match_full_replay_and_cold_history() {
    let (mut fs, mut live, mut kernel, admin, bob) = setup_reads();
    let bytes = encode_transaction(&cases()[0]).unwrap();
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
        request(6, &bytes),
        &mut clock(6),
        &NeverCancel,
    )
    .unwrap();
    live.rebase_metadata(&mut fs, rebase_limits()).unwrap();
    drop(live);
    // Independent full replay is a small-fixture oracle, not the packed implementation path.
    let (model, _) = CommitCoordinator::open(
        &mut fs,
        &EntryName::new("packed-graph-bridge").unwrap(),
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(1_720_000),
        CounterEntropy(1_730_000),
        &mut TestKeyAdapter,
        GraphState::new(scope()),
    )
    .unwrap();
    let snapshot = model.read_view().unwrap().state().clone();
    drop(model);
    let (mut fs, recovered) = recover(reopen(fs, 6, 1_750_000), suffix_limits());
    let (live, _) = recovered.unwrap();
    let reader = AuthorizedPackedReader::new(&live, &kernel, read_limits()).unwrap();
    for principal in [&admin, &bob] {
        for id in [1, 3, 4, 99] {
            let mut requests = vec![GraphReadRequest::Record { id: record(id) }];
            requests.extend((1..=6).map(|revision| GraphReadRequest::RecordAt {
                id: record(id),
                revision: CommitRevision::new(revision).unwrap(),
            }));
            for request in requests {
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
            }
        }
    }
    assert_eq!(
        reader
            .read(
                &mut fs,
                &bob,
                &GraphReadRequest::Record { id: record(1) },
                &NeverCancel
            )
            .unwrap(),
        GraphReadOutput::Record(None)
    ); // Current embedded reference is hidden.
    assert!(matches!(
        reader
            .read(
                &mut fs,
                &bob,
                &GraphReadRequest::RecordAt {
                    id: record(1),
                    revision: CommitRevision::new(1).unwrap()
                },
                &NeverCancel
            )
            .unwrap(),
        GraphReadOutput::Record(Some(_))
    )); // Historical version has no hidden reference.
    assert!(matches!(
        reader.read(
            &mut fs,
            &admin,
            &GraphReadRequest::RecordAt {
                id: record(1),
                revision: CommitRevision::new(7).unwrap()
            },
            &NeverCancel
        ),
        Err(AuthorizedReadError::Domain(GraphDiskError::Graph(
            GraphError::UnknownReadView(_)
        )))
    ));
}

#[test]
fn authorized_packed_points_deny_before_io_and_cancellation_is_sticky() {
    let (mut fs, live, kernel, admin, bob) = setup_reads();
    let foreign = PolicyKernel::new().authenticate(&mut Identity, &1).unwrap();
    let absent = kernel.authenticate(&mut Identity, &9).unwrap();
    let reader = AuthorizedPackedReader::new(&live, &kernel, read_limits()).unwrap();
    fs.arm(
        FaultPlan::new([FaultPoint {
            operation: FsOp::ReadAt,
            occurrence: 1,
            action: FaultAction::Error(uste_storage::AdapterErrorKind::Io),
        }])
        .unwrap(),
    )
    .unwrap();
    for principal in [&foreign, &absent, &bob] {
        assert!(matches!(
            reader.read(
                &mut fs,
                principal,
                &GraphReadRequest::Record { id: record(2) },
                &NeverCancel
            ),
            Err(AuthorizedReadError::Authorization(
                AuthorizedError::Unauthorized
            ))
        ));
    }
    assert!(matches!(
        reader.read(
            &mut fs,
            &admin,
            &GraphReadRequest::Record { id: record(1) },
            &Cancel
        ),
        Err(AuthorizedReadError::Authorization(
            AuthorizedError::Transaction(TransactionError::Cancelled)
        ))
    ));
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
    assert_eq!(fs.pending_faults(), 1);
    assert!(
        reader
            .read(
                &mut fs,
                &admin,
                &GraphReadRequest::Record { id: record(1) },
                &NeverCancel
            )
            .is_err()
    );
    assert_eq!(fs.pending_faults(), 0);
    struct Once(std::cell::Cell<usize>);
    impl uste_txn::Cancellation for Once {
        fn is_cancelled(&self) -> bool {
            let n = self.0.get();
            self.0.set(n + 1);
            n == 1
        }
    }
    assert!(matches!(
        reader.read(
            &mut fs,
            &admin,
            &GraphReadRequest::Record { id: record(4) },
            &Once(std::cell::Cell::new(0))
        ),
        Err(AuthorizedReadError::Authorization(
            AuthorizedError::Transaction(TransactionError::Cancelled)
        ))
    ));
    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
}

#[test]
fn authorized_packed_points_exact_limits_and_late_corruption() {
    use uste_storage::FileSystem;
    for historical in [false, true] {
        let (mut fs, live, kernel, admin, _) = setup_reads();
        let request = if historical {
            GraphReadRequest::RecordAt {
                id: record(4),
                revision: CommitRevision::new(2).unwrap(),
            }
        } else {
            GraphReadRequest::Record { id: record(4) }
        };
        fs.arm(FaultPlan::default()).unwrap();
        let output = AuthorizedPackedReader::new(&live, &kernel, read_limits())
            .unwrap()
            .read(&mut fs, &admin, &request, &NeverCancel)
            .unwrap();
        let GraphReadOutput::Record(Some(value)) = &output else {
            panic!("present fixture")
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
        assert_eq!(
            AuthorizedPackedReader::new(&live, &kernel, exact)
                .unwrap()
                .read(&mut fs, &admin, &request, &NeverCancel)
                .unwrap(),
            output
        );
        for variant in 0..3 {
            let mut narrow = exact;
            if historical {
                match variant {
                    0 => narrow.historical.maximum_pages -= 1,
                    1 => narrow.historical.maximum_encoded_bytes -= 1,
                    _ => narrow.historical.maximum_returned_bytes -= 1,
                }
            } else {
                match variant {
                    0 => narrow.current.maximum_pages -= 1,
                    1 => narrow.current.maximum_encoded_bytes -= 1,
                    _ => narrow.current.maximum_value_bytes -= 1,
                }
            }
            assert!(
                AuthorizedPackedReader::new(&live, &kernel, narrow)
                    .unwrap()
                    .read(&mut fs, &admin, &request, &NeverCancel)
                    .is_err()
            );
        }
        let family = if historical { 3 } else { 2 };
        let location = live.state().unwrap().current_base().unwrap().families()
            [usize::from(family - 1)]
        .root
        .unwrap();
        let physical = location
            .resolve(
                scope(),
                GRAPH_PACKED_PROFILE_V1,
                family,
                CommitRevision::new(5).unwrap(),
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
        assert!(
            AuthorizedPackedReader::new(&live, &kernel, read_limits())
                .unwrap()
                .read(&mut fs, &admin, &request, &NeverCancel)
                .is_err()
        );
        byte[0] ^= 1;
        fs.write_at(&file, offset, &byte).unwrap();
        assert_eq!(
            AuthorizedPackedReader::new(&live, &kernel, read_limits())
                .unwrap()
                .read(&mut fs, &admin, &request, &NeverCancel)
                .unwrap(),
            output
        );
    }
}

#[test]
fn authorized_packed_points_all_observed_read_faults_return_no_result() {
    let operations = [FsOp::OpenExisting, FsOp::Metadata, FsOp::ReadAt];
    let mut cases = 0;
    for request in [
        GraphReadRequest::Record { id: record(4) },
        GraphReadRequest::RecordAt {
            id: record(4),
            revision: CommitRevision::new(2).unwrap(),
        },
    ] {
        let (mut fs, live, kernel, admin, _) = setup_reads();
        fs.arm(FaultPlan::default()).unwrap();
        AuthorizedPackedReader::new(&live, &kernel, read_limits())
            .unwrap()
            .read(&mut fs, &admin, &request, &NeverCancel)
            .unwrap();
        let counts = operations.map(|operation| fs.operation_count(operation));
        for (operation, count) in operations.into_iter().zip(counts) {
            for occurrence in 1..=count {
                for action in [
                    FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                    FaultAction::CrashBefore,
                    FaultAction::CrashAfter,
                ] {
                    let (mut fs, live, kernel, admin, _) = setup_reads();
                    fs.arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                    assert!(
                        AuthorizedPackedReader::new(&live, &kernel, read_limits())
                            .unwrap()
                            .read(&mut fs, &admin, &request, &NeverCancel)
                            .is_err()
                    );
                    assert_eq!(fs.pending_faults(), 0);
                    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
                    drop(live);
                    let (mut fs, result) = recover(reopen(fs, 5, 1_790_000), suffix_limits());
                    let (live, _) = result.unwrap();
                    assert!(
                        AuthorizedPackedReader::new(&live, &kernel, read_limits())
                            .unwrap()
                            .read(&mut fs, &admin, &request, &NeverCancel)
                            .is_ok()
                    );
                    cases += 1;
                }
            }
        }
    }
    assert!(cases > 0);
    eprintln!("authorized packed point fault cases: {cases}");
}

#[test]
fn authorized_packed_points_pending_and_uncertain_writes_cannot_expose_stale_graph() {
    for uncertain in [false, true] {
        let (mut fs, mut live, mut kernel, admin, _) = setup_reads();
        let bytes = encode_transaction(&cases()[0]).unwrap();
        if uncertain {
            fs.arm(
                FaultPlan::new([FaultPoint {
                    operation: FsOp::SyncData,
                    occurrence: 2,
                    action: FaultAction::CrashAfter,
                }])
                .unwrap(),
            )
            .unwrap();
        }
        let result = AuthorizedPackedWriter::new(
            &mut live,
            &mut kernel,
            preparation(4 * 1024 * 1024),
            publication(0),
        )
        .unwrap()
        .commit(
            &mut fs,
            &admin,
            request(6, &bytes),
            &mut clock(6),
            &NeverCancel,
        );
        if uncertain {
            assert!(matches!(
                result,
                Err(AuthorizedDiskWriteError::Authorization(
                    AuthorizedError::Transaction(TransactionError::OutcomeUnknown)
                ))
            ));
            assert!(matches!(
                AuthorizedPackedReader::new(&live, &kernel, read_limits()),
                Err(AuthorizedError::Transaction(
                    TransactionError::OutcomeUnknown
                ))
            ));
            drop(live);
            let (mut recovered_fs, result) = recover(reopen(fs, 5, 1_820_000), suffix_limits());
            let (recovered, _) = result.unwrap();
            assert!(
                AuthorizedPackedReader::new(&recovered, &kernel, read_limits())
                    .unwrap()
                    .read(
                        &mut recovered_fs,
                        &admin,
                        &GraphReadRequest::Record { id: record(1) },
                        &NeverCancel
                    )
                    .is_ok()
            );
        } else {
            assert!(matches!(
                result,
                Err(AuthorizedDiskWriteError::CommittedPublication { .. })
            ));
            fs.arm(FaultPlan::default()).unwrap();
            assert!(matches!(
                AuthorizedPackedReader::new(&live, &kernel, read_limits()),
                Err(AuthorizedError::InvalidPolicy)
            ));
            assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
            AuthorizedPackedWriter::new(&mut live, &mut kernel, preparation(1), publication(10000))
                .unwrap()
                .commit(&mut fs, &admin, request(6, &bytes), &mut clock(7), &Cancel)
                .unwrap();
            assert!(
                AuthorizedPackedReader::new(&live, &kernel, read_limits())
                    .unwrap()
                    .read(
                        &mut fs,
                        &admin,
                        &GraphReadRequest::Record { id: record(1) },
                        &NeverCancel
                    )
                    .is_ok()
            );
        }
    }
}
