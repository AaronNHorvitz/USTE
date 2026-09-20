//! Profile-bound packed bootstrap. No ordinary replay beyond one bounded policy transaction.
use super::*;
mod binding;

pub(super) fn source_binding(
    fs: &mut Fs,
    recovery: &mut Recovery,
    profile: Bm01Profile,
) -> Result<(), LinuxRunnerError> {
    binding::verify(fs, recovery, profile)
}

pub(super) fn identity<T>(profile: Bm01Profile, construct: impl FnOnce([u8; 16]) -> T) -> T {
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(b"BM01PK1\0");
    bytes[8..].copy_from_slice(&profile.entities().to_be_bytes());
    construct(bytes)
}

pub(super) fn install(
    raw: &mut CommitCoordinator<GraphState, Fs, RecoveryEnvelope, OsEntropy, OsEntropy>,
    fs: &mut Fs,
    profile: Bm01Profile,
) -> Result<(), LinuxRunnerError> {
    let key = identity(profile, IdempotencyKey::from_bytes);
    let transaction = identity(profile, TransactionId::from_bytes);
    if let Some((revision, _)) = raw
        .checkpoint_anchor()
        .map_err(|_| error("USTE_BM01_PACKED_BOOTSTRAP_PROFILE"))?
    {
        let mut outcomes = raw.checkpoint_outcomes();
        let Some((principal, stored_key, outcome)) = outcomes.next() else {
            return Err(error("USTE_BM01_PACKED_BOOTSTRAP_PROFILE"));
        };
        if revision.get() != 1
            || outcome.revision != revision
            || principal != PRINCIPAL
            || stored_key != key
            || outcome.transaction_id != transaction
            || outcomes.next().is_some()
        {
            return Err(error("USTE_BM01_PACKED_BOOTSTRAP_PROFILE"));
        }
    }
    let bytes = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        Vec::new(),
        uste_graph::DurablePolicyMutation::Install {
            policy: benchmark_policy(scope()).map_err(|_| error("USTE_BM01_POLICY_PROFILE"))?,
        },
    ))
    .map_err(|_| error("USTE_BM01_POLICY_ENCODE"))?;
    let outcome = raw
        .commit(
            fs,
            TransactionRequest {
                principal: PRINCIPAL,
                idempotency_key: key,
                transaction_id: transaction,
                canonical_request: &bytes,
                blob_inventory: None,
            },
            &mut SystemClock::new(),
            &NeverCancel,
        )
        .map_err(|_| error("USTE_BM01_POLICY_COMMIT"))?;
    if outcome.revision.get() != 1 {
        return Err(error("USTE_BM01_PACKED_BOOTSTRAP_PROFILE"));
    }
    Ok(())
}

pub(super) fn resume(
    recovery: Recovery,
    fs: &mut Fs,
    adapter: &mut PortableRecoveryAdapter,
    profile: Bm01Profile,
    limits: Limits,
    owner_work: &mut OwnerWork,
) -> Result<Recovery, LinuxRunnerError> {
    if recovery
        .authenticated_frontier_anchor()
        .is_some_and(|(revision, _)| revision.get() > 1)
    {
        return Err(error("USTE_BM01_PACKED_BOOTSTRAP_PROFILE"));
    }
    let mut raw = recovery
        .into_bounded_coordinator(
            fs,
            GraphState::new(scope()),
            retention()?,
            CoordinatorRecoveryLimits::new(1, 0).map_err(|_| error("USTE_BM01_LIMITS"))?,
            1_048_576,
        )
        .map_err(|_| error("USTE_BM01_PACKED_BOOTSTRAP_RECOVERY"))?;
    install(&mut raw, fs, profile)?;
    owner_work.record(
        OwnerStage::BootstrapResume,
        raw.vault_decrypt_report()
            .map_err(|_| error("USTE_BM01_CRYPTO_COUNTER"))?,
        raw.vault_encrypt_report()
            .map_err(|_| error("USTE_BM01_CRYPTO_COUNTER"))?,
    )?;
    drop(raw);
    open_recovery(fs, adapter, limits).map(|(recovery, _)| recovery)
}
