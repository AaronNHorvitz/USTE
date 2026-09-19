use super::*;
#[path = "packed_authorized_reads.rs"]
mod read;
use uste_graph::{PackedGraphWritePreparationLimits, PackedGraphWritePublicationLimits};
use uste_policy::{
    Action, AuthenticatedPrincipal, NamespaceGrant, PermissionSet, PolicyKernel, PrincipalDigest,
    TrustedPrincipalAdapter,
};
use uste_txn::{
    AuthorizedDiskWriteError, AuthorizedError, AuthorizedPackedWriter, AuthorizedTransactionRequest,
};

struct Identity;
impl TrustedPrincipalAdapter for Identity {
    type Credential = u8;
    fn authenticate(
        &mut self,
        value: &u8,
    ) -> Result<PrincipalDigest, uste_policy::AuthenticationError> {
        Ok(PrincipalDigest::from_bytes([*value; 32]))
    }
}
struct Cancel;
impl uste_txn::Cancellation for Cancel {
    fn is_cancelled(&self) -> bool {
        true
    }
}
fn preparation(bytes: u64) -> PackedGraphWritePreparationLimits {
    PackedGraphWritePreparationLimits {
        proof: PackedGraphPreparationLimits {
            proof: GraphDiskPreparationLimits::new(100, 1000, 100, 100, bytes).unwrap(),
            ..preparation_limits()
        },
        delta: GraphStateDeltaLimits::new(1000, 16 * 1024 * 1024).unwrap(),
        certificates: limits(2).certificates,
    }
}
fn publication(batches: u64) -> PackedGraphWritePublicationLimits {
    PackedGraphWritePublicationLimits {
        stage: PackedGraphStageLimits {
            maximum_batches: batches,
            ..stage_limits(2)
        },
        maximum_attempts: 8,
    }
}
fn request(revision: u8, bytes: &[u8]) -> AuthorizedTransactionRequest<'_> {
    AuthorizedTransactionRequest {
        idempotency_key: IdempotencyKey::from_bytes([revision; 16]),
        transaction_id: TransactionId::from_bytes([revision + 32; 16]),
        canonical_request: bytes,
        blob_inventory: None,
    }
}
fn setup() -> (
    Fs,
    Live,
    PolicyKernel,
    AuthenticatedPrincipal,
    AuthenticatedPrincipal,
) {
    let (mut fs, mut live) = install(parts()).unwrap();
    let quota = QuotaLimits::new(1024 * 1024, 1024 * 1024, 1024 * 1024, 8, 1024).unwrap();
    let mut policy = NamespacePolicy::new(scope(), PolicyVersion::new(2).unwrap(), quota);
    for who in [1, 2] {
        let mut actions = vec![Action::Commit, Action::ReadRecord, Action::ReadOwnOutcome];
        if who == 1 {
            actions.push(Action::ManagePolicy);
        }
        let mut grant = NamespaceGrant::new(PermissionSet::from_actions(actions), quota);
        if who == 2 {
            for id in [2, 4] {
                grant
                    .deny_record(
                        record(id).record(),
                        PermissionSet::from_actions([Action::ReadRecord, Action::Commit]),
                    )
                    .unwrap();
            }
        }
        policy
            .grant(PrincipalDigest::from_bytes([who; 32]), grant)
            .unwrap();
    }
    let tx = GraphTransaction::with_policy_mutation(
        scope(),
        vec![],
        DurablePolicyMutation::Replace {
            expected: PolicyVersion::new(1).unwrap(),
            policy: policy.clone(),
        },
    );
    let outcome = append(&mut fs, &mut live, 4, &tx);
    publish_packed_graph_live_base(&mut live, &mut fs, outcome, stage_limits(2), 8).unwrap();
    live.rebase_metadata(&mut fs, rebase_limits()).unwrap();
    let mut kernel = PolicyKernel::new();
    kernel.install_initial_policy(policy).unwrap();
    let admin = kernel.authenticate(&mut Identity, &1).unwrap();
    let bob = kernel.authenticate(&mut Identity, &2).unwrap();
    (fs, live, kernel, admin, bob)
}

