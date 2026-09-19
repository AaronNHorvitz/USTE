use super::*;
#[path = "packed_live.rs"]
mod live;
use uste_graph::{
    PackedGraphBase, PackedGraphDelta, PackedGraphStageLimits, prepare_packed_graph_delta,
    stage_packed_graph_delta,
};
use uste_txn::RecoveredFrontierTransaction;

struct Input {
    fs: Fs,
    recovery: Recovery,
    base: PackedGraphBase,
    old: RecoveredFrontierTransaction,
    target: RecoveredFrontierTransaction,
    plan: PackedGraphDelta,
    old_digest: [u8; 32],
    new_digest: [u8; 32],
}
fn stage_limits(batch: usize) -> PackedGraphStageLimits {
    PackedGraphStageLimits {
        certificates: limits(batch).certificates,
        batch: limits(batch).batch,
        deltas_per_batch: batch,
        maximum_batches: 10000,
        maximum_read_pages: 100000,
        maximum_written_pages: 10000,
    }
}
fn input(request: &GraphTransaction) -> Input {
    let (mut memory, name, old_digest) = fixture();
    let (mut coordinator, _) = CommitCoordinator::open(
        &mut memory,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(950_000),
        CounterEntropy(960_000),
        &mut TestKeyAdapter,
        GraphState::new(scope()),
    )
    .unwrap();
    commit(&mut coordinator, &mut memory, 4, request.clone());
    let snapshot = coordinator.read_view().unwrap().state().clone();
    let new_digest = GraphState::logical_state_digest(&snapshot).unwrap();
    drop(coordinator);
    memory.restart().unwrap();
    reopen_input(
        FaultFileSystem::new(memory, FaultPlan::default()),
        request,
        old_digest,
        new_digest,
        970_000,
    )
}
fn reopen_input(
    mut fs: Fs,
    request: &GraphTransaction,
    old_digest: [u8; 32],
    new_digest: [u8; 32],
    entropy: u64,
) -> Input {
    let name = EntryName::new("packed-graph-bridge").unwrap();
    let (mut recovery, source, target) = open(&mut fs, &name, entropy);
    let mut cursor = recovery
        .open_transaction_cursor(
            CommitRevision::new(3).unwrap(),
            CommitRevision::new(3).unwrap(),
            1,
            1024 * 1024,
        )
        .unwrap();
    let old = recovery
        .next_recovered_transaction(&mut fs, &mut cursor)
        .unwrap()
        .unwrap();
    assert!(
        recovery
            .next_recovered_transaction(&mut fs, &mut cursor)
            .unwrap()
            .is_none()
    );
    recovery.finish_transaction_cursor(cursor).unwrap();
    let (base, _) =
        bridge_graph_base_to_packed(&mut recovery, &mut fs, &source, &old, limits(2)).unwrap();
    let maintenance = recovery
        .packed_indexes_with_io(&mut fs, &target, limits(2).certificates)
        .unwrap();
    let prepared = prepare_packed_graph_transaction(
        &maintenance,
        &mut fs,
        &base,
        request.clone(),
        preparation_limits(),
    )
    .unwrap()
    .0;
    assert_eq!(prepared.result_digest(), target.outcome().result_digest);
    let plan = prepare_packed_graph_delta(
        prepared,
        GraphStateDeltaLimits::new(1000, 16 * 1024 * 1024).unwrap(),
    )
    .unwrap();
    Input {
        fs,
        recovery,
        base,
        old,
        target,
        plan,
        old_digest,
        new_digest,
    }
}

