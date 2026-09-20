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
        (live, base.get(), groups)
    };
    let target = profile
        .continuation_target(frontier)
        .map_err(|_| error("USTE_BM06_PACKED_FRONTIER"))?;
    engine::recovery::complete_generation_prefix(
        &mut live,
        fs,
        kernel,
        principal,
        profile,
        target,
        limits,
        &mut SystemClock::new(),
        &mut |sequence| batch(profile, sequence).map_err(|error| error.code().to_string()),
        &mut |revision| observer(revision).map_err(|error| error.code().to_string()),
    )
    .map_err(|_| error("USTE_BM06_PACKED_MATERIALIZE"))?;
    Ok((live, base, groups))
}
