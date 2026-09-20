//! Read-only fixture binding for explicit policy-only reconstruction; no retry-clock dependence.
use super::*;

pub(super) fn verify(
    fs: &mut Fs,
    recovery: &mut Recovery,
    profile: Bm01Profile,
) -> Result<(), LinuxRunnerError> {
    let first = uste_types::CommitRevision::FIRST;
    let mut cursor = recovery
        .open_transaction_cursor(first, first, 1, 1_048_576)
        .map_err(|_| error("USTE_BM01_PACKED_BINDING"))?;
    let transaction = recovery
        .next_recovered_transaction(fs, &mut cursor)
        .map_err(|_| error("USTE_BM01_PACKED_BINDING"))?
        .ok_or_else(|| error("USTE_BM01_PACKED_BINDING"))?;
    if recovery
        .next_recovered_transaction(fs, &mut cursor)
        .map_err(|_| error("USTE_BM01_PACKED_BINDING"))?
        .is_some()
    {
        return Err(error("USTE_BM01_PACKED_BINDING"));
    }
    recovery
        .finish_transaction_cursor(cursor)
        .map_err(|_| error("USTE_BM01_PACKED_BINDING"))?;
    let policy = encode_transaction(&GraphTransaction::with_policy_mutation(
        scope(),
        Vec::new(),
        uste_graph::DurablePolicyMutation::Install {
            policy: benchmark_policy(scope()).map_err(|_| error("USTE_BM01_POLICY_PROFILE"))?,
        },
    ))
    .map_err(|_| error("USTE_BM01_POLICY_ENCODE"))?;
    if transaction.blob_inventory().is_some()
        || transaction.outcome().transaction_id != identity(profile, TransactionId::from_bytes)
        || transaction.canonical_request() != policy
    {
        return Err(error("USTE_BM01_PACKED_BINDING"));
    }
    Ok(())
}
