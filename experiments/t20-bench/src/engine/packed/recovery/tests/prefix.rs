use super::*;
use uste_txn::{CheckpointState, TransactionState};

#[test]
fn packed_bm06_partial_generations_cold_admit_retry_and_match_reference() {
    exercise(false);
}

#[test]
fn packed_bm06_two_batch_tail_recovers_from_retained_checkpoint() {
    exercise(true);
}

fn exercise(tail: bool) {
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
    for (sequence, current, history) in [(2, 512, 512), (3, 513, 513), (4, 513, 1025)]
        .into_iter()
        .take(if tail { 2 } else { 3 })
    {
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
    if tail {
        for generation in [0, 2, VERSIONS, u64::MAX] {
            assert!(
                certify_generation_tail(
                    &mut live,
                    &mut fs,
                    &mut kernel,
                    &principal,
                    profile,
                    generation,
                    limits,
                    &mut clock(5),
                    &mut |_| panic!("invalid generation must precede batch access")
                )
                .is_err()
            );
        }
        let mut narrow = limits;
        narrow.legacy.groups = 4;
        assert!(
            certify_generation_tail(
                &mut live,
                &mut fs,
                &mut kernel,
                &principal,
                profile,
                1,
                narrow,
                &mut clock(5),
                &mut |_| panic!("frontier bound must precede batch access")
            )
            .is_err()
        );
        assert!(
            certify_generation_tail(
                &mut live,
                &mut fs,
                &mut kernel,
                &principal,
                profile,
                1,
                limits,
                &mut clock(5),
                &mut |sequence| {
                    let mut request = batch(profile, sequence)?;
                    request.sequence += 1;
                    Ok(request)
                }
            )
            .is_err()
        );
        assert_eq!(
            live.state()
                .unwrap()
                .current_base()
                .unwrap()
                .anchor()
                .0
                .get(),
            3
        );
        for sequence in 4..=5 {
            let bytes = encode_transaction(&GraphTransaction::new(
                scope(),
                profile.batch(scope(), sequence).unwrap(),
            ))
            .unwrap();
            let prepared = reference
                .prepare(
                    &bytes,
                    None,
                    uste_types::CommitRevision::new(sequence).unwrap(),
                )
                .unwrap();
            reference.publish(prepared);
        }
        let expected = GraphState::logical_state_digest(&reference.snapshot()).unwrap();
        let mut tail_clock = ScriptedClock::new((4..=5).map(|sequence| {
            Ok(ClockObservation {
                wall_utc: UtcInstant::new(sequence, 0).unwrap(),
                monotonic_ticks: sequence as u64,
            })
        }));
        let outcome = certify_generation_tail(
            &mut live,
            &mut fs,
            &mut kernel,
            &principal,
            profile,
            1,
            limits,
            &mut tail_clock,
            &mut |sequence| batch(profile, sequence),
        )
        .unwrap();
        assert_eq!(outcome.revision.get(), 5);
        assert!(uste_storage::Clock::observe(&mut tail_clock).is_err());
        assert!(live.state().unwrap().needs_repair());
        assert!(verify_prefix_history(&live, &mut fs, &kernel, &principal, profile, 5).is_err());
        drop(live);
        let recovery = open(&mut fs, &name, limits, 90_000_000).unwrap();
        assert_eq!(
            crate::engine::packed::prefix::latest_revision(&mut fs, &recovery, limits)
                .unwrap()
                .get(),
            4
        );
        // Intermediate derived roots exist, but this selected-checkpoint recovery must replay both batches.
        let (mut recovered, digest, groups) = admit_at(
            &mut fs,
            recovery,
            limits,
            Some(uste_types::CommitRevision::new(3).unwrap()),
            [513, 513, 0, 0, 0, 0, 1, 1],
        )
        .unwrap();
        assert!(digest.is_none());
        assert_eq!(groups, 2);
        assert_eq!(
            verify_prefix_history(&recovered, &mut fs, &kernel, &principal, profile, 5).unwrap(),
            1026
        );
        for sequence in 4..=5 {
            let retried = commit_batch(
                &mut recovered,
                &mut fs,
                &mut kernel,
                &principal,
                batch(profile, sequence).unwrap(),
                &mut clock(5),
                limits,
            )
            .unwrap()
            .0;
            assert_eq!(retried.revision.get(), sequence);
            if sequence == 5 {
                assert_eq!(retried, outcome);
            }
            assert_eq!(recovered.overlay_counts(), (0, 0));
        }
        drop(recovered);
        let recovery = open(&mut fs, &name, limits, 91_000_000).unwrap();
        let mut selected = limits;
        selected.counts = [513, 1026, 0, 0, 0, 0, 1, 1];
        assert_eq!(admit(&mut fs, recovery, selected).unwrap().1, expected);
        return;
    }
    for revision in [0, 3, 5, profile.frontier() + 1, u64::MAX] {
        assert!(
            verify_prefix_history(&live, &mut fs, &kernel, &principal, profile, revision).is_err()
        );
    }
}