#[test]
fn authorized_packed_graph_denial_targets_quota_and_stale_policy_precede_clock_and_io() {
    let (mut fs, mut live, mut kernel, admin, bob) = setup();
    let foreign_kernel = PolicyKernel::new();
    let foreign = foreign_kernel.authenticate(&mut Identity, &1).unwrap();
    let absent = kernel.authenticate(&mut Identity, &9).unwrap();
    let bytes = encode_transaction(&cases()[0]).unwrap(); // Includes explicitly referenced hidden entity 2.
    fs.arm(
        FaultPlan::new([FaultPoint {
            operation: FsOp::ReadAt,
            occurrence: 1,
            action: FaultAction::Error(uste_storage::AdapterErrorKind::Io),
        }])
        .unwrap(),
    )
    .unwrap();
    let mut writer = AuthorizedPackedWriter::new(
        &mut live,
        &mut kernel,
        preparation(4 * 1024 * 1024),
        publication(10000),
    )
    .unwrap();
    for principal in [&foreign, &absent, &bob] {
        assert_eq!(
            writer.commit(
                &mut fs,
                principal,
                request(5, &bytes),
                &mut ScriptedClock::new([]),
                &NeverCancel
            ),
            Err(AuthorizedDiskWriteError::Authorization(
                AuthorizedError::Unauthorized
            ))
        );
    }
    let policy_bytes = encode_transaction(&cases()[20]).unwrap();
    assert_eq!(
        writer.commit(
            &mut fs,
            &bob,
            request(5, &policy_bytes),
            &mut ScriptedClock::new([]),
            &NeverCancel
        ),
        Err(AuthorizedDiskWriteError::Authorization(
            AuthorizedError::Unauthorized
        ))
    );
    let other = NamespaceRef::new(scope().database(), NamespaceId::from_bytes([99; 16]));
    let cross_scope = encode_transaction(&GraphTransaction::with_policy_mutation(
        other,
        vec![],
        DurablePolicyMutation::Install {
            policy: NamespacePolicy::new(
                other,
                PolicyVersion::new(1).unwrap(),
                QuotaLimits::new(100, 100, 100, 1, 1).unwrap(),
            ),
        },
    ))
    .unwrap();
    assert_eq!(
        writer.commit(
            &mut fs,
            &admin,
            request(5, &cross_scope),
            &mut ScriptedClock::new([]),
            &NeverCancel
        ),
        Err(AuthorizedDiskWriteError::Authorization(
            AuthorizedError::Unauthorized
        ))
    );
    let inventory = uste_storage::BlobInventory::new(scope(), []).unwrap();
    assert_eq!(
        writer.commit(
            &mut fs,
            &admin,
            AuthorizedTransactionRequest {
                blob_inventory: Some(&inventory),
                ..request(5, &bytes)
            },
            &mut ScriptedClock::new([]),
            &NeverCancel
        ),
        Err(AuthorizedDiskWriteError::Authorization(
            AuthorizedError::Transaction(TransactionError::InvalidRequest)
        ))
    );
    assert_eq!(
        writer.commit(
            &mut fs,
            &admin,
            request(5, &vec![0; 1024 * 1024 + 1]),
            &mut ScriptedClock::new([]),
            &NeverCancel
        ),
        Err(AuthorizedDiskWriteError::Authorization(
            AuthorizedError::ResourceLimit
        ))
    );
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
    assert_eq!(fs.pending_faults(), 1);
    drop(writer);
    let mut wrong = PolicyKernel::new();
    wrong
        .install_initial_policy(NamespacePolicy::new(
            scope(),
            PolicyVersion::new(2).unwrap(),
            QuotaLimits::new(10, 10, 10, 1, 1).unwrap(),
        ))
        .unwrap();
    assert!(matches!(
        AuthorizedPackedWriter::new(
            &mut live,
            &mut wrong,
            preparation(4 * 1024 * 1024),
            publication(10000)
        ),
        Err(AuthorizedError::InvalidPolicy)
    ));
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
}

