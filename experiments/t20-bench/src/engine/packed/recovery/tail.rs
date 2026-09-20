//! Fixture-only generation construction; only the last batch deliberately leaves publication pending.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn certify_generation_tail<
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
>(
    live: &mut PackedEngine<F, W, E, I>,
    fs: &mut F,
    kernel: &mut PolicyKernel,
    principal: &AuthenticatedPrincipal,
    profile: Bm06Profile,
    generation: u64,
    limits: Limits,
    clock: &mut impl uste_storage::Clock,
    make_batch: &mut impl FnMut(u64) -> Result<disk::DiskBatch, String>,
) -> Result<uste_txn::TransactionOutcome, String> {
    certify_generation_tail_observed(
        live,
        fs,
        kernel,
        principal,
        profile,
        generation,
        limits,
        clock,
        make_batch,
        &mut |_| Ok(()),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn certify_generation_tail_observed<
    F: OwnershipFileSystem,
    W: DurableKeyEnvelope,
    E: EntropySource,
    I: EntropySource,
>(
    live: &mut PackedEngine<F, W, E, I>,
    fs: &mut F,
    kernel: &mut PolicyKernel,
    principal: &AuthenticatedPrincipal,
    profile: Bm06Profile,
    generation: u64,
    limits: Limits,
    clock: &mut impl uste_storage::Clock,
    make_batch: &mut impl FnMut(u64) -> Result<disk::DiskBatch, String>,
    observer: &mut impl FnMut(u64) -> Result<(), String>,
) -> Result<uste_txn::TransactionOutcome, String> {
    if !(1..VERSIONS).contains(&generation) {
        return Err("BM-06 tail generation outside update range".into());
    }
    let checkpoint = 1 + generation * profile.batches_per_version();
    let terminal = checkpoint + profile.batches_per_version();
    if terminal > limits.legacy.groups
        || live
            .state()
            .map_err(debug)?
            .current_base()
            .map(|base| base.anchor().0.get())
            != Some(checkpoint)
    {
        return Err("BM-06 tail requires the exact complete preceding generation".into());
    }
    for sequence in checkpoint + 1..=terminal {
        let request = make_batch(sequence)?;
        if request.sequence != sequence {
            return Err("BM-06 tail batch sequence mismatch".into());
        }
        if sequence != terminal {
            commit_batch_observed(
                live, fs, kernel, principal, request, clock, limits, observer,
            )?;
            continue;
        }
        let bytes = encode_transaction(&GraphTransaction::new(scope(), request.operations))
            .map_err(debug)?;
        let mut publication = limits.publication;
        publication.stage.maximum_batches = 1;
        let mut writer = AuthorizedPackedWriter::new(live, kernel, limits.preparation, publication)
            .map_err(debug)?;
        let outcome = match writer.commit(
            fs,
            principal,
            AuthorizedTransactionRequest {
                idempotency_key: request.idempotency_key,
                transaction_id: request.transaction_id,
                canonical_request: &bytes,
                blob_inventory: None,
            },
            clock,
            &NeverCancel,
        ) {
            Err(uste_txn::AuthorizedDiskWriteError::CommittedPublication {
                outcome,
                error:
                    uste_txn::TransactionError::Storage(
                        uste_storage::journal::StorageError::ResourceLimit,
                    ),
            }) if outcome.revision.get() == terminal => Ok(outcome),
            Ok(_) => Err("BM-06 terminal publication unexpectedly completed".into()),
            Err(error) => Err(format!("BM-06 unexpected terminal refusal: {error:?}")),
        }?;
        observer(outcome.revision.get())?;
        return Ok(outcome);
    }
    Err("BM-06 empty tail".into())
}