#[test]
fn packed_graph_staging_every_observed_fault_preserves_old_base_and_restarts() {
    let request = complex_request();
    let mut observed = input(&request);
    observed.fs.arm(FaultPlan::default()).unwrap();
    let (expected, _) = stage_packed_graph_delta(
        &mut observed.recovery,
        &mut observed.fs,
        &observed.base,
        &observed.target,
        &observed.plan,
        stage_limits(2),
    )
    .unwrap();
    let expected = expected.publication_claims().state_digest;
    let counts = [
        FsOp::OpenExisting,
        FsOp::Metadata,
        FsOp::ReadAt,
        FsOp::CreateNew,
        FsOp::WriteAt,
        FsOp::SetLen,
        FsOp::SyncAll,
        FsOp::SyncData,
        FsOp::SyncDirectory,
    ]
    .map(|op| (op, observed.fs.operation_count(op)));
    let mut cases = 0;
    for (operation, count) in counts {
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let mut input = input(&request);
                let old_families = input.base.families();
                input
                    .fs
                    .arm(
                        FaultPlan::new([FaultPoint {
                            operation,
                            occurrence,
                            action,
                        }])
                        .unwrap(),
                    )
                    .unwrap();
                assert!(
                    stage_packed_graph_delta(
                        &mut input.recovery,
                        &mut input.fs,
                        &input.base,
                        &input.target,
                        &input.plan,
                        stage_limits(2)
                    )
                    .is_err(),
                    "{operation:?} {occurrence} {action:?}"
                );
                assert_eq!(input.fs.pending_faults(), 0);
                assert!(input.base.families() == old_families);
                drop(input.recovery);
                input.fs.restart().unwrap();
                let mut input = reopen_input(
                    input.fs,
                    &request,
                    input.old_digest,
                    input.new_digest,
                    990_000,
                );
                let (roots, _) = input
                    .recovery
                    .discover_packed_roots_at_revision(
                        &mut input.fs,
                        GRAPH_PACKED_PROFILE_V1,
                        input.target.revision(),
                        limits(2).certificates,
                        uste_storage::journal::PackedRootDiscoveryLimits::new(2, 2 * 4177).unwrap(),
                    )
                    .unwrap();
                assert!(roots.is_empty());
                let (next, _) = stage_packed_graph_delta(
                    &mut input.recovery,
                    &mut input.fs,
                    &input.base,
                    &input.target,
                    &input.plan,
                    stage_limits(2),
                )
                .unwrap();
                assert_eq!(next.publication_claims().state_digest, expected);
                assert_eq!(
                    packed_graph_v1_digest(
                        &mut input.recovery,
                        &mut input.fs,
                        &next,
                        &input.target,
                        limits(2).certificates,
                        export_limits(),
                        4 * 1024 * 1024
                    )
                    .unwrap(),
                    input.new_digest
                );
                assert_eq!(
                    packed_graph_v1_digest(
                        &mut input.recovery,
                        &mut input.fs,
                        &input.base,
                        &input.old,
                        limits(2).certificates,
                        export_limits(),
                        4 * 1024 * 1024
                    )
                    .unwrap(),
                    input.old_digest
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 333);
}

#[test]
fn packed_graph_delta_exact_retention_and_staging_corruption_owner_refusal() {
    use uste_storage::FileSystem;
    let request = complex_request();
    let mut input = input(&request);
    for (count, bytes, accepted) in [
        (input.plan.delta_count(), input.plan.logical_bytes(), true),
        (
            input.plan.delta_count() - 1,
            input.plan.logical_bytes(),
            false,
        ),
        (
            input.plan.delta_count(),
            input.plan.logical_bytes() - 1,
            false,
        ),
    ] {
        let maintenance = input
            .recovery
            .packed_indexes_with_io(&mut input.fs, &input.target, limits(2).certificates)
            .unwrap();
        let prepared = prepare_packed_graph_transaction(
            &maintenance,
            &mut input.fs,
            &input.base,
            request.clone(),
            preparation_limits(),
        )
        .unwrap()
        .0;
        input.fs.arm(FaultPlan::default()).unwrap();
        let result =
            prepare_packed_graph_delta(prepared, GraphStateDeltaLimits::new(count, bytes).unwrap());
        assert_eq!(result.is_ok(), accepted);
        assert_eq!(input.fs.operation_count(FsOp::ReadAt), 0);
        assert_eq!(input.fs.operation_count(FsOp::WriteAt), 0);
    }
    for family in [2_u8, 3, 7] {
        let physical = input.base.families()[usize::from(family - 1)]
            .root
            .unwrap()
            .resolve(
                scope(),
                GRAPH_PACKED_PROFILE_V1,
                family,
                input.old.revision(),
            )
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
        let directory = input
            .fs
            .open_directory(
                &input.fs.root(),
                &EntryName::new("packed-graph-bridge").unwrap(),
            )
            .unwrap();
        let file = input.fs.open_existing(&directory, &name).unwrap();
        let offset = physical.page * 20545 + 137;
        let mut byte = [0];
        assert_eq!(input.fs.read_at(&file, offset, &mut byte).unwrap(), 1);
        byte[0] ^= 1;
        assert_eq!(input.fs.write_at(&file, offset, &byte).unwrap(), 1);
        assert!(
            stage_packed_graph_delta(
                &mut input.recovery,
                &mut input.fs,
                &input.base,
                &input.target,
                &input.plan,
                stage_limits(2)
            )
            .is_err()
        );
        byte[0] ^= 1;
        assert_eq!(input.fs.write_at(&file, offset, &byte).unwrap(), 1);
        let (next, _) = stage_packed_graph_delta(
            &mut input.recovery,
            &mut input.fs,
            &input.base,
            &input.target,
            &input.plan,
            stage_limits(2),
        )
        .unwrap();
        assert_eq!(
            packed_graph_v1_digest(
                &mut input.recovery,
                &mut input.fs,
                &next,
                &input.target,
                limits(2).certificates,
                export_limits(),
                4 * 1024 * 1024
            )
            .unwrap(),
            input.new_digest
        );
    }
    let mut foreign = self::input(&request);
    foreign.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        stage_packed_graph_delta(
            &mut foreign.recovery,
            &mut foreign.fs,
            &input.base,
            &foreign.target,
            &input.plan,
            stage_limits(2)
        )
        .is_err()
    );
    assert_eq!(foreign.fs.operation_count(FsOp::CreateNew), 0);
    // A target base is not its own predecessor, even when paired with the same certified result.
    let (next, _) = stage_packed_graph_delta(
        &mut input.recovery,
        &mut input.fs,
        &input.base,
        &input.target,
        &input.plan,
        stage_limits(2),
    )
    .unwrap();
    input.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        stage_packed_graph_delta(
            &mut input.recovery,
            &mut input.fs,
            &next,
            &input.target,
            &input.plan,
            stage_limits(2)
        )
        .is_err()
    );
    assert_eq!(input.fs.operation_count(FsOp::ReadAt), 0);
    assert_eq!(input.fs.operation_count(FsOp::CreateNew), 0);
}

