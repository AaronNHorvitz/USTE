//! Bounded authenticated zero/one-revision native history bootstrap.
use super::*;

pub(super) fn install(
    raw: &mut CommitCoordinator<GraphState, Fs, RecoveryEnvelope, OsEntropy, OsEntropy>,
    fs: &mut Fs,
    profile: Bm06Profile,
) -> Result<(), LinuxRunnerError> {
    let key = bootstrap_id(profile, IdempotencyKey::from_bytes);
    let transaction = bootstrap_id(profile, TransactionId::from_bytes);
    if let Some((revision, _)) = raw
        .checkpoint_anchor()
        .map_err(|_| error("USTE_BM06_PACKED_BOOTSTRAP"))?
    {
        let mut outcomes = raw.checkpoint_outcomes();
        let Some((principal, stored, outcome)) = outcomes.next() else {
            return Err(error("USTE_BM06_PACKED_BINDING"));
        };
        if revision.get() != 1
            || outcome.revision != revision
            || principal != PRINCIPAL
            || stored != key
            || outcome.transaction_id != transaction
            || outcomes.next().is_some()
        {
            return Err(error("USTE_BM06_PACKED_BINDING"));
        }
    }
    let outcome = raw
        .commit(
            fs,
            TransactionRequest {
                principal: PRINCIPAL,
                idempotency_key: key,
                transaction_id: transaction,
                canonical_request: &policy_bytes()?,
                blob_inventory: None,
            },
            &mut SystemClock::new(),
            &NeverCancel,
        )
        .map_err(|_| error("USTE_BM06_PACKED_BOOTSTRAP"))?;
    if outcome.revision.get() != 1 {
        return Err(error("USTE_BM06_PACKED_BINDING"));
    }
    Ok(())
}

pub(super) fn resume(
    recovery: Recovery,
    fs: &mut Fs,
    adapter: &mut PortableRecoveryAdapter,
    profile: Bm06Profile,
    limits: Limits,
    owner_work: &mut OwnerWork,
) -> Result<Recovery, LinuxRunnerError> {
    if recovery
        .authenticated_frontier_anchor()
        .is_some_and(|(revision, _)| revision.get() > 1)
    {
        return Err(error("USTE_BM06_PACKED_BINDING"));
    }
    let mut raw = recovery
        .into_bounded_coordinator(
            fs,
            GraphState::new(scope()),
            retention()?,
            CoordinatorRecoveryLimits::new(1, 0).map_err(|_| error("USTE_BM06_LIMITS"))?,
            1_048_576,
        )
        .map_err(|_| error("USTE_BM06_PACKED_BOOTSTRAP"))?;
    install(&mut raw, fs, profile)?;
    record_owner(
        owner_work,
        OwnerStage::BootstrapResume,
        raw.vault_decrypt_report(),
    )?;
    drop(raw);
    open(fs, adapter, limits).map(|(recovery, _)| recovery)
}
