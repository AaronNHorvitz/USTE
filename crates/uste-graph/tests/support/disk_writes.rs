use super::*;
use uste_policy::AuthenticatedPrincipal;
use uste_txn::{
    AuthorizedDiskWriteError, AuthorizedDiskWriter, AuthorizedTransactionRequest, TransactionState,
};

type Fs = FaultFileSystem<MemoryFileSystem>;
type Disk = uste_txn::DiskCommitCoordinator<
    GraphDiskLiveState,
    Fs,
    TestEnvelope,
    CounterEntropy,
    CounterEntropy,
>;

fn preparation(bytes: u64) -> uste_graph::GraphDiskWritePreparationLimits {
    uste_graph::GraphDiskWritePreparationLimits {
        proof: GraphDiskPreparationLimits::new(100, 100, 100, 100, bytes).unwrap(),
        delta: GraphStateDeltaLimits::new(1000, 1024 * 1024).unwrap(),
    }
}

fn publication(bytes: u64) -> GraphStateRootMergeLimits {
    GraphStateRootMergeLimits::uniform(
        IndexRunMergeLimits::new(
            IndexRunReadLimits::new(100, 1000, 1024 * 1024).unwrap(),
            1000,
            1024 * 1024,
            1000,
            1024 * 1024,
        )
        .unwrap(),
        bytes,
    )
    .unwrap()
}

fn request(id: u8, encoded: &[u8]) -> AuthorizedTransactionRequest<'_> {
    AuthorizedTransactionRequest {
        idempotency_key: IdempotencyKey::from_bytes([id; 16]),
        transaction_id: TransactionId::from_bytes([id + 32; 16]),
        canonical_request: encoded,
        blob_inventory: None,
    }
}

struct Cancel;
impl uste_txn::Cancellation for Cancel {
    fn is_cancelled(&self) -> bool {
        true
    }
}

