# M1 — Bounded local memory integration pilot

Date: 2026-09-18
Status: completed at Decision 0058 for exact implementation `b9689f3`; broader roadmap and release
gates remain open.

## Outcome and non-goals

Deliver a native Rust, offline, source-backed memory backend that a generic consumer can
actually try: ingest an approved fact and its source, restart, retrieve it with a citation,
correct it, expose a contradiction, revoke access, and rebuild safely. Use synthetic data.
The consumer retains its authoritative source store and approval policy. USTE initially
holds a disposable derived index, not the only copy of someone's memory.

M1 is not R1/R2 acceptance, a public alpha, production qualification, or completion of all five
memory functions. Procedures are inert records; remembering one never grants execution.
Working-context selection remains the consumer's job. Complete physical forgetting, backup
erasure and authoritative migration remain later lifecycle work.

[Decision 0054](decisions/0054-memory-first-delivery.md) and [TASKS](../TASKS.md) place
T-63–T-68 ahead of the remaining scale campaign and broader spatial/physics implementation.
They do not delete or close any original task, change FR/NFR requirements, or reduce benchmarks.
USTE stays independently named; consumer-specific integration plans belong to consumers.

## Work order and artifacts

1. **T-63 — Stabilize and bound.** Inspect interrupted recovery changes without discarding
   or silently accepting them. Record the selected commit and dirty-work disposition.
   Verify the chosen journal/recovery/backend path and freeze an explicit pilot profile.
   Unverified optional recovery work can remain excluded from that profile; required safety
   fixes cannot be deferred. No claim that the crashed session's last changes passed.
2. **T-64 — Write and reopen.** Implement source/record admission, immutable source versions,
   evidence links, corrections and durable idempotent retries through existing authorization.
   Resolve the recovered blob-reservation limitation sufficiently to ingest new sources after
   restart, with quota reconciliation and abandoned-upload fault tests; do not disable quotas.
3. **T-65 — Retrieve and explain.** Provide identity, bounded graph and lexical retrieval over
   admitted text, returning exact source versions and byte/line locators. Compare against an
   independent small-corpus oracle. Support the declared current and historical knowledge
   queries; explicitly reject other temporal/spatial predicates rather than approximate them.
4. **T-66 — Revoke and rebuild.** Enforce current source policy at read time, prevent stale
   results after correction/revocation and specify fail-closed resynchronization. An index
   generation not reconciled to the source authority must not serve results. Test cleanup,
   interrupted rebuild and refusal of stale copies; distinguish read exclusion from physical
   erasure. Do not claim full purge/backup guarantees from deleting an index directory.
5. **T-67 — Make it runnable.** Supply a generic restricted Rust adapter, a local CLI/harness,
   setup instructions and a scripted offline demonstration. Prefer the smallest reviewed
   transport; an embedded adapter does not prove local-service IPC security. Any IPC included
   must implement peer authentication, key custody, framing, limits and process ownership.
   No model, hosted account, feed connector or provider secret is needed.
6. **T-68 — Qualify and hand off.** Run all exit cases, publish exact build/features and
   measured limits, document rollback, and provide a consumer integration checklist.
   The consumer must separately register and test its integration; do not modify another
   repository's runtime as an incidental milestone step.

Implement shared components in the main native engine, not a throwaway second database.
Cross-reference reuse in T-21–T-28 and T-34–T-36 without closing their broader scopes.

## Pilot resource and compatibility contract

Before admitting data, T-63 must freeze numeric caps for total retained state, record/history
counts, source bytes, per-record payload, journal/coordinator metadata, retry ledgers,
staged uploads, query work/results, concurrency, RSS and recovery time. Include temporary
copies and replay peaks, not just the live graph. Cap checks must survive reopen and reject
before accepting writes that exceed recoverable state. Long-running growth requires an
explicit stop/rebuild or qualified maintenance path, never silent data loss.

A small in-memory projection over durable encrypted state is permitted if measured within
those caps. There is no larger-than-memory claim until T-20/BM-01/BM-06 qualify.
Existing full-product budgets, including the unmet BM-04 target, remain unchanged.
Record host RAM/swap, filesystem, build profile and concurrency; leave headroom for the
desktop. Run bounded jobs and stop on the declared budget, not host-wide memory exhaustion.
Record p50/p95/p99 retrieval latency, ingestion throughput, cold restart time and peak RSS.
Freeze acceptance budgets before measurement; do not choose passing thresholds afterward.

The initial format profile is trusted-consumer-supplied UTF-8 text plus opaque byte storage
within limits. Stored bytes are not parsed merely by being present. Bind extracted text to
exact source bytes/version and honest locators; arbitrary extraction requires the later
isolated parser boundary. PDF/Office/OCR/audio/video understanding, embeddings, navigation,
motion/contact simulation and imported-price examples remain on the preserved roadmap.
No instructions, macros or code from a source are executable through this profile.

Pin schema/disk/API versions, reject unknown formats, and document the upgrade boundary.
No silent downgrade, destructive reset or unspecified migration of an existing database.

## Required exit cases

Record commands, expected outcomes, actual results and exact tested revision for each case:

| Case | Must demonstrate |
|---|---|
| M1-A | Approved scoped facts, relationships and exact source bytes survive clean restart and forced process termination after durable acknowledgment |
| M1-B | Lost-response retry is idempotent; interrupted uploads reconcile safely; new ingestion succeeds after reopen without quota leakage |
| M1-C | Current and historical queries distinguish source/event time from recorded knowledge; corrections and contradictions preserve honest evidence |
| M1-D | Retrieval resolves exact authorized source/version/locator and original bytes; changed bytes, malformed locators and unsupported queries fail explicitly |
| M1-E | Another namespace, stale approval or revoked source cannot leak records, snippets, counts, graph paths, bytes or cached results |
| M1-F | Disk full, corruption, wrong key, lock conflict, cancellation and exhausted budgets fail safely; committed-state recovery is checked against the oracle |
| M1-G | Interrupted synchronization/rebuild and stale index copies never resurrect revoked facts; source authority remains intact when the index is disabled |
| M1-H | A fresh authorized local checkout can run the documented synthetic demo offline without cloud/model credentials |
| M1-I | Measured cold/warm query, ingestion, recovery and peak-memory results meet the frozen pilot profile with normal encryption, authorization and durability |
| M1-J | An exact-version consumer handoff states supported operations, limitations, rollback, remaining full-product tasks and separate integration prerequisites |

These cases supplement applicable existing VT coverage; none replaces an original VT/BM gate.
No checkbox closes from documentation, a mock-only adapter or a happy-path screenshot.

## Integration boundary and return to the full roadmap

Consumer adoption starts with shadow queries into a rebuildable index. Exact approval/scope
and source-version checks remain in both layers. Use outbox/checkpoint reconciliation and
watermarks rather than pretending two stores share an atomic commit. Failure falls back only
to the consumer's existing authorized local path or visibly disables retrieval.
Personal/sensitive datasets, sole-copy storage and authoritative cutover are not authorized
by M1; require appropriate lifecycle, migration, threat-review and consumer acceptance gates.

After verified T-68, hand off the pinned backend and resume T-20/T-19, then full R2, R3 and R4.
Spatial indexes, geographic/history queries, navigation, kinematics, constrained contact physics,
rich files, complete retention/purge, backup/restore, operational hardening and independent
review remain required at their existing gates. M1 supplies no speed/security certification.
T-62 remains mandatory before any executable distribution outside authorized development
participants; local M1 work does not require GitHub administration credentials.
