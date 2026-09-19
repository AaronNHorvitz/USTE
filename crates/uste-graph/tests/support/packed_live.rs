use super::*;
#[path = "packed_suffix.rs"]
mod recovery;
use uste_graph::{GraphPackedLiveState, publish_packed_graph_live_base};
use uste_storage::journal::CertifiedPackedRoot;
use uste_txn::{
    AuthorizedDiskPolicyState, COORDINATOR_PACKED_PROFILE_V1, COORDINATOR_PACKED_USAGE_PROFILE_V1,
    CoordinatorRecoveryLimits, PackedCommitCoordinator, PackedCommitLimits,
    PackedCoordinatorLimits, PackedCoordinatorPrefix, PackedMetadataRebaseLimits,
    PackedQuotaPrefix, TransactionError, stage_packed_coordinator_prefix,
    stage_packed_quota_prefix,
};
type Live =
    PackedCommitCoordinator<GraphPackedLiveState, Fs, TestEnvelope, CounterEntropy, CounterEntropy>;
struct Parts {
    fs: Fs,
    recovery: Recovery,
    state: GraphPackedLiveState,
    primary: PackedCoordinatorPrefix,
    quota: PackedQuotaPrefix,
    primary_root: CertifiedPackedRoot,
    quota_root: CertifiedPackedRoot,
}
fn metadata_limits() -> PackedCoordinatorLimits {
    PackedCoordinatorLimits {
        certificates: limits(2).certificates,
        lookup: preparation_limits().lookup,
        batch: limits(2).batch,
        maximum_references: 4,
        maximum_owners: 4,
    }
}
fn rebase_limits() -> PackedMetadataRebaseLimits {
    PackedMetadataRebaseLimits {
        staging: metadata_limits(),
        maximum_groups: 2,
        maximum_encoded_bytes: 2 * 1024 * 1024,
        certificate_window: 2,
        maximum_publication_attempts: 8,
    }
}
fn parts() -> Parts {
    let (memory, name, _) = fixture();
    let mut fs = FaultFileSystem::new(memory, FaultPlan::default());
    let (mut recovery, source, transaction) = open(&mut fs, &name, 1_010_000);
    let (base, _) =
        bridge_graph_base_to_packed(&mut recovery, &mut fs, &source, &transaction, limits(2))
            .unwrap();
    let claims = base.publication_claims();
    let root = recovery
        .publish_recovered_packed_root(
            &mut fs,
            GRAPH_PACKED_PROFILE_V1,
            claims,
            &base.families(),
            8,
        )
        .unwrap();
    let state = GraphPackedLiveState::from_published(
        &recovery
            .packed_indexes_with_io(&mut fs, &transaction, limits(2).certificates)
            .unwrap(),
        base,
        root,
    )
    .unwrap();
    let mut primary = None;
    let mut quota = None;
    let mut cursor = recovery
        .open_transaction_cursor(
            CommitRevision::new(1).unwrap(),
            transaction.revision(),
            3,
            3 * 1024 * 1024,
        )
        .unwrap();
    while let Some(tx) = recovery
        .next_recovered_transaction(&mut fs, &mut cursor)
        .unwrap()
    {
        let next = stage_packed_coordinator_prefix(
            &mut recovery,
            &mut fs,
            primary.as_ref(),
            &tx,
            metadata_limits(),
        )
        .unwrap()
        .0;
        quota = Some(
            stage_packed_quota_prefix(
                &mut recovery,
                &mut fs,
                quota.as_ref(),
                &next,
                &tx,
                metadata_limits(),
            )
            .unwrap()
            .0,
        );
        primary = Some(next);
    }
    recovery.finish_transaction_cursor(cursor).unwrap();
    let primary = primary.unwrap();
    let quota = quota.unwrap();
    let primary_root = recovery
        .publish_recovered_packed_root(
            &mut fs,
            COORDINATOR_PACKED_PROFILE_V1,
            claims,
            &primary.families(),
            8,
        )
        .unwrap();
    let quota_root = recovery
        .publish_recovered_packed_root(
            &mut fs,
            COORDINATOR_PACKED_USAGE_PROFILE_V1,
            claims,
            &quota.families(),
            8,
        )
        .unwrap();
    Parts {
        fs,
        recovery,
        state,
        primary,
        quota,
        primary_root,
        quota_root,
    }
}
fn install(parts: Parts) -> Result<(Fs, Live), TransactionError> {
    let live = PackedCommitCoordinator::from_admitted_prefixes(
        parts.recovery,
        parts.primary,
        parts.quota,
        &parts.primary_root,
        &parts.quota_root,
        parts.state,
        RetentionDays::new(30).unwrap(),
        CoordinatorRecoveryLimits::new(2, 0).unwrap(),
    )?;
    Ok((parts.fs, live))
}
fn request(revision: u8, bytes: &[u8]) -> TransactionRequest<'_> {
    TransactionRequest {
        principal: uste_policy::PrincipalDigest::from_bytes([0x90; 32]),
        idempotency_key: IdempotencyKey::from_bytes([revision; 16]),
        transaction_id: TransactionId::from_bytes([revision + 32; 16]),
        canonical_request: bytes,
        blob_inventory: None,
    }
}
fn commit_limits() -> PackedCommitLimits {
    PackedCommitLimits {
        lookup: preparation_limits().lookup,
        storage: None,
    }
}
fn prepare(fs: &mut Fs, live: &mut Live, tx: GraphTransaction) -> PackedGraphDelta {
    let mut borrow = live.reducer_and_index_maintenance().unwrap();
    let base = borrow.reducer.current_base().unwrap();
    let maintenance = borrow
        .indexes
        .packed_indexes(fs, limits(2).certificates)
        .unwrap();
    let prepared =
        prepare_packed_graph_transaction(&maintenance, fs, base, tx, preparation_limits())
            .unwrap()
            .0;
    prepare_packed_graph_delta(
        prepared,
        GraphStateDeltaLimits::new(1000, 16 * 1024 * 1024).unwrap(),
    )
    .unwrap()
}
fn append(fs: &mut Fs, live: &mut Live, revision: u8, tx: &GraphTransaction) -> TransactionOutcome {
    let plan = prepare(fs, live, tx.clone());
    let bytes = encode_transaction(tx).unwrap();
    live.commit_prepared(
        fs,
        request(revision, &bytes),
        plan,
        &mut clock(revision.into()),
        &NeverCancel,
        commit_limits(),
        &mut PageCache::new(64 * 1024).unwrap(),
    )
    .unwrap()
}
fn retry(fs: &mut Fs, live: &mut Live, revision: u8, tx: &GraphTransaction) -> TransactionOutcome {
    let bytes = encode_transaction(tx).unwrap();
    live.commit(
        fs,
        request(revision, &bytes),
        &mut clock(20),
        &NeverCancel,
        commit_limits(),
        &mut PageCache::new(64 * 1024).unwrap(),
    )
    .unwrap()
}