#[test]
fn authorized_packed_graph_reference_retry_collision_expiry_cancellation_and_cold_reopen() {
    let (mut fs, mut live, mut kernel, admin, _) = setup();
    let tx = cases()[0].clone();
    let bytes = encode_transaction(&tx).unwrap();
    let expected = prepare(&mut fs, &mut live, tx).result_digest();
    let mut writer = AuthorizedPackedWriter::new(
        &mut live,
        &mut kernel,
        preparation(4 * 1024 * 1024),
        publication(10000),
    )
    .unwrap();
    let outcome = writer
        .commit(
            &mut fs,
            &admin,
            request(5, &bytes),
            &mut clock(5),
            &NeverCancel,
        )
        .unwrap();
    assert_eq!(outcome.result_digest, expected);
    drop(writer);
    let mut writer =
        AuthorizedPackedWriter::new(&mut live, &mut kernel, preparation(1), publication(0))
            .unwrap();
    assert_eq!(
        writer
            .commit(&mut fs, &admin, request(5, &bytes), &mut clock(6), &Cancel)
            .unwrap(),
        outcome
    );
    let collision = AuthorizedTransactionRequest {
        idempotency_key: IdempotencyKey::from_bytes([99; 16]),
        ..request(5, &bytes)
    };
    assert_eq!(
        writer.commit(&mut fs, &admin, collision, &mut clock(6), &NeverCancel),
        Err(AuthorizedDiskWriteError::Authorization(
            AuthorizedError::Transaction(TransactionError::Conflict)
        ))
    );
    assert_eq!(
        writer.commit(&mut fs, &admin, request(6, &bytes), &mut clock(6), &Cancel),
        Err(AuthorizedDiskWriteError::Authorization(
            AuthorizedError::Transaction(TransactionError::Cancelled)
        ))
    );
    assert_eq!(
        writer.commit(
            &mut fs,
            &admin,
            request(5, &bytes),
            &mut clock(outcome.expires_at.seconds() as u64 + 1),
            &NeverCancel
        ),
        Err(AuthorizedDiskWriteError::Authorization(
            AuthorizedError::Transaction(TransactionError::IdempotencyExpired)
        ))
    );
    drop(writer);
    live.rebase_metadata(&mut fs, rebase_limits()).unwrap();
    drop(live);
    let input = reopen(fs, 5, 1_510_000);
    let (mut fs, result) = recover(input, suffix_limits());
    let (mut live, report) = result.unwrap();
    assert!(report.is_none());
    let mut writer =
        AuthorizedPackedWriter::new(&mut live, &mut kernel, preparation(1), publication(0))
            .unwrap();
    assert_eq!(
        writer
            .commit(&mut fs, &admin, request(5, &bytes), &mut clock(6), &Cancel)
            .unwrap(),
        outcome
    );
}

#[test]
fn authorized_packed_graph_hidden_dependency_error_is_content_free() {
    let (mut fs, mut live, mut kernel, _, bob) = setup();
    let bytes = encode_transaction(&GraphTransaction::new(
        scope(),
        vec![Operation::DeleteEntity {
            target: record(1),
            expected: Expected::Version(uste_graph::RecordVersion::FIRST),
            policy: DeletePolicy::Reject,
            affected: vec![],
        }],
    ))
    .unwrap();
    let mut writer = AuthorizedPackedWriter::new(
        &mut live,
        &mut kernel,
        preparation(4 * 1024 * 1024),
        publication(10000),
    )
    .unwrap();
    fs.arm(FaultPlan::default()).unwrap();
    assert_eq!(
        writer.commit(
            &mut fs,
            &bob,
            request(5, &bytes),
            &mut clock(5),
            &NeverCancel
        ),
        Err(AuthorizedDiskWriteError::Preparation(
            TransactionError::Conflict
        ))
    );
    assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
}