#[test]
fn packed_graph_staging_exact_limits_and_receipt_request_binding() {
    let request = complex_request();
    let mut input = input(&request);
    let (_, report) = stage_packed_graph_delta(
        &mut input.recovery,
        &mut input.fs,
        &input.base,
        &input.target,
        &input.plan,
        stage_limits(2),
    )
    .unwrap();
    let exact = PackedGraphStageLimits {
        maximum_batches: report.batches,
        maximum_read_pages: report.read_pages,
        maximum_written_pages: report.written_pages,
        ..stage_limits(2)
    };
    let (_, actual) = stage_packed_graph_delta(
        &mut input.recovery,
        &mut input.fs,
        &input.base,
        &input.target,
        &input.plan,
        exact,
    )
    .unwrap();
    assert_eq!(actual, report);
    for index in 0..5 {
        let mut limits = exact;
        match index {
            0 => limits.maximum_batches -= 1,
            1 => limits.maximum_read_pages -= 1,
            2 => limits.maximum_written_pages -= 1,
            3 => limits.deltas_per_batch = 0,
            4 => limits.deltas_per_batch = 513,
            _ => unreachable!(),
        }
        input.fs.arm(FaultPlan::default()).unwrap();
        assert!(
            stage_packed_graph_delta(
                &mut input.recovery,
                &mut input.fs,
                &input.base,
                &input.target,
                &input.plan,
                limits
            )
            .is_err()
        );
        if [0, 3, 4].contains(&index) {
            assert_eq!(input.fs.operation_count(FsOp::ReadAt), 0);
            assert_eq!(input.fs.operation_count(FsOp::CreateNew), 0);
        }
    }
    input.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        stage_packed_graph_delta(
            &mut input.recovery,
            &mut input.fs,
            &input.base,
            &input.old,
            &input.plan,
            exact
        )
        .is_err()
    );
    assert_eq!(input.fs.operation_count(FsOp::ReadAt), 0);
    assert_eq!(input.fs.operation_count(FsOp::CreateNew), 0);
    // Different canonical precondition, identical successful record changes and result digest.
    let mut alternate = request.operations().to_vec();
    let Operation::ReplaceEntity { expected, .. } = &mut alternate[0] else {
        panic!("replace")
    };
    *expected = Expected::Version(uste_graph::RecordVersion::FIRST);
    let alternate = GraphTransaction::new(scope(), alternate);
    let maintenance = input
        .recovery
        .packed_indexes_with_io(&mut input.fs, &input.target, limits(2).certificates)
        .unwrap();
    let prepared = prepare_packed_graph_transaction(
        &maintenance,
        &mut input.fs,
        &input.base,
        alternate,
        preparation_limits(),
    )
    .unwrap()
    .0;
    assert_eq!(
        prepared.result_digest(),
        input.target.outcome().result_digest
    );
    let alternate = prepare_packed_graph_delta(
        prepared,
        GraphStateDeltaLimits::new(1000, 16 * 1024 * 1024).unwrap(),
    )
    .unwrap();
    input.fs.arm(FaultPlan::default()).unwrap();
    assert!(
        stage_packed_graph_delta(
            &mut input.recovery,
            &mut input.fs,
            &input.base,
            &input.target,
            &alternate,
            exact
        )
        .is_err()
    );
    assert_eq!(input.fs.operation_count(FsOp::ReadAt), 0);
    assert_eq!(input.fs.operation_count(FsOp::CreateNew), 0);
}