#[test]
fn packed_live_graph_pending_repair_exact_retries_and_repeated_metadata_rebase() {
    let (mut fs, mut live) = install(parts()).unwrap();
    let (mut model_fs, name, _) = fixture();
    let (mut model, _) = CommitCoordinator::open(
        &mut model_fs,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(1_050_000),
        CounterEntropy(1_060_000),
        &mut TestKeyAdapter,
        GraphState::new(scope()),
    )
    .unwrap();
    let mut prior = Vec::new();
    for revision in 4..=9 {
        let tx = GraphTransaction::new(
            scope(),
            vec![Operation::Create {
                expected: Expected::Absent,
                record: NewRecord::Entity(NewEntity {
                    id: record(revision + 10),
                    entity_type: text("bounded-live"),
                    schema_version: 1,
                    properties: Value::RecordRef(record(1)),
                }),
            }],
        );
        let old = live.state().unwrap().snapshot();
        assert!(old.ordered_state_digest.is_some());
        let outcome = append(&mut fs, &mut live, revision, &tx);
        assert_eq!(
            outcome,
            commit(&mut model, &mut model_fs, revision, tx.clone())
        );
        let state = live.state().unwrap();
        assert!(state.needs_repair());
        assert!(state.current_base().is_none());
        assert!(state.current_durable_policy().is_err());
        assert!(state.snapshot().ordered_state_digest.is_none());
        assert_eq!(state.revision(), outcome.revision);
        assert_eq!(retry(&mut fs, &mut live, revision, &tx), outcome);
        assert!(live.rebase_metadata(&mut fs, rebase_limits()).is_err());
        assert!(live.rebase_required());
        if let Some((_, old_outcome)) = prior.last() {
            assert!(
                publish_packed_graph_live_base(
                    &mut live,
                    &mut fs,
                    *old_outcome,
                    stage_limits(2),
                    8
                )
                .is_err()
            );
        }
        let mut tiny = stage_limits(2);
        tiny.maximum_batches = 0;
        fs.arm(FaultPlan::default()).unwrap();
        assert!(publish_packed_graph_live_base(&mut live, &mut fs, outcome, tiny, 8).is_err());
        assert_eq!(fs.operation_count(FsOp::CreateNew), 0);
        assert!(live.state().unwrap().needs_repair());
        assert_eq!(retry(&mut fs, &mut live, revision, &tx), outcome);
        assert!(
            publish_packed_graph_live_base(&mut live, &mut fs, outcome, stage_limits(2), 8)
                .unwrap()
                .is_some()
        );
        assert!(live.state().unwrap().current_durable_policy().is_ok());
        assert_ne!(
            live.state().unwrap().snapshot().ordered_state_digest,
            old.ordered_state_digest
        );
        assert!(
            publish_packed_graph_live_base(&mut live, &mut fs, outcome, stage_limits(2), 8)
                .unwrap()
                .is_none()
        );
        let report = live
            .rebase_metadata(&mut fs, rebase_limits())
            .unwrap()
            .unwrap();
        assert_eq!(report.journal.groups, 1);
        assert_eq!(live.overlay_counts(), (0, 0));
        assert_eq!(
            report.primary_root.manifest().claims().state_digest,
            live.state()
                .unwrap()
                .snapshot()
                .ordered_state_digest
                .unwrap()
        );
        prior.push((tx, outcome));
    }
    for (tx, outcome) in prior {
        assert_eq!(
            retry(&mut fs, &mut live, outcome.revision.get() as u8, &tx),
            outcome
        );
    }
}

