//! Bounded exact retries and fresh appends to a declared complete-generation boundary.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn complete_generation_prefix<
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
    target: u64,
    limits: Limits,
    clock: &mut impl uste_storage::Clock,
    make_batch: &mut impl FnMut(u64) -> Result<disk::DiskBatch, String>,
    observer: &mut impl FnMut(u64) -> Result<(), String>,
) -> Result<(), String> {
    let frontier = live
        .state()
        .map_err(debug)?
        .current_base()
        .ok_or("BM-06 continuation requires a published base")?
        .anchor()
        .0
        .get();
    if target <= 1
        || target > profile.frontier()
        || target < frontier
        || target > limits.legacy.groups
        || !(target - 1).is_multiple_of(profile.batches_per_version())
    {
        return Err("BM-06 continuation target outside complete-generation bounds".into());
    }
    verify_prefix_history(live, fs, kernel, principal, profile, frontier)?;
    for sequence in 2..=target {
        let request = make_batch(sequence)?;
        if request.sequence != sequence {
            return Err("BM-06 continuation batch sequence mismatch".into());
        }
        commit_batch_observed(
            live, fs, kernel, principal, request, clock, limits, observer,
        )?;
    }
    Ok(())
}
