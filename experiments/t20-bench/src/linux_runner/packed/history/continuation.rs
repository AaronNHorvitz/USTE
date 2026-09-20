//! Complete a bounded native history prefix using exact retries and authenticated suffixes.
use super::*;

pub(super) fn complete(
    recovery: Recovery,
    fs: &mut Fs,
    profile: Bm06Profile,
    limits: Limits,
    kernel: &mut PolicyKernel,
    principal: &AuthenticatedPrincipal,
    observer: &mut impl FnMut(u64) -> Result<(), LinuxRunnerError>,
) -> Result<(Packed, u64, u64), LinuxRunnerError> {
    let frontier = recovery
        .authenticated_frontier_anchor()
        .ok_or_else(|| error("USTE_BM06_PACKED_FRONTIER"))?
        .0
        .get();
    counts(profile, frontier)?;
    let (mut live, base, groups) = if frontier == 1 {
        let (live, report) = recover_packed_graph_origin(
            recovery,
            fs,
            retention()?,
            CoordinatorRecoveryLimits::new(1, 0).map_err(|_| error("USTE_BM06_LIMITS"))?,
            limits.origin,
        )
        .map_err(|_| error("USTE_BM06_PACKED_BOOTSTRAP"))?;
        if report.suffix.journal.groups != 0 {
            return Err(error("USTE_BM06_PACKED_BOOTSTRAP"));
        }
        (live, 1, 0)
    } else {
        let base = engine::prefix::latest_revision(fs, &recovery, limits)
            .map_err(|_| error("USTE_BM06_PACKED_PREFIX"))?;
        let (live, _, groups) = engine::admit_at(
            fs,
            recovery,
            limits,
            Some(base),
            counts(profile, base.get())?,
        )
        .map_err(|_| error("USTE_BM06_PACKED_ADMISSION"))?;
        engine::recovery::verify_history(&live, fs, kernel, principal, profile, frontier - 1)
            .map_err(|_| error("USTE_BM06_PACKED_HISTORY"))?;
        (live, base.get(), groups)
    };
    let target = if frontier == profile.frontier() {
        frontier
    } else {
        profile.checkpoint_revision()
    };
    for sequence in 2..=target {
        engine::commit_batch_observed(
            &mut live,
            fs,
            kernel,
            principal,
            batch(profile, sequence)?,
            &mut SystemClock::new(),
            limits,
            &mut |revision| observer(revision).map_err(|error| error.code().to_string()),
        )
        .map_err(|_| error("USTE_BM06_PACKED_MATERIALIZE"))?;
    }
    Ok((live, base, groups))
}
