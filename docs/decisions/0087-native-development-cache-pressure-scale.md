# Decision 0087 — Separate native development cache-pressure scale

Date: 2026-09-18

Status: T-20 development admission; no qualifying campaign or scalability result implied.

After profile-derived work limits, partial I/O accounting and ordered cache eviction, admit native
development fixtures up to 10,000 entities / 100,000 relationships. Keep both memory-adapter
verification commands capped at 1,000 entities. Native create/resume/open, crash probe, query and
supervised sample paths share the new count guard before filesystem or child-process access.
Qualifying 100,000/1,000,000 native execution remains refused. Reports expose the development
ceiling and remain explicitly nonqualifying; no latency, recovery, cache or hardware threshold
is reduced.

The common fixture batch generator no longer owns the memory adapter's admission rule. Its
private validated `Bm01Profile` remains bounded by the accepted exact profile, and it streams
at most 10,000 operations per callback. Adapter entry points own their distinct count limits.
The new native ceiling has exactly 23 journal revisions and 210,001 graph operations; tests
encode every batch and verify the existing 16 MiB request cap, ordered sequence and total counts.
The 64 MiB cache, profile-derived proof/admission/merge limits, deterministic identities,
authorization, portable recovery, normal flushes, separate oracle, query watchdog and strict
output checks remain unchanged.

This permits a resource-scoped development observation, not an automatic large campaign. Start
only with checked host headroom, one heavy workload and the established 3G/4G/512M process-group
limits; record exact commands, versions, timeouts, failures and measured residency. Retain a
partially built fixture if a bounded execution stops so recovery can be inspected without
discarding evidence. A larger cache-pressure fixture does not prove a larger-than-RAM database
or replace BM-06's independent protocol and thirty trials. Missing roots and multi-revision
graph suffix recovery retain their existing fail-closed boundaries pending streaming recovery work.