#[test]
fn packed_live_graph_owner_binding_rechecked_at_coordinator_install() {
    let mut first = parts();
    let mut second = parts();
    std::mem::swap(&mut first.state, &mut second.state);
    // Same synthetic contents, profiles, revision and digests; different live ownership proofs.
    assert!(install(first).is_err());
    assert!(install(second).is_err());
}

#[test]
fn packed_live_graph_every_repair_fault_preserves_certified_outcome_and_cold_replay() {
    let tx = cases()[0].clone();
    let (mut model_fs, name, _) = fixture();
    let (mut model, _) = CommitCoordinator::open(
        &mut model_fs,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(1_070_000),
        CounterEntropy(1_080_000),
        &mut TestKeyAdapter,
        GraphState::new(scope()),
    )
    .unwrap();
    let expected = commit(&mut model, &mut model_fs, 4, tx.clone());
    let expected_digest =
        GraphState::logical_state_digest(model.read_view().unwrap().state()).unwrap();
    let (mut observed_fs, mut observed) = install(parts()).unwrap();
    let outcome = append(&mut observed_fs, &mut observed, 4, &tx);
    observed_fs.arm(FaultPlan::default()).unwrap();
    publish_packed_graph_live_base(&mut observed, &mut observed_fs, outcome, stage_limits(2), 8)
        .unwrap();
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
    .map(|op| (op, observed_fs.operation_count(op)));
    let mut checked = 0;
    for (operation, count) in counts {
        for occurrence in 1..=count {
            for action in [
                FaultAction::Error(uste_storage::AdapterErrorKind::Io),
                FaultAction::CrashBefore,
                FaultAction::CrashAfter,
            ] {
                let (mut fs, mut live) = install(parts()).unwrap();
                let outcome = append(&mut fs, &mut live, 4, &tx);
                assert_eq!(outcome, expected);
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
                    publish_packed_graph_live_base(&mut live, &mut fs, outcome, stage_limits(2), 8)
                        .is_err(),
                    "{operation:?} {occurrence} {action:?}"
                );
                assert_eq!(fs.pending_faults(), 0);
                let pending = live.state().unwrap();
                assert!(pending.needs_repair());
                assert_eq!(pending.revision(), outcome.revision);
                assert!(pending.current_base().is_none());
                assert!(pending.current_durable_policy().is_err());
                // An ordinary I/O failure permits repair in place, without another journal append.
                if matches!(action, FaultAction::Error(_)) {
                    fs.arm(FaultPlan::default()).unwrap();
                    assert_eq!(retry(&mut fs, &mut live, 4, &tx), outcome);
                    publish_packed_graph_live_base(&mut live, &mut fs, outcome, stage_limits(2), 8)
                        .unwrap();
                    assert!(!live.state().unwrap().needs_repair());
                }
                drop(live);
                fs.restart().unwrap();
                fs.arm(FaultPlan::default()).unwrap();
                // Independent full reducer is a test oracle, not a claimed packed recovery path.
                let (mut replay, _) = CommitCoordinator::open(
                    &mut fs,
                    &name,
                    scope(),
                    RetentionDays::new(30).unwrap(),
                    CounterEntropy(1_090_000),
                    CounterEntropy(1_100_000),
                    &mut TestKeyAdapter,
                    GraphState::new(scope()),
                )
                .unwrap();
                assert_eq!(
                    GraphState::logical_state_digest(replay.read_view().unwrap().state()).unwrap(),
                    expected_digest
                );
                assert_eq!(commit(&mut replay, &mut fs, 4, tx.clone()), outcome);
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 171);
}

#[test]
fn packed_live_graph_uncertain_commit_quarantines_reads_and_maintenance() {
    let (mut fs, mut live) = install(parts()).unwrap();
    let tx = cases()[0].clone();
    let plan = prepare(&mut fs, &mut live, tx.clone());
    fs.arm(
        FaultPlan::new([FaultPoint {
            operation: FsOp::SyncData,
            occurrence: 2,
            action: FaultAction::CrashAfter,
        }])
        .unwrap(),
    )
    .unwrap();
    let bytes = encode_transaction(&tx).unwrap();
    assert_eq!(
        live.commit_prepared(
            &mut fs,
            request(4, &bytes),
            plan,
            &mut clock(4),
            &NeverCancel,
            commit_limits(),
            &mut PageCache::new(64 * 1024).unwrap()
        ),
        Err(TransactionError::OutcomeUnknown)
    );
    assert_eq!(fs.pending_faults(), 0);
    assert!(matches!(
        live.state(),
        Err(TransactionError::OutcomeUnknown)
    ));
    assert!(matches!(
        live.reducer_and_index_maintenance(),
        Err(TransactionError::OutcomeUnknown)
    ));
    assert!(matches!(
        live.rebase_metadata(&mut fs, rebase_limits()),
        Err(TransactionError::OutcomeUnknown)
    ));
    drop(live);
    fs.restart().unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    let name = EntryName::new("packed-graph-bridge").unwrap();
    let (mut replay, _) = CommitCoordinator::open(
        &mut fs,
        &name,
        scope(),
        RetentionDays::new(30).unwrap(),
        CounterEntropy(1_110_000),
        CounterEntropy(1_120_000),
        &mut TestKeyAdapter,
        GraphState::new(scope()),
    )
    .unwrap();
    assert_eq!(
        replay.read_view().unwrap().state().revision(),
        Some(CommitRevision::new(4).unwrap())
    );
    assert_eq!(commit(&mut replay, &mut fs, 4, tx).revision.get(), 4);
}

#[test]
fn packed_live_graph_prepared_request_and_stale_base_refuse_without_publication() {
    let (mut fs, mut live) = install(parts()).unwrap();
    let tx = cases()[0].clone();
    let changed = cases()[3].clone();
    let old = live.state().unwrap().snapshot();
    let plan = prepare(&mut fs, &mut live, tx.clone());
    let bytes = encode_transaction(&changed).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert!(
        live.commit_prepared(
            &mut fs,
            request(4, &bytes),
            plan,
            &mut clock(4),
            &NeverCancel,
            commit_limits(),
            &mut PageCache::new(64 * 1024).unwrap()
        )
        .is_err()
    );
    assert_eq!(live.state().unwrap().snapshot(), old);
    assert_eq!(live.overlay_counts(), (0, 0));
    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
    let pending_stale = prepare(&mut fs, &mut live, changed.clone());
    let repaired_stale = prepare(&mut fs, &mut live, changed);
    let outcome = append(&mut fs, &mut live, 4, &tx);
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        live.commit_prepared(
            &mut fs,
            request(5, &bytes),
            pending_stale,
            &mut clock(5),
            &NeverCancel,
            commit_limits(),
            &mut PageCache::new(64 * 1024).unwrap()
        ),
        Err(TransactionError::Conflict)
    );
    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
    assert_eq!(live.overlay_counts(), (1, 0));
    publish_packed_graph_live_base(&mut live, &mut fs, outcome, stage_limits(2), 8).unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        live.commit_prepared(
            &mut fs,
            request(5, &bytes),
            repaired_stale,
            &mut clock(5),
            &NeverCancel,
            commit_limits(),
            &mut PageCache::new(64 * 1024).unwrap()
        ),
        Err(TransactionError::Conflict)
    );
    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
    assert_eq!(live.overlay_counts(), (1, 0));
    assert_eq!(retry(&mut fs, &mut live, 4, &tx), outcome);
}

#[test]
fn packed_live_graph_policy_readiness_changes_only_after_exact_root_repair() {
    let (mut fs, mut live) = install(parts()).unwrap();
    assert_eq!(
        live.state()
            .unwrap()
            .current_durable_policy()
            .unwrap()
            .version(),
        PolicyVersion::new(1).unwrap()
    );
    let tx = cases()[20].clone();
    let outcome = append(&mut fs, &mut live, 4, &tx);
    assert!(live.state().unwrap().current_durable_policy().is_err());
    assert!(
        publish_packed_graph_live_base(&mut live, &mut fs, outcome, stage_limits(2), 0).is_err()
    );
    assert!(live.state().unwrap().current_durable_policy().is_err());
    assert_eq!(retry(&mut fs, &mut live, 4, &tx), outcome);
    publish_packed_graph_live_base(&mut live, &mut fs, outcome, stage_limits(2), 8).unwrap();
    assert_eq!(
        live.state()
            .unwrap()
            .current_durable_policy()
            .unwrap()
            .version(),
        PolicyVersion::new(2).unwrap()
    );
    live.rebase_metadata(&mut fs, rebase_limits()).unwrap();
    assert_eq!(retry(&mut fs, &mut live, 4, &tx), outcome);
}