#[test]
fn packed_graph_staging_partition_independence_matches_exact_old_and_new_v1_states() {
    for request in [
        cases()[0].clone(),
        cases()[6].clone(),
        cases()[18].clone(),
        complex_request(),
        cases()[20].clone(),
    ] {
        let mut input = input(&request);
        let old_families = input.base.families();
        let mut commitment = None;
        for batch in [1, 2, 512] {
            let (next, report) = stage_packed_graph_delta(
                &mut input.recovery,
                &mut input.fs,
                &input.base,
                &input.target,
                &input.plan,
                stage_limits(batch),
            )
            .unwrap();
            assert_eq!(
                next.anchor(),
                (input.target.revision(), *input.target.certificate_digest())
            );
            assert_eq!(next.source_v1_digest(), None);
            assert!(report.peak_batch_deltas <= batch);
            assert!(report.batches >= 8);
            assert_eq!(
                packed_graph_v1_digest(
                    &mut input.recovery,
                    &mut input.fs,
                    &next,
                    &input.target,
                    limits(2).certificates,
                    export_limits(),
                    4 * 1024 * 1024
                )
                .unwrap(),
                input.new_digest
            );
            assert_eq!(
                packed_graph_v1_digest(
                    &mut input.recovery,
                    &mut input.fs,
                    &input.base,
                    &input.old,
                    limits(2).certificates,
                    export_limits(),
                    4 * 1024 * 1024
                )
                .unwrap(),
                input.old_digest
            );
            assert!(input.base.families() == old_families);
            if let Some(expected) = commitment {
                assert_eq!(next.publication_claims().state_digest, expected);
            } else {
                commitment = Some(next.publication_claims().state_digest);
            }
            input
                .recovery
                .publish_recovered_packed_root(
                    &mut input.fs,
                    GRAPH_PACKED_PROFILE_V1,
                    next.publication_claims(),
                    &next.families(),
                    8,
                )
                .unwrap();
        }
    }
}
