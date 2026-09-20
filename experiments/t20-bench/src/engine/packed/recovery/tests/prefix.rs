use super::*;
use uste_txn::{CheckpointState, TransactionState};

#[test]
fn packed_bm06_partial_generations_cold_admit_retry_and_match_reference() {
    // A bounded partial fixture, not the capped 100-generation CLI or a qualification run.
    let profile = Bm06Profile::new(513).unwrap();
    let limits = Limits::recovery(profile).unwrap();
    let policy = crate::engine::recovery::recovery_policy().unwrap();
    let mut fs = MemoryFileSystem::new(512 * 1024 * 1024);
    let name = EntryName::new("bm06-packed-partial").unwrap();
    let vault = KeyVault::create(
        scope().database(),
        &mut TestKeyAdapter,
        CounterEntropy(80_000_001),
    )
    .unwrap();
    let mut raw = CommitCoordinator::create(
        &mut fs,
        scope(),
        RetentionDays::new(30).unwrap(),
        name.clone(),
        vault,
        CounterEntropy(81_000_001),
        GraphState::new(scope()),
    )
    .unwrap();
    let encoded = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        vec![],
        uste_graph::DurablePolicyMutation::Install {
            policy: policy.clone(),
        },
    ))
    .unwrap();
    raw.commit(
        &mut fs,
        TransactionRequest {
            principal: PRINCIPAL,
            idempotency_key: identity(1, IdempotencyKey::from_bytes),
            transaction_id: identity(1, TransactionId::from_bytes),
            canonical_request: &encoded,
            blob_inventory: None,
        },
        &mut clock(1),
        &NeverCancel,
    )
    .unwrap();
    let mut reference = GraphState::new(scope());
    let prepared = reference
        .prepare(&encoded, None, uste_types::CommitRevision::FIRST)
        .unwrap();
    reference.publish(prepared);
    drop(raw);
    let recovery = open(&mut fs, &name, limits, 82_000_000).unwrap();
    let (mut live, _) = recover_packed_graph_origin(
        recovery,
        &mut fs,
        RetentionDays::new(30).unwrap(),
        CoordinatorRecoveryLimits::new(1, 0).unwrap(),
        limits.origin,
    )
    .unwrap();
    let mut kernel = kernel(policy).unwrap();
    let principal = kernel.authenticate(&mut AuthAdapter, &()).unwrap();
    assert_eq!(
        verify_prefix_history(&live, &mut fs, &kernel, &principal, profile, 1).unwrap(),
        0
    );
    for (sequence, current, history) in [(2, 512, 512), (3, 513, 513), (4, 513, 1025)] {
        let request = batch(profile, sequence).unwrap();
        let bytes = encode_transaction(&GraphTransaction::new(scope(), request.operations.clone()))
            .unwrap();
        let revision = uste_types::CommitRevision::new(sequence).unwrap();
        let prepared = reference.prepare(&bytes, None, revision).unwrap();
        reference.publish(prepared);
        let expected_digest = GraphState::logical_state_digest(&reference.snapshot()).unwrap();
        let outcome = commit_batch(
            &mut live,
            &mut fs,
            &mut kernel,
            &principal,
            request,
            &mut clock(sequence),
            limits,
        )
        .unwrap()
        .0;
        assert_eq!(outcome.revision, revision);
        assert_eq!(
            verify_prefix_history(&live, &mut fs, &kernel, &principal, profile, sequence).unwrap(),
            history
        );
        drop(live);
        let recovery = open(&mut fs, &name, limits, 84_000_000 + sequence * 1_000_000).unwrap();
        let mut selected = limits;
        selected.counts = [current, history, 0, 0, 0, 0, 1, 1];
        let (reopened, digest) = admit(&mut fs, recovery, selected).unwrap();
        assert_eq!(digest, expected_digest);
        live = reopened;
        assert_eq!(
            verify_prefix_history(&live, &mut fs, &kernel, &principal, profile, sequence).unwrap(),
            history
        );
        assert_eq!(
            commit_batch(
                &mut live,
                &mut fs,
                &mut kernel,
                &principal,
                batch(profile, sequence).unwrap(),
                &mut clock(sequence),
                limits
            )
            .unwrap()
            .0,
            outcome
        );
        assert_eq!(live.overlay_counts(), (0, 0));
        if sequence == 3 {
            assert_eq!(
                verify_history(&live, &mut fs, &kernel, &principal, profile, 1).unwrap(),
                513
            );
        } else {
            assert!(verify_history(&live, &mut fs, &kernel, &principal, profile, 1).is_err());
        }
    }
    for revision in [0, 3, 5, profile.frontier() + 1, u64::MAX] {
        assert!(
            verify_prefix_history(&live, &mut fs, &kernel, &principal, profile, revision).is_err()
        );
    }
}
