use super::*;
#[path = "packed_authorized_cache.rs"]
mod cache;
use uste_graph::{AdjacencyDirection, GraphSnapshot, PackedGraphExpansionLimits};

fn expansion_limits(values: [u64; 5]) -> PackedGraphReadLimits {
    PackedGraphReadLimits {
        expansion: Some(
            PackedGraphExpansionLimits::new(values[0], values[1], values[2], values[3], values[4])
                .unwrap(),
        ),
        ..read_limits()
    }
}
fn ample() -> [u64; 5] {
    [10000, 10000 * 20545, 10000, 4 * 1024 * 1024, 10000]
}
fn setup_expansion() -> (
    Fs,
    Live,
    PolicyKernel,
    AuthenticatedPrincipal,
    AuthenticatedPrincipal,
    GraphSnapshot,
) {
    let (mut fs, mut live, mut kernel, admin, bob) = setup_reads();
    let mut operations = Vec::new();
    for (id, from, to) in [(11, 1, 2), (12, 1, 1), (13, 2, 1), (14, 1, 2)] {
        operations.push(Operation::Create {
            expected: Expected::Absent,
            record: NewRecord::Relationship(NewRelationship {
                id: record(id),
                from: record(from),
                to: record(to),
                relationship_type: text("packed-expansion"),
                properties: Value::Null,
                evidence: vec![record(3)],
                valid_time: ValidTime::Unknown,
            }),
        });
    }
    operations.push(Operation::Create {
        expected: Expected::Absent,
        record: NewRecord::Assertion(NewAssertion {
            id: record(20),
            subject: record(1),
            predicate: text("depends"),
            object: Value::RecordRef(record(2)),
            evidence: vec![record(3)],
            valid_time: ValidTime::Unknown,
        }),
    });
    let transactions = [
        GraphTransaction::new(scope(), operations),
        GraphTransaction::new(
            scope(),
            (11..=14)
                .map(|id| Operation::ActOnRelationship {
                    target: record(id),
                    expected: Expected::Version(uste_graph::RecordVersion::FIRST),
                    action: AssertionAction::Accept,
                    correction: None,
                    correction_expected: None,
                })
                .collect(),
        ),
    ];
    for (offset, tx) in transactions.iter().enumerate() {
        let revision = 6 + offset as u8;
        let bytes = encode_transaction(tx).unwrap();
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
            request(revision, &bytes),
            &mut clock(revision.into()),
            &NeverCancel,
        )
        .unwrap();
        live.rebase_metadata(&mut fs, rebase_limits()).unwrap();
    }
    drop(live);
    let (model, _) = CommitCoordinator::open(
        &mut fs,
        &EntryName::new("packed-graph-bridge").unwrap(),
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(1_910_000),
        CounterEntropy(1_920_000),
        &mut TestKeyAdapter,
        GraphState::new(scope()),
    )
    .unwrap();
    let snapshot = model.read_view().unwrap().state().clone();
    drop(model);
    let (mut fs, result) = recover(reopen(fs, 7, 1_940_000), suffix_limits());
    let (live, _) = result.unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    (fs, live, kernel, admin, bob, snapshot)
}
fn adjacent() -> GraphReadRequest {
    GraphReadRequest::Adjacent {
        entity: record(1),
        direction: AdjacencyDirection::Either,
        maximum: 100,
    }
}
#[test]
fn authorized_packed_expansion_matches_reference_directions_duplicates_support_and_visibility() {
    let (mut fs, live, kernel, admin, bob, snapshot) = setup_expansion();
    let reader = AuthorizedPackedReader::new(&live, &kernel, expansion_limits(ample())).unwrap();
    for principal in [&admin, &bob] {
        for request in [
            adjacent(),
            GraphReadRequest::Adjacent {
                entity: record(1),
                direction: AdjacencyDirection::Incoming,
                maximum: 100,
            },
            GraphReadRequest::Adjacent {
                entity: record(1),
                direction: AdjacencyDirection::Outgoing,
                maximum: 100,
            },
            GraphReadRequest::Adjacent {
                entity: record(99),
                direction: AdjacencyDirection::Either,
                maximum: 100,
            },
            GraphReadRequest::SupportedBy {
                evidence: record(3),
                maximum: 100,
            },
            GraphReadRequest::SupportedBy {
                evidence: record(99),
                maximum: 100,
            },
        ] {
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
    for principal in [&admin, &bob] {
        for request in [
            GraphReadRequest::Adjacent {
                entity: record(1),
                direction: AdjacencyDirection::Either,
                maximum: 0,
            },
            GraphReadRequest::SupportedBy {
                evidence: record(3),
                maximum: 0,
            },
        ] {
            assert!(matches!(
                reader.read(&mut fs, principal, &request, &NeverCancel),
                Err(AuthorizedReadError::Domain(GraphDiskError::Graph(
                    GraphError::ResultLimit { .. }
                )))
            ));
        }
    }
    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
}

#[test]
fn authorized_packed_expansion_all_five_aggregate_limits_are_exact_and_shared() {
    let (mut fs, live, kernel, admin, _, _) = setup_expansion();
    let expected = AuthorizedPackedReader::new(&live, &kernel, expansion_limits(ample()))
        .unwrap()
        .read(&mut fs, &admin, &adjacent(), &NeverCancel)
        .unwrap();
    let measured_pages = fs.operation_count(FsOp::ReadAt);
    let mut exact = ample();
    for dimension in 0..5 {
        let mut lower = 0;
        let mut upper = ample()[dimension];
        while lower < upper {
            let middle = lower + (upper - lower) / 2;
            let mut candidate = ample();
            candidate[dimension] = middle;
            match AuthorizedPackedReader::new(&live, &kernel, expansion_limits(candidate))
                .unwrap()
                .read(&mut fs, &admin, &adjacent(), &NeverCancel)
            {
                Ok(output) => {
                    assert_eq!(output, expected);
                    upper = middle;
                }
                Err(_) => lower = middle + 1,
            }
        }
        exact[dimension] = lower;
        assert!(lower > 0);
    }
    assert_eq!(exact[0], measured_pages);
    assert_eq!(exact[1], measured_pages * 20545);
    assert_eq!(exact[4], 10); // Five distinct edges, each plus its neighbor; self-loop emitted once.
    assert_eq!(
        AuthorizedPackedReader::new(&live, &kernel, expansion_limits(exact))
            .unwrap()
            .read(&mut fs, &admin, &adjacent(), &NeverCancel)
            .unwrap(),
        expected
    );
    for dimension in 0..5 {
        let mut narrow = exact;
        narrow[dimension] -= 1;
        assert!(
            AuthorizedPackedReader::new(&live, &kernel, expansion_limits(narrow))
                .unwrap()
                .read(&mut fs, &admin, &adjacent(), &NeverCancel)
                .is_err()
        );
    }
    eprintln!("packed expansion exact [pages, encoded, candidates, returned, lookups]: {exact:?}");
}

#[test]
fn authorized_packed_expansion_permission_disabled_and_cancellation_boundaries() {
    let (mut fs, live, kernel, admin, _) = setup(); // No ExpandGraph grant.
    fs.arm(FaultPlan::default()).unwrap();
    assert!(matches!(
        AuthorizedPackedReader::new(&live, &kernel, expansion_limits(ample()))
            .unwrap()
            .read(&mut fs, &admin, &adjacent(), &NeverCancel),
        Err(AuthorizedReadError::Authorization(
            AuthorizedError::Unauthorized
        ))
    ));
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
    let (mut fs, live, kernel, admin, _, _) = setup_expansion();
    assert!(matches!(
        AuthorizedPackedReader::new(&live, &kernel, read_limits())
            .unwrap()
            .read(&mut fs, &admin, &adjacent(), &NeverCancel),
        Err(AuthorizedReadError::Domain(
            GraphDiskError::UnsupportedRequest
        ))
    ));
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
    let reader = AuthorizedPackedReader::new(&live, &kernel, expansion_limits(ample())).unwrap();
    assert!(matches!(
        reader.read(&mut fs, &admin, &adjacent(), &Cancel),
        Err(AuthorizedReadError::Authorization(
            AuthorizedError::Transaction(TransactionError::Cancelled)
        ))
    ));
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
    struct Once(std::cell::Cell<usize>);
    impl uste_txn::Cancellation for Once {
        fn is_cancelled(&self) -> bool {
            let n = self.0.get();
            self.0.set(n + 1);
            n == 3
        }
    }
    assert!(matches!(
        reader.read(&mut fs, &admin, &adjacent(), &Once(std::cell::Cell::new(0))),
        Err(AuthorizedReadError::Authorization(
            AuthorizedError::Transaction(TransactionError::Cancelled)
        ))
    ));
    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
    assert!(
        PackedGraphExpansionLimits::new(0, 0, uste_graph::MAX_TRAVERSAL_VISITS as u64 + 1, 0, 0)
            .is_err()
    );
    assert!(
        PackedGraphExpansionLimits::new(
            0,
            0,
            0,
            uste_storage::MAX_INDEX_RESULT_BYTES as u64 + 1,
            0
        )
        .is_err()
    );
    assert!(
        PackedGraphExpansionLimits::new(
            0,
            0,
            0,
            0,
            2 * uste_graph::MAX_TRAVERSAL_VISITS as u64 + 1
        )
        .is_err()
    );
    assert!(
        PackedGraphExpansionLimits::new(
            uste_storage::packed_tree_cursor::MAX_CURSOR_PAGES + 1,
            0,
            0,
            0,
            0
        )
        .is_err()
    );
    assert!(
        PackedGraphExpansionLimits::new(
            0,
            uste_storage::packed_tree_cursor::MAX_CURSOR_ENCODED_BYTES + 1,
            0,
            0,
            0
        )
        .is_err()
    );
}

#[test]
fn authorized_packed_expansion_late_secondary_and_current_corruption_fail_closed() {
    use uste_storage::FileSystem;
    for family in [2_u8, 4, 5, 6] {
        let (mut fs, live, kernel, admin, _, _) = setup_expansion();
        let request = if family == 6 {
            GraphReadRequest::SupportedBy {
                evidence: record(3),
                maximum: 100,
            }
        } else {
            adjacent()
        };
        let location = live.state().unwrap().current_base().unwrap().families()
            [usize::from(family - 1)]
        .root
        .unwrap();
        let physical = location
            .resolve(
                scope(),
                GRAPH_PACKED_PROFILE_V1,
                family,
                CommitRevision::new(7).unwrap(),
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
        let reader =
            AuthorizedPackedReader::new(&live, &kernel, expansion_limits(ample())).unwrap();
        assert!(
            reader
                .read(&mut fs, &admin, &request, &NeverCancel)
                .is_err()
        );
        byte[0] ^= 1;
        fs.write_at(&file, offset, &byte).unwrap();
        assert!(reader.read(&mut fs, &admin, &request, &NeverCancel).is_ok());
    }
}

#[test]
fn authorized_packed_expansion_every_observed_read_fault_returns_no_partial_output() {
    let operations = [FsOp::OpenExisting, FsOp::Metadata, FsOp::ReadAt];
    let mut cases = 0;
    for request in [
        adjacent(),
        GraphReadRequest::SupportedBy {
            evidence: record(3),
            maximum: 100,
        },
    ] {
        let (mut fs, live, kernel, admin, _, _) = setup_expansion();
        let expected = AuthorizedPackedReader::new(&live, &kernel, expansion_limits(ample()))
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
                    let (mut fs, live, kernel, admin, _, _) = setup_expansion();
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
                        AuthorizedPackedReader::new(&live, &kernel, expansion_limits(ample()))
                            .unwrap()
                            .read(&mut fs, &admin, &request, &NeverCancel)
                            .is_err()
                    );
                    assert_eq!(fs.pending_faults(), 0);
                    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
                    drop(live);
                    let (mut fs, result) = recover(reopen(fs, 7, 1_970_000), suffix_limits());
                    let (live, _) = result.unwrap();
                    assert_eq!(
                        AuthorizedPackedReader::new(&live, &kernel, expansion_limits(ample()))
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
    eprintln!("authorized packed expansion fault cases: {cases}");
}
