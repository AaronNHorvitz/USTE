# Decision 0192: Packed history phase measurements

Date: 2026-09-20

Status: Accepted; development phase accounting, not recovery qualification.

Partition successful packed native history command work into four fixed sequential stages:

- `setup_and_admission`: root/credential setup, creation or continuation when requested,
  source binding, optional origin rebuild and cold graph/coordinator admission. Construction
  continuation also verifies its existing prefix here; this is not pure recovery timing.
- `history_verification`: the final explicit historical payload oracle at the declared frontier.
- `post_verification_tail_or_retry`: exact recovery retries or certified tail construction,
  plus disposal of that owner. Ordinary read phases have no tail/retry operation.
- `terminal_digest_admission`: the final independent cold open/admission for the v1 digest;
  intentionally pending-tail phases do not attempt a terminal digest.

Capture monotonic elapsed durations and cumulative adapter snapshots at each boundary. Report
checked deltas, refusing backwards time/counters or overflow. Sum every stage's adapter counters
and require equality with the final adapter snapshot. Four fixed snapshots retain no history,
paths, identities or payload. Millisecond total command time remains for compatibility;
stage durations use microseconds, with sub-microsecond rounding explicitly permitted.

The measured interval ends at terminal-digest completion, before process RSS/report formatting.
Adapter observations exclude credential/root-descriptor setup, internal adapter syscalls and
handle drops; wall-clock intervals include the work actually performed between boundaries.
These are not physical-device bytes or complete authenticated-I/O measurements. All commands
retain zero qualifying trials and explicitly mark phase work as nonqualifying recovery latency.
Existing older evidence remains attributed to its pinned implementation, without invented splits.

Tests exercise actual adapter reads/writes/flushes, exact partition durations, clock/counter
rewind refusal and empty intervals. Native create/open/tail/recovery/rebuild/resume and bounded
prefix reports must conserve all four byte counters and all fourteen operation/failure counters.
Existing digests, certificate-prefix checks and fault expectations remain unchanged. This is a
measurement prerequisite; it does not raise admission caps or satisfy the reserved-host campaign.