pub(super) fn verify(
    disk: &mut Disk,
    filesystem: &mut Fs,
    kernel: &mut PolicyKernel,
    admin: &AuthenticatedPrincipal,
    bob: &AuthenticatedPrincipal,
    reference: &mut GraphState,
) {
    let transaction = GraphTransaction::new(
        scope(),
        vec![Operation::ReplaceEntity {
            target: record(1),
            expected: Expected::Version(uste_graph::RecordVersion::FIRST),
            properties: Value::Bool(true),
        }],
    );
    let encoded = encode_transaction(&transaction).unwrap();
    let denied = encode_transaction(&GraphTransaction::new(
        scope(),
        vec![Operation::ReplaceEntity {
            target: record(3),
            expected: Expected::Version(uste_graph::RecordVersion::FIRST),
            properties: Value::Null,
        }],
    ))
    .unwrap();
    filesystem
        .arm(
            FaultPlan::new([FaultPoint {
                operation: FaultOperation::ReadAt,
                occurrence: 1,
                action: FaultAction::Error(uste_storage::AdapterErrorKind::Io),
            }])
            .unwrap(),
        )
        .unwrap();
    {
        let mut writer = AuthorizedDiskWriter::new(
            disk,
            kernel,
            preparation(1024 * 1024),
            publication(2 * 1024 * 1024),
        )
        .unwrap();
        let foreign_kernel = PolicyKernel::new();
        let foreign = foreign_kernel.authenticate(&mut Identity, &1).unwrap();
        assert!(matches!(
            writer.commit(
                filesystem,
                &foreign,
                request(3, &encoded),
                &mut ScriptedClock::new([]),
                &NeverCancel
            ),
            Err(AuthorizedDiskWriteError::Authorization(
                uste_txn::AuthorizedError::Unauthorized
            ))
        ));
        let oversized = vec![0; 1024 * 1024 + 1];
        assert!(matches!(
            writer.commit(
                filesystem,
                admin,
                request(3, &oversized),
                &mut ScriptedClock::new([]),
                &NeverCancel
            ),
            Err(AuthorizedDiskWriteError::Authorization(
                uste_txn::AuthorizedError::ResourceLimit
            ))
        ));
        assert!(matches!(
            writer.commit(
                filesystem,
                bob,
                request(3, &denied),
                &mut ScriptedClock::new([]),
                &NeverCancel
            ),
            Err(AuthorizedDiskWriteError::Authorization(
                uste_txn::AuthorizedError::Unauthorized
            ))
        ));
        assert_eq!(filesystem.operation_count(FaultOperation::ReadAt), 0);
        assert_eq!(filesystem.pending_faults(), 1);
        assert!(
            writer
                .commit(
                    filesystem,
                    admin,
                    request(3, &encoded),
                    &mut clock(3),
                    &NeverCancel
                )
                .is_err()
        );
        assert_eq!(filesystem.pending_faults(), 0);
    }
    assert_eq!(disk.state().unwrap().revision().get(), 2);
    assert_eq!(disk.overlay_counts(), (0, 0));
    let prepared = reference
        .prepare(&encoded, None, CommitRevision::new(3).unwrap())
        .unwrap();
    let expected_digest = GraphState::result_digest(&prepared);
    let outcome = {
        let mut writer = AuthorizedDiskWriter::new(
            disk,
            kernel,
            preparation(1024 * 1024),
            publication(2 * 1024 * 1024),
        )
        .unwrap();
        // This clock has exactly one sample for preflight plus actual commit.
        writer
            .commit(
                filesystem,
                admin,
                request(3, &encoded),
                &mut clock(3),
                &NeverCancel,
            )
            .unwrap()
    };
    assert_eq!(outcome.result_digest, expected_digest);
    reference.publish(prepared);
    assert!(!disk.state().unwrap().is_pending());
    assert_eq!(disk.overlay_counts(), (1, 0));
    {
        let mut writer =
            AuthorizedDiskWriter::new(disk, kernel, preparation(1), publication(2 * 1024 * 1024))
                .unwrap();
        assert_eq!(
            writer
                .commit(
                    filesystem,
                    admin,
                    request(3, &encoded),
                    &mut clock(4),
                    &Cancel
                )
                .unwrap(),
            outcome
        );
        let conflicting = AuthorizedTransactionRequest {
            idempotency_key: IdempotencyKey::from_bytes([99; 16]),
            ..request(3, &encoded)
        };
        assert!(matches!(
            writer.commit(filesystem, admin, conflicting, &mut clock(4), &NeverCancel),
            Err(AuthorizedDiskWriteError::Authorization(
                uste_txn::AuthorizedError::Transaction(uste_txn::TransactionError::Conflict)
            ))
        ));
        assert!(matches!(
            writer.commit(
                filesystem,
                admin,
                request(3, &encoded),
                &mut clock(10_000_000),
                &NeverCancel
            ),
            Err(AuthorizedDiskWriteError::Authorization(
                uste_txn::AuthorizedError::Transaction(
                    uste_txn::TransactionError::IdempotencyExpired
                )
            ))
        ));
        assert!(matches!(
            writer.commit(
                filesystem,
                admin,
                request(4, &encoded),
                &mut clock(4),
                &NeverCancel
            ),
            Err(AuthorizedDiskWriteError::Preparation(_))
        ));
    }
    assert_eq!(disk.overlay_counts(), (1, 0));
    let previous_policy = kernel.namespace_policy(scope()).unwrap().clone();
    let deletion = encode_transaction(&GraphTransaction::new(
        scope(),
        vec![Operation::DeleteEntity {
            target: record(1),
            expected: Expected::Version(uste_graph::RecordVersion::new(2).unwrap()),
            policy: DeletePolicy::Reject,
            affected: vec![],
        }],
    ))
    .unwrap();
    {
        let mut writer = AuthorizedDiskWriter::new(
            disk,
            kernel,
            preparation(1024 * 1024),
            publication(2 * 1024 * 1024),
        )
        .unwrap();
        assert_eq!(
            writer.commit(
                filesystem,
                bob,
                request(4, &deletion),
                &mut clock(4),
                &NeverCancel
            ),
            Err(AuthorizedDiskWriteError::Preparation(
                uste_txn::TransactionError::Conflict
            ))
        );
    }
    let mut next = NamespacePolicy::new(
        scope(),
        PolicyVersion::new(2).unwrap(),
        previous_policy.quotas(),
    );
    for (principal, grant) in previous_policy.grants() {
        if principal != bob.digest() {
            next.grant(principal, grant.clone()).unwrap();
        }
    }
    let policy_change = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        vec![],
        DurablePolicyMutation::Replace {
            expected: PolicyVersion::new(1).unwrap(),
            policy: next.clone(),
        },
    ))
    .unwrap();
    let prepared = reference
        .prepare(&policy_change, None, CommitRevision::new(4).unwrap())
        .unwrap();
    let certified = {
        let mut writer =
            AuthorizedDiskWriter::new(disk, kernel, preparation(1024 * 1024), publication(1))
                .unwrap();
        let error = writer
            .commit(
                filesystem,
                admin,
                request(4, &policy_change),
                &mut clock(4),
                &NeverCancel,
            )
            .unwrap_err();
        let AuthorizedDiskWriteError::CommittedPublication { outcome, .. } = error else {
            panic!("expected certified repair failure: {error:?}")
        };
        assert!(matches!(
            writer.commit(
                filesystem,
                bob,
                request(5, &encoded),
                &mut ScriptedClock::new([]),
                &NeverCancel
            ),
            Err(AuthorizedDiskWriteError::Authorization(
                uste_txn::AuthorizedError::Unauthorized
            ))
        ));
        outcome
    };
    assert_eq!(
        certified.result_digest,
        GraphState::result_digest(&prepared)
    );
    reference.publish(prepared);
    assert_eq!(kernel.namespace_policy(scope()), Some(&next));
    assert!(disk.state().unwrap().is_pending());
    assert!(matches!(
        uste_txn::AuthorizedDiskMetadata::new(disk, kernel),
        Err(uste_txn::AuthorizedError::InvalidPolicy)
    ));
    filesystem
        .arm(
            FaultPlan::new([FaultPoint {
                operation: FaultOperation::CreateNew,
                occurrence: 1,
                action: FaultAction::Error(uste_storage::AdapterErrorKind::Io),
            }])
            .unwrap(),
        )
        .unwrap();
    {
        let mut writer =
            AuthorizedDiskWriter::new(disk, kernel, preparation(1), publication(2 * 1024 * 1024))
                .unwrap();
        assert!(
            matches!(writer.commit(filesystem, admin, request(4, &policy_change), &mut clock(5), &Cancel),
            Err(AuthorizedDiskWriteError::CommittedPublication { outcome, .. }) if outcome == certified)
        );
        assert_eq!(filesystem.pending_faults(), 0);
        assert_eq!(
            writer
                .commit(
                    filesystem,
                    admin,
                    request(4, &policy_change),
                    &mut clock(5),
                    &Cancel
                )
                .unwrap(),
            certified
        );
    }
    assert!(!disk.state().unwrap().is_pending());
    assert_eq!(disk.state().unwrap().revision().get(), 4);
    let roots = disk
        .load_index_root_manifests(filesystem, GRAPH_STATE_PROFILE_V1)
        .unwrap();
    let current = roots
        .iter()
        .find(|root| root.anchor() == disk.state().unwrap().current_base().unwrap().anchor())
        .unwrap();
    assert_eq!(
        current.logical_state_digest(),
        &GraphState::logical_state_digest(&reference.snapshot()).unwrap()
    );
    let uncertain = encode_transaction(&GraphTransaction::new(
        scope(),
        vec![Operation::ReplaceEntity {
            target: record(1),
            expected: Expected::Version(uste_graph::RecordVersion::new(2).unwrap()),
            properties: Value::Bool(false),
        }],
    ))
    .unwrap();
    filesystem
        .arm(
            FaultPlan::new([FaultPoint {
                operation: FaultOperation::SyncData,
                occurrence: 2,
                action: FaultAction::Error(uste_storage::AdapterErrorKind::Io),
            }])
            .unwrap(),
        )
        .unwrap();
    let mut writer = AuthorizedDiskWriter::new(
        disk,
        kernel,
        preparation(1024 * 1024),
        publication(2 * 1024 * 1024),
    )
    .unwrap();
    assert!(matches!(
        writer.commit(
            filesystem,
            admin,
            request(5, &uncertain),
            &mut clock(6),
            &NeverCancel
        ),
        Err(AuthorizedDiskWriteError::Authorization(
            uste_txn::AuthorizedError::Transaction(uste_txn::TransactionError::OutcomeUnknown)
        ))
    ));
    assert_eq!(filesystem.pending_faults(), 0);
    assert!(matches!(
        writer.commit(
            filesystem,
            admin,
            request(4, &policy_change),
            &mut ScriptedClock::new([]),
            &NeverCancel
        ),
        Err(AuthorizedDiskWriteError::Authorization(
            uste_txn::AuthorizedError::Transaction(uste_txn::TransactionError::OutcomeUnknown)
        ))
    ));
}