#[test]
fn authorized_packed_graph_certified_revocation_precedes_failed_repair_and_older_retry() {
    let (mut fs, mut live, mut kernel, admin, bob) = setup();
    let old_bytes = encode_transaction(&cases()[0]).unwrap();
    let old = AuthorizedPackedWriter::new(
        &mut live,
        &mut kernel,
        preparation(4 * 1024 * 1024),
        publication(10000),
    )
    .unwrap()
    .commit(
        &mut fs,
        &admin,
        request(5, &old_bytes),
        &mut clock(5),
        &NeverCancel,
    )
    .unwrap();
    let previous = kernel.namespace_policy(scope()).unwrap();
    let mut next = NamespacePolicy::new(scope(), PolicyVersion::new(3).unwrap(), previous.quotas());
    for (who, grant) in previous.grants() {
        if who != bob.digest() {
            next.grant(who, grant.clone()).unwrap();
        }
    }
    let bytes = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        vec![],
        DurablePolicyMutation::Replace {
            expected: PolicyVersion::new(2).unwrap(),
            policy: next.clone(),
        },
    ))
    .unwrap();
    let error = AuthorizedPackedWriter::new(
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
    )
    .unwrap_err();
    let AuthorizedDiskWriteError::CommittedPublication { outcome, .. } = error else {
        panic!("certified publication failure required");
    };
    assert_eq!(outcome.revision.get(), 6);
    assert_eq!(kernel.namespace_policy(scope()), Some(&next));
    assert!(live.state().unwrap().needs_repair());
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
        AuthorizedPackedWriter::new(&mut live, &mut kernel, preparation(1), publication(0))
            .unwrap()
            .commit(
                &mut fs,
                &bob,
                request(7, &old_bytes),
                &mut ScriptedClock::new([]),
                &NeverCancel
            ),
        Err(AuthorizedDiskWriteError::Authorization(
            AuthorizedError::Unauthorized
        ))
    );
    assert_eq!(fs.operation_count(FsOp::ReadAt), 0);
    assert_eq!(fs.pending_faults(), 1);
    assert_eq!(
        AuthorizedPackedWriter::new(&mut live, &mut kernel, preparation(1), publication(0))
            .unwrap()
            .commit(
                &mut fs,
                &admin,
                request(5, &old_bytes),
                &mut clock(7),
                &Cancel
            )
            .unwrap(),
        old
    );
    assert!(live.state().unwrap().needs_repair());
    assert!(
        matches!(AuthorizedPackedWriter::new(&mut live, &mut kernel, preparation(1), publication(0)).unwrap()
        .commit(&mut fs, &admin, request(6, &bytes), &mut clock(7), &Cancel),
        Err(AuthorizedDiskWriteError::CommittedPublication { outcome: same, .. }) if same == outcome)
    );
    // Eligible old retries and admission-only repair refusal have not consumed the armed read.
    assert_eq!(fs.pending_faults(), 1);
    assert!(
        matches!(AuthorizedPackedWriter::new(&mut live, &mut kernel, preparation(1), publication(10000)).unwrap()
        .commit(&mut fs, &admin, request(6, &bytes), &mut clock(7), &Cancel),
        Err(AuthorizedDiskWriteError::CommittedPublication { outcome: same, .. }) if same == outcome)
    );
    assert_eq!(fs.pending_faults(), 0);
    assert_eq!(
        AuthorizedPackedWriter::new(&mut live, &mut kernel, preparation(1), publication(10000))
            .unwrap()
            .commit(&mut fs, &admin, request(6, &bytes), &mut clock(7), &Cancel)
            .unwrap(),
        outcome
    );
    assert!(!live.state().unwrap().needs_repair());
    live.rebase_metadata(&mut fs, rebase_limits()).unwrap();
    drop(live);
    let input = reopen(fs, 6, 1_530_000);
    let (mut fs, result) = recover(input, suffix_limits());
    let (mut live, _) = result.unwrap();
    assert_eq!(
        live.state().unwrap().current_durable_policy().unwrap(),
        &next
    );
    assert_eq!(
        AuthorizedPackedWriter::new(&mut live, &mut kernel, preparation(1), publication(0))
            .unwrap()
            .commit(&mut fs, &admin, request(6, &bytes), &mut clock(7), &Cancel)
            .unwrap(),
        outcome
    );
}

#[test]
fn authorized_packed_graph_precommit_io_failure_and_uncertainty_preserve_outcome_boundary() {
    for uncertain in [false, true] {
        let (mut fs, mut live, mut kernel, admin, _) = setup();
        let bytes = encode_transaction(&cases()[0]).unwrap();
        let fault = if uncertain {
            FaultPoint {
                operation: FsOp::SyncData,
                occurrence: 2,
                action: FaultAction::CrashAfter,
            }
        } else {
            FaultPoint {
                operation: FsOp::ReadAt,
                occurrence: 1,
                action: FaultAction::Error(uste_storage::AdapterErrorKind::Io),
            }
        };
        fs.arm(FaultPlan::new([fault]).unwrap()).unwrap();
        let error = AuthorizedPackedWriter::new(
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
        .unwrap_err();
        assert_eq!(fs.pending_faults(), 0);
        if uncertain {
            assert_eq!(
                error,
                AuthorizedDiskWriteError::Authorization(AuthorizedError::Transaction(
                    TransactionError::OutcomeUnknown
                ))
            );
            assert!(matches!(
                live.state(),
                Err(TransactionError::OutcomeUnknown)
            ));
            assert!(
                AuthorizedPackedWriter::new(&mut live, &mut kernel, preparation(1), publication(0))
                    .is_err()
            );
            drop(live);
            let input = reopen(fs, 4, 1_550_000);
            let (mut fs, result) = recover(input, suffix_limits());
            let (mut live, report) = result.unwrap();
            assert_eq!(report.unwrap().journal.groups, 1);
            let outcome =
                AuthorizedPackedWriter::new(&mut live, &mut kernel, preparation(1), publication(0))
                    .unwrap()
                    .commit(&mut fs, &admin, request(5, &bytes), &mut clock(6), &Cancel)
                    .unwrap();
            assert_eq!(outcome.revision.get(), 5);
        } else {
            assert!(matches!(
                error,
                AuthorizedDiskWriteError::Authorization(_)
                    | AuthorizedDiskWriteError::Preparation(_)
            ));
            assert_eq!(live.state().unwrap().revision().get(), 4);
            assert_eq!(live.overlay_counts(), (0, 0));
            assert_eq!(fs.operation_count(FsOp::WriteAt), 0);
        }
    }
}
