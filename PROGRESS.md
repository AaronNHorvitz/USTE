# Implementation progress and handoff

Updated: 2026-09-18 · Branch: `codex/uste-implementation`

Latest completed task is T-68; M1 is complete at exact implementation `b9689f3`. Decision 0055 selects verified recovery commit `7393def` and freezes
the executable `memory-pilot-v1` limits in the new safe-Rust `uste-memory` crate. Implementation
commit `97537e5` adds durable source/fact admission, bounded authorized retrieval, exact citations,
corrections, revocation and fail-closed rebuild under Decision 0056. Commit `a07bdec` adds the
Decision 0057 restricted embedded Linux adapter, consumer-owned versioned source/outbox contract
and runnable offline demo. Decision 0058 records the passing M1-A–J matrix, frozen-limit release
measurements and exact consumer handoff. Work now resumes T-20 and then T-19; full-project tasks and
release gates remain open.
Branch history
through the current handoff adds T-20's encrypted disk-index, authorized-read, bounded checkpoint transport,
deterministic benchmark-fixture foundations, bounded graph deltas/reverse dependencies and the
complete certificate-anchored `graph-state-v1` root plus bounded semantic reconstruction. The
certificate-paired coordinator metadata increment adds an executable cold root-to-seeded-open path.
Decision 0032 adds authenticated bounded base/delta scratch merge, and Decision 0033 connects one
bounded graph transaction to an independently validated terminal `graph-state-v1` root. The
Decision 0034 makes ordinary graph/spatial/composite ingest preparation request-bounded.
Decision 0035 adds a bounded explicit-I/O current-record proof phase followed by storage-free graph
preparation. Decision 0036 adds complete bounded history/reverse proof buckets so every graph
operation and precondition variant can use that phase. Decision 0037 carries authenticated
metadata/counts into a proof-derived terminal-root plan. Decision 0038 binds
that proof result into the authoritative coordinator commit without repeating full-state
preparation. Decision 0039 adds explicit authorized cache/I/O measurement. Decision 0040 connects a
capped, nonqualifying production-engine equivalence driver to the pinned fixture and oracle.
Decision 0041 streams exact-profile construction through bounded transactions and pins its
212-revision plan. Decision 0042 adds resumable Linux/Btrfs materialization and authenticated
portable-recovery open phases. Decision 0043 adds a separate bounded oracle summary and Linux
correctness-query phase. Decision 0044 adds real durable-prefix SIGKILL/resume coverage. Decision
0045 adds the independent warm-up/measured oracle bundle. Decision 0046 adds repeated paired-cache
sampling without claiming a benchmark pass. Decision 0047 adds a preemptive persistent-worker query
deadline for the CLI. Decision 0048 removes the complete snapshot from proof-derived terminal-root
publication. Decision 0049 returns that admitted root directly for the next disk preparation.
Decision 0050 adds the resumable cursor and predecessor proof needed for cold admission. Decision
0051 uses them to semantically admit a cold `GraphDiskBase` without rebuilding complete graph maps.
Decision 0052 replaces the complete graph in the warm authoritative write loop with one admitted
base and one bounded pending terminal-root plan. Decision 0053 reopens that state at an authenticated
journal frontier as either the ready base or exactly one revalidated pending suffix, without
reconstructing `GraphState`. Decision 0059 at `8885da4` removes the hidden absolute-maximum root
pre-scrub from graph recovery: fixed manifest discovery now precedes one caller-bounded semantic
scan. T-20 remains open pending disk-backed coordinator metadata, larger-than-memory qualification
and qualifying BM-01/BM-06 results. Review was
performed by Codex agents and does not represent independent external security certification.

T-49 is complete at its typed R1 transaction-contract scope. A T-19 audit found that its required
BM-01/BM-06 results depend on T-20, while T-20 incorrectly depended on T-19. Decision 0024 preserves
every budget and orders T-20 first; T-19 and R1 acceptance remain open. T-62 remains an independent,
unverified external distribution prerequisite.

## Completed this increment

- Added `CoordinatorRecoveryLimits` and `open_journal_anchored_prepared_bounded`: callers can
  cap retained outcomes and first blob owners before coordinator map insertion during recovery.
  Duplicate blob inventory use consumes no additional owner slot. Refusal returns no coordinator
  and releases ownership; a later adequately admitted open recovers the exact durable suffix.
  The first regression exposed nested storage error mapping; recovery budget refusal now returns
  typed `TransactionError::ResourceLimit`. Final sequential capped verification passed 29
  transaction tests, 5 graph disk-index tests and warnings-denied transaction clippy. These are
  count limits for coordinator maps, not journal/RSS bounds or disk-backed metadata completion.
- Resumed after the capacity interruption and pushed `8885da4`/`808f2fd` without rewriting history.
  Confirmed the M1 pin's lock digest and unchanged pilot sources against Decision 0058, and
  reconciled obsolete T-64 next-step instructions with completed T-63–T-68 status.
- Extended bounded manifest discovery to both coordinator metadata owners. The existing seed
  reconstruction continues to authenticate all families and enforce caller budgets before return.
  `CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo test -p uste-replay --test coordinator_checkpoint
  --locked --offline` passed all 3 cases; strict `uste-txn --all-targets` clippy passed. Both ran
  sequentially under `MemoryHigh=3G`, `MemoryMax=4G`, `MemorySwapMax=512M`.
- Advanced T-20 at `8885da4` with Decision 0059. Graph root discovery now authenticates only two
  fixed manifests and journal anchors before caller-selected cursor budgets apply; it no longer
  scans every run twice or allocates the default scrub cache before recovery limits. A same-length
  corrupt run remains provisional and fails cursor/admission validation. The publication overwrite
  path deliberately retains its complete scrub to protect the sole good fallback.
- Sequential 4 GiB-cgroup verification passed 43 graph, 77 storage and 28 transaction tests,
  including complete storage crash matrices, plus warnings-denied clippy. The complete repository
  gate also passed with 132 documentation links, 129 active IDs, 152 definitions, the 68-task graph
  and 31 passed/2 exact-profile-ignored scaled T-20 tests. No BM-01/BM-06 campaign ran because host
  swap remains saturated and the coordinator-metadata boundary is not yet honest.
- Completed T-68/M1 at `b9689f3` plus Decision 0058 evidence. A real child acknowledged memory
  revision 4, was SIGKILLed and recovered the exact citation in a fresh process. Wrong password and
  committed-certificate mutation fail closed. Existing no-space publication matrices, pilot query
  cancellation/budgets, authorization/revocation and independent-oracle recovery complete M1-F.
- The exact `b9689f3` release path measured 55,512,414 B/s for a durable encrypted 1 MiB ingest, 355 ms cold
  recovery, warm authorized-query p99 below 1 microsecond reporting resolution across 1,000 samples,
  and 265,180 KiB peak RSS. All pass the thresholds frozen in T-63; no threshold or full-product
  benchmark was changed.
- Added the exact-version consumer handoff pinned to full commit, Cargo.lock SHA-256, Rust 1.95.0,
  default features and x86_64 Linux/Btrfs. It requires separate consumer-side source/outbox/policy
  mapping, shadow testing and rollback; it authorizes no AgentMage change or authoritative migration.
- Completed T-67 at `a07bdec` with `uste-memory-adapter`. One mutable embedded adapter owns the
  Linux/Btrfs writer, key, scope, policy principal and authority generation; it returns only bounded
  owned results. The consumer remains authoritative for immutable source bytes and a strict
  version-1 upload outbox that is durable before staging and cleared after an exact idempotent commit.
- Added `scripts/run_memory_pilot_demo.sh` and a separate-root synthetic consumer. The real encrypted
  Btrfs run reached generation 2/revision 11 after exact ingest/citation, competing-owner refusal,
  reopen, correction/history, contradiction, revocation and rebuild. Adapter tests reject unknown
  checkpoint versions/scope/duplicates and warnings-denied clippy passes. The complete capped
  repository check passes with 127 documentation links/124 active IDs/152 definitions, the 68-task
  graph, and 31 passed/2 qualifying-only ignored scaled T-20 cases. No IPC, cloud, model, provider
  credential, AgentMage change or authoritative migration is included.
- Completed T-64 at `97537e5`: canonical `UMEM` 1.0 transactions now admit immutable exact-text or
  opaque source versions, evidence-bound facts, corrections, contradictions, retractions and source
  revocation through the existing encrypted blob/journal/policy path. Retry returns the same durable
  outcome after reopen. A trusted complete upload outbox can reconcile committed/aborted/empty
  staging and reopen ingestion without disabling quotas; unresolved durable staging remains closed.
- Completed T-65 with identity, bounded lexical and one-hop reads plus exact source citation
  resolution. The pilot explicitly supports recorded current/as-of knowledge and exact/missing
  source-event filters only. An independent fixture oracle agrees; cross-scope and unsupported
  queries, candidate/output budgets and cooperative cancellation fail explicitly.
- Completed T-66 with current source-policy filtering, durable revocation, process-local stale-view
  invalidation and consumer authority generations. Begin-rebuild durably clears/blocks the derived
  projection, remains blocked across restart and serves only after exact reimport plus completion.
  Decision 0056 and the core evidence state that this is read exclusion, not physical erasure.
- Focused sequential verification under the 4 GiB/512 MiB cgroup passed 6 `uste-memory` and 28
  `uste-txn` tests plus warnings-denied clippy. The complete `scripts/check.sh` also passed in that
  capped scope: documentation reported 124 links/121 active IDs/152 definitions, the 68-task graph
  passed, and the scaled T-20 experiment reported 31 passed with its two qualifying cases ignored.
  The exact end-to-end M1 fault/measurement matrix has not run and is reserved for T-68 after the
  T-67 executable adapter exists.
- Recovered the unstaged Decision 0053 implementation without discarding or presuming it valid.
  Focused transaction/graph suites, warnings-denied clippy and the complete repository check passed
  before it was committed and pushed as `7393def`.
- Completed T-63 with Decision 0055 and `uste_memory::PILOT_PROFILE`: 4,096 commits, 256 source
  versions, 2,048 retained facts, 32 MiB total/1 MiB per source, 16 MiB logical state, bounded
  staging/query/concurrency, 512 MiB RSS and predeclared recovery/latency/throughput thresholds.
  These are pilot limits, not replacements for BM-01/BM-04/BM-06.
- Added T-63 evidence recording the exact baseline, dirty-work disposition, host RAM/swap/Btrfs
  preflight and sequential 4 GiB-cgroup verification. The large T-20 campaign was not run.

- Recorded owner-authorized Decision 0011, separating local development governance (T-06)
  from operational private-reporting verification (new T-62) without claiming verification.
- Corrected reporting participation to include the reporter and authorized repository security/
  triage participants, rather than the inaccurate maintainer-only description.
- Closed T-06/T-07 and R0 at the development decision/evidence scope after aligning stale
  pre-decision specification text and rerunning its actual prerequisites.
- Added a task-graph check proving the 62-task graph is acyclic, T-08 does not inherit T-62,
  and R4 decision T-44 does inherit T-62. R2/R3 are explicitly local readiness gates and no
  executable distribution is allowed while T-62 is open.
- Completed T-08 with a Rust 1.95.0/resolver-3 workspace, initial safe-Rust `uste-types` crate,
  root lockfile, read-only CI, active-document/reference validation and reproducible quality/
  supply-chain scripts. Excluded experiment workspaces retain their independent pinned locks.
- Completed T-09 with Decision 0012, bounded generic values, typed/scoped identities, UTC
  instants, nonzero monotonic commit revisions and a strict canonical format-1.0 codec.
- Completed T-10 with an independent `uste-testkit` oracle using ordered maps, clone-and-publish
  transactions, retained revision snapshots and scans rather than future production reducers.
- Completed T-11 with Decision 0013 and a safe first-party `uste-crypto` boundary around the pinned
  RustCrypto profile: exact object/recovery envelopes, fixed authenticated context, padded in-place
  AEAD, bounded nonce sessions, redacted lockable key ownership and a portable fixed-cost Argon2id
  recovery adapter.
- Added literal `crypto-v1` profile vectors and fail-closed coverage for every context field and
  ciphertext mutation, truncation, wrong key/password/database, entropy/nonce failures, locked use,
  maximum payload, authenticated malformed recovery padding and writer-incarnation separation.
- Corrected review findings before binding evidence: caller-sized crypto buffers now reserve
  fallibly, rejected credentials zeroize, resource failures remain retryable, guarded-memory is not
  claimed and future clone/restore/rotation/hardening responsibilities stay explicit.
- Completed T-12 with Decision 0014 and safe handle-relative filesystem, clock and randomness
  capabilities plus a deterministic volatile/durable memory adapter and per-operation fault plans.
- Covered interrupted and short I/O, zero/over-reported progress, disk full, failed file/directory
  sync, crash-before/after rename and data flush, sticky crash/restart, stale handles, wall rollback,
  random failure and exact repeatability. Crash state cannot be cleared without adapter restart.
- Added a guarded real-process scenario under Btrfs-backed Cargo target scratch: the child flushes
  file bytes and the directory, signals over a pipe, is killed with SIGKILL, and exact bytes reopen.
  This is process-loss harness evidence, not a power-loss or production-filesystem claim.
- Began T-13 with Decision 0015 and the pinned `rustix 1.1.5` x86_64 Linux adapter: descriptor-rooted
  no-symlink opens, checked object types, positional I/O, 0600/0700 creation, no-replace rename,
  explicit data/full/directory syncs, recovery truncation and unique-FD nonblocking ownership.
- Implemented encrypted creation manifest, authenticated log/segment headers, exact opaque group
  envelopes, fixed hash-chained commit certificates, poisoned uncertain writers and bounded
  streaming recovery. T-13's empty-inventory profile is extended by T-15's verified nonempty blob
  inventories.
- Verified exact byte replay, competing-owner rejection, durable lost-response recovery, previous-
  frontier recovery after certificate-sync failure, incomplete-tail repair and hard failure for
  complete certificate/group corruption. A Btrfs child was SIGKILLed after certificate sync and
  the production adapter reopened the exact committed group.
- Completed T-14 with Decision 0016 and `uste-txn`: owned prepare/publish changes, encrypted canonical
  transaction groups, coherent pinned readers, namespace/principal retry scope, durable outcome
  reconstruction, expiry and uncertain-handle quarantine. Literal group/malformed recovery,
  complete initial publication-fault, short-I/O, both cancellation-boundary, 32-caller conflict,
  restart and lost-response retry tests pass.
- Completed T-15's local acceptance with Decision 0017: bounded encrypted blob chunks, temporary-
  to-canonical staging, per-chunk authenticated progress witnesses, paired terminal witnesses,
  key-derived opaque inventory names, canonical inventory publication, commit-gated range reads and
  bounded recovery-time verification. Missing first/middle/last acknowledged chunks, single
  terminal-copy loss, malformed canonical objects, cross-context replay and foreign handles fail
  closed without token resurrection or silent shortening.
- Exercised error, no-space, short/zero progress and crash-before/after behavior across chunk,
  progress, multi-chunk finish, both terminal copies, abort cleanup, inventory, group and certificate
  publication. Blob-bearing lost-response retry, exact/+1 storage-profile limits and same-byte
  upload/namespace isolation pass.
- Measured the release-built production Linux-adapter path with normal Argon2id recovery wrapping,
  encryption and durability: a 12 GiB commit/reopen/full-hash round trip used 267,636 KiB peak RSS,
  1.06653x disk bytes and reproduced SHA-256
  `7eb969346fd20004fd1bb01f0ba1a8b356aea4c684ba22b1f8fde7c0014589c4`. Ingest was 95.923 MiB/s,
  below BM-04's 250 MiB/s target, so BM-04 remains unpassed despite T-15 correctness/RSS closure.
- Completed T-16 with Decision 0018 and a storage-independent, safe-Rust `uste-policy` kernel:
  trusted-adapter authentication mints opaque issuing-kernel-bound principals; absent policy denies;
  action permissions are independent; record rules narrow namespace grants; and policy revisions
  invalidate leases. The consumer transaction request has no caller-controlled principal field.
- Added the mandatory authorized transaction facade. It checks namespace and reducer-declared
  record requirements before state/index/storage access, rejects cross-namespace reducer targets,
  binds views/uploads to their issuing coordinator and exposes no generic reducer snapshot.
  Transaction-ID outcomes are owner-filtered, and denied unknown/existing paths use content-free
  failures. A review-found same-scope fabricated resume-token capability was closed by requiring
  authenticated durable journal evidence for every token absent from the in-memory ledger.
- Enforced checked exact-plaintext quotas for request bytes, staging, committed unique blobs, live
  handles and range reads. Failed writes reconcile accepted bytes; finalize retains the reservation;
  abort releases it; commit moves it once. Recovery reconstructs first-publication ownership and
  committed charges. Zero-byte reservations are capped at 32 and inventory lookup has a blob index.
- Format 1.0 cannot enumerate abandoned uncommitted reservations, so a recovered authorized
  coordinator conservatively rejects new upload starts. Evidenced known uploads, including a
  zero-byte final marker, remain resumable for reconciliation/abort; T-35 owns complete enumeration.
- Completed T-17 with Decision 0019 and the safe-Rust `uste-graph` production reducer: typed scoped
  entities/evidence/assertions/relationships, strict canonical requests, revision history, explicit
  valid time, lifecycle/corrections, reference closure and exact bounded cascade/retract deletion.
- Added atomic ordered outgoing/incoming adjacency and evidence-provenance indexes with independent
  rebuild checks. Cycles, self-loops, parallel edges and terminal transitions retain deterministic
  behavior. A separate 1,000,000-candidate visit ceiling stops before over-limit allocation.
- Added durable native graph policy bootstrap/replacement/history and reducer-owned authorized
  projections. Missing/mismatched durable policy fails closed, consumer install is rejected, stale
  idempotent replacement retries preserve the current version, revocation stales views and an
  uncertain commit quarantines existing views.
- Closed review-found graph disclosure paths: corrections require read authority on the new ID;
  candidate caps apply after filtering; direct/traversal/provenance results authorize every embedded
  endpoint, evidence, correction and nested property reference without returning hidden counts.
- Bound recovery receipts to complete canonical affected-record and policy bytes. Future policy and
  record history revisions fail explicitly. The production reducer agrees on shared modeled record
  state with the independent ordered-map oracle after each of 160 generated transactions, validates
  its derived-index rebuild separately, and restores graph/policy state after encrypted restart.
- Completed T-18 with Decision 0020 and `uste-replay`: contiguous cold replay verifies result
  digests before publish, graph checkpoints preserve all record/policy history, and canonical
  coordinator checkpoints bind retained outcomes and first-commit blob ownership.
- Added opaque storage-authenticated candidates, exact historical certificate anchors and seeded
  coordinator open. Recovery reauthenticates the full journal, exactly compares prefix coordinator
  metadata at the anchor and applies the ordinary reducer only to the suffix.
- Added alternating encrypted 1 MiB checkpoint chunks with a 256 MiB cap and terminal manifests.
  The 28-case crash matrix, 21-case authenticated malformed-carrier matrix, missing/corrupt/swapped
  chunk fallback and encrypted graph checkpoint/suffix restart equivalence pass.
- Closed review findings for split-brain generations, frontier-only anchor matching, forgeable
  seed provenance, infallible large-frame allocation and encoder/decoder invariant drift. Final
  code/evidence review reported no remaining high- or medium-severity T-18 finding.
- Completed T-45 with Decision 0021 and the safe-Rust `uste-time` crate while keeping `uste-types`
  std-only: strict RFC 3339 and explicit-unit numeric normalization, checked full-range Gregorian
  arithmetic, hash-verified embedded TZDB 2026c local resolution and explicit local presentation.
- Added bounded canonical timestamp envelopes carrying exact source token, artifact/version/locator,
  interpretation, precision, uncertainty, assumptions and accepted result. Closed invariants reject
  unknown profiles, contradictory states and over-limit inputs before copying or scanning them.
- Proved graph cold replay restores the authenticated accepted instant without reparsing the source
  token or resolving a zone. Equal and rolling-back wall samples still publish and recover distinct
  consecutive revisions. All 12 R0 time vectors execute across time/graph tests, and all 3,652,059
  supported Gregorian days round-trip through the independent checked calendar arithmetic.
- Completed T-48 with Decision 0022, exact fixed-point local/geographic primitives in std-only
  `uste-types`, and safe-Rust `uste-spatial` schemas for worlds, immutable frames/geometries and
  source-backed position observations. Missing height/position stays unknown and stored spatial
  truth has no floating/nonfinite representation.
- Added a bounded atomic reference-history catalog with exact recorded-revision visibility,
  gap-free versions, category/scope/dimension checks, root/parent closure, a 32-edge frame-depth
  ceiling, cycle rejection, immutable observation IDs, idempotent source events and prior-commit
  same-entity/world corrections. An independent scan oracle agrees on every tested frame depth.
- Added canonical transactions, effect-bound insert/retry results, cold replay and canonical
  checkpoint rebuilding. A 64 MiB canonical logical-byte bound makes the R1 catalog's hostile-input
  limit explicit; Decision 0034 later removes its prepare clone, but it remains an in-memory
  correctness catalog rather than a scalable index or BM-10 result.
- Bound 11 spatial record variants plus a transaction and checkpoint to exact length/SHA-256
  fixtures. Every record/transaction/checkpoint truncation and trailing input fails; checkpoint
  count, scope, ordering, duplicate and future-revision mutations fail closed. Final agent review
  found no remaining high-severity T-48 core issue.
- Completed T-49 with Decision 0023 and safe-Rust `uste-ingest`: one capability-free reducer owns a
  cloned graph/spatial/private-ledger candidate and publishes it only after exact source/mapping,
  authorization, checkpoint and current/historical external-reference closure all pass.
- Added bounded canonical start/batch contracts, trusted no-write preview, namespace-global source-
  event identity, durable job/batch receipts, logical hashing and 256 MiB composite checkpoints.
  Exact coordinator retry survives encrypted memory-adapter restart; alternate retry identity,
  changed bindings, stale batches and duplicate events fail closed.
- Closed review findings for same-ID/different-metadata blob binding, job deletion, embedded job-
  binding authorization leakage, cross-job event reuse, infallible frame copies and restore-time
  temporal closure. Final agent review found no remaining high-severity T-49 issue.
- Kept the T-49 boundary explicit: payload digests are caller-declared commitments, preview is a
  trusted raw helper, batches are fully accepted and nonempty, and T-54 still owns canonical per-row
  effect verification, CSV/JSON mapping, rejected-row reports, CLI and exact item/price fixtures.
- Advanced T-20 with Decision 0025 and native Rust encrypted immutable sorted runs: exact 16 KiB
  logical pages, 2 KiB roots, opaque two-slot root names, exact certificate/profile bindings,
  bounded prefix reads and a 64 MiB default/256 MiB maximum decrypted-page cache.
- Added current-record, outgoing/incoming adjacency and provenance disk families. Admission now
  compares the supplied snapshot with the coordinator's exact live reducer state, independently
  recomputes mandatory family/count/digest expectations, fully scrubs durable pages and rejects a
  stale handle after any later commit.
- Closed review findings for cross-database cache confusion, run/root binding, plaintext-bearing
  cache diagnostics, cached scrub bypass, same-length/trailing run corruption, transient I/O
  misclassification, corrupt-newest overwrite fallback and same-revision foreign snapshots.
- Pinned the page/root format and opaque `IndexName` role in literal acceptance vectors. Root and
  run corruption, operational read faults, all root-publication crash boundaries, encrypted restart,
  self-consistent wrong projections and raw graph reference equivalence pass. This is explicitly
  not T-20 closure or BM evidence; see `docs/evidence/disk-index-foundation.md`.
- Added reducer-owned indexed reads to the mandatory authorization facade. Top-level targets are
  authorized before index I/O; adjacency/provenance candidates and every embedded reference are
  filtered without exposing hidden counts; stale policies/views fail before disk access; current
  roots reject historical requests. Facade maintenance requires `ManageSchema`, handles keep cache
  counters opaque, mixed-direction scans share global candidate/byte limits, and admitted view
  binding is constant-time. Authorized in-memory/disk results agree before and after an encrypted
  restart.
- Added borrow-aware reducer checkpoint access so cold replay, checkpoint metadata and graph/spatial
  state encoding do not clone retained snapshots solely for inspection. Graph canonical checkpoint
  bytes now stream to a fallible sink and remain byte-identical to the format-1.0 collector.
- Added declared-length checkpoint publication with a one-new-payload-chunk plaintext buffer and
  terminal-manifest visibility only after exact production. Explicit producer failure, short and long
  streams leave the earlier candidate usable. Index prefix scans now also expose a bounded visitor
  path instead of requiring result collection.
- Added bounded checkpoint recovery transport: discovery authenticates/rehashes candidates without
  retaining complete plaintext, filters opaque metadata against the verified certificate chain and
  revalidates a selected chunk stream under the live owner/key context. Partial sink output is never
  publishable unless the terminal digest succeeds; current reducer decoders remain full-state.
- Added Decision 0027 and bounded graph transaction overlays. Successful preparation clones and
  validates only changed records; publication applies exact history, adjacency and provenance
  contributions instead of replacing/rebuilding the complete snapshot. Prepared deltas are
  non-cloneable and bound to their exact scope/base revision before any live mutation.
- Proved a one-record update in a 1,024-record graph retains one ordered before/after change, matches
  the prior canonical result digest and a full-rebuild reference snapshot, and rejects a stale
  same-base prepared delta without mutation. Cascade retraction removes adjacency but retains
  provenance. Final agent review found no remaining high- or medium-severity finding.
- Added Decision 0028 and an incrementally maintained reverse-dependency map with explicit owner
  kind/state/version/revision and ORed roles for entity, assertion and relationship references.
  Checkpoint decode rebuilds it and independent derived-index validation compares it.
- Delete now reconciles the target's base bucket with earlier changes in the same transaction,
  rather than scanning unrelated records. Regressions cover added/removed property references,
  accepted/proposed/retracted status changes, exact cascade and duplicate-mutation ordering, nested
  duplicate role aggregation and rejected-prepare atomicity.
- Added Decision 0029 and the pinned `graph-state-v1` profile: metadata, current records, complete
  record history, outgoing/incoming adjacency, provenance, reverse references and current/policy
  history stream into at most eight encrypted immutable runs under an exact certificate/root bind.
  Metadata offsets and all reverse kind/state/role codes are literal acceptance data.
- State-root publication independently verifies produced family counts/digests; admission compares
  the exact live scope/revision/logical digest, rejects self-consistent wrong metadata, fully scrubs
  pages, survives encrypted restart and becomes stale after the next commit. It remains optional and
  cannot construct reducer state or change journal authority. A canonical fixture pins every
  nonempty family key/value/count and seven run digests; focused agent re-review found no remaining
  high- or medium-severity issue.
- Added Decision 0030 and a privileged, certificate-rechecked full-run visitor outside consumer
  APIs. It uses one stable handle, bypasses shared cache, enforces caller page/entry/logical-byte bounds,
  authenticates every page, retains at most one assembled entry and verifies exact length,
  ordering, entry count and terminal logical digest. Partial visitor output is explicitly provisional.
- Added a distinct anchored graph-state candidate and bounded reconstruction path. It validates
  metadata/family shape before allocation, decodes current/history/policy into private maps through
  the checkpoint semantic constructor, rebuilds and stream-compares derived families, and checks
  all descriptors plus the state digest before returning. The all-eight-family fixture proves exact
  equivalence, restart, stale historical reconstruction, wrong-root and caller-budget rejection;
  authenticated descriptor-consistent current/history and adjacency mismatches also fail closed.
- Reconstruction removes a monolithic checkpoint byte buffer but still reads already-scrubbed runs
  again and materializes full ordinary `GraphState`. Caller load limits govern only this second
  pass; discovery's first scrub is bounded by absolute carrier maxima. The root omits retry/
  transaction/blob-owner metadata, so no coordinator seed, larger-than-memory recovery or
  benchmark pass is claimed. Focused final agent review found no remaining high- or medium-severity
  issue; this is not independent external security certification.
- Added Decision 0031 and the frozen `coordinator-meta-v1` profile with mandatory metadata plus
  optional ordered outcome and first-blob-owner families. Counts, fixed logical bytes, reserved
  fields, transaction uniqueness, blob shape and descriptor families are checked before seed use.
- Added a temporary authenticated recovery owner and complete cross-profile anchor identity. Cold
  recovery now rejects an unpaired newer metadata root, reconstructs a historical graph/metadata
  pair, drops the reader, and lets seeded open reauthenticate the prefix and replay the suffix.
- Seeded prefix verification now moves entries from expected maps to verified maps while decoding,
  avoiding an additional complete prefix-map copy. Graph and coordinator maps remain full-memory;
  caller reconstruction limits do not bound discovery or allocator RSS.
- Added Decision 0032 and a trusted authenticated merge from one optional `index-v1` base plus
  exact sorted before/after deltas into one unpublished encrypted current-revision run. Present
  before-values compare byte-for-byte, absent before-values require absence and absent after-values
  are tombstones; empty output creates no run.
- The merge holds one stable source handle through terminal length/count/digest verification and
  separately caps source pages/entries/logical bytes, delta count/bytes and output entries/bytes.
  Multi-page insert/replace/delete, all-tombstone output, no-base construction, wrong-before/order
  rejection, corruption discovered after provisional output and ten target create/write/size/sync
  crash boundaries pass. Frozen `index-v1` still has one terminal run per family, and the primitive
  itself neither validates graph semantics nor publishes a root.
- Added Decision 0033 and an opaque no-I/O graph delta plan bound to the exact admitted base anchor,
  scope, base/target revision and prepared result digest. It converts current/history, accepted
  adjacency, provenance, canonical reverse-role and current/history policy changes into sorted,
  coalesced exact deltas under aggregate count/byte limits.
- Postcommit publication requires the matching durable outcome, merge-rewrites all eight families,
  independently recomputes the expected terminal descriptors and full logical digest from the
  actual current reducer, and publishes only the complete matching family set. Cache failure cannot
  undo or weaken the journal transaction; partial scratch runs remain invisible.
- The revision-three-to-four disk fixture combines entity reference replacement, accepted-edge
  retraction, source-backed assertion creation and policy replacement. It rejects an undersized
  preflight, incremental count and logical-byte plan budgets plus a wrong outcome, then proves a
  postcommit merge-budget failure leaves the committed state intact and no target root visible
  across storage restart. The test harness retains the non-durable plan for an in-process retry;
  actual process loss requires a full-root rebuild. The retry exercises insert/replace/delete plus
  empty adjacency output, proves delta and full roots both reconstruct to the exact live state, and
  reopens both after restart. The
  independent validator remains a full in-memory scan, so no larger-than-memory or benchmark claim
  is made.
- Added Decision 0034 and removed complete retained-state candidate copies from ordinary spatial
  and composite ingest preparation. A borrowed graph view overlays exact changes for closure
  validation; the spatial overlay retains request indexes, per-request effect revisions/outcomes
  and inserted records only; the ingest ledger delta retains one job/receipt plus admitted rows.
- Graph scope/revision/policy-version/touched before-values, the optional spatial scope/revision/catalog
  fingerprint and job/sequence/source-event bases are all preflighted before component mutation.
  Canonical request/result/checkpoint/reducer profiles and journal authority are unchanged.
- Added pinned spatial and ingest result/chain digests plus same-batch version/forward-reference,
  retry/correction, retry-only checkpoint, one-record-over-populated-base and stale/foreign-plan
  atomicity coverage. Current maps, closure scans, snapshots and checkpoint decode remain
  full-memory; this is not T-20 closure or BM evidence.
- Added Decision 0035 and a privileged `graph-state-v1` preparation loader that retains only the
  exact positive/negative current-record closure plus current policy. Caller limits bound unique
  proofs, reference occurrences and logical proof bytes; the caller-owned page cache remains
  explicit and authenticated index work is reported.
- The resulting view owns no storage/coordinator/key capability and reuses the existing reducer in
  a pure consuming phase. Durable commit result digests match for a referenced assertion create
  and an existing assertion transition whose unchanged references must be proven.
- Exact proof budgets pass; one-less proof/reference/logical-byte budgets fail; an occupied
  correction ID is proven and rejected; stale roots fail; deletion and historical predicates fail
  as unsupported before root/page access. Review-found correction-ID and preallocation/work-limit
  holes were corrected before acceptance. Complete
  reverse/history proofs, root-delta generation from the partial view, live disk overlays and
  BM-01/BM-06 remain open.
- Added Decision 0036 and complete bounded family-3 history-prefix and family-7 reverse-owner
  proofs to disk preparation. Aggregate history/reverse entry and logical-byte limits combine with
  the frozen per-prefix result cap; decoded IDs, revisions, versions and reserved bytes fail closed.
- A two-operation retract-then-delete suppresses the authenticated base reverse dependency through
  the transaction overlay, and a historical `ReadView` replacement uses its complete prefix. Both
  match their actual durable commit digests. Zero entry budgets reject each one-entry proof, so no
  partial bucket is admitted as complete.
- All graph operation and precondition variants are now representable by the preparation proof.
  The live/recovered reducers remain full-memory; T-20 stays open.
- Added Decision 0037 and an exact authenticated family-1 metadata proof. The proof budget/report
  now covers its key/value and index lookup, and decoded revision, policy invariants and all family
  counts are checked against the admitted root descriptors.
- The pure proof result retains the exact root anchor, base metadata counters and current policy,
  then derives all eight bounded terminal-family delta sets without filesystem, coordinator, cache
  or key-vault access. Its record-plus-policy fixture matches the full-snapshot plan's delta
  count/bytes, publishes the proof-derived root through bounded merge, and admits it against the
  live reducer.
- Added Decision 0038 and the opt-in `ExternallyPreparedTransactionState` coordinator contract.
  Graph privately binds every prepared delta to its exact canonical request digest, then validates
  request bytes, absent blob inventory, next revision, scope, policy version and touched
  before-values before the ordinary durable journal append and live publication sequence.
- The end-to-end fixture rejects request/prepared substitution without revision advancement,
  durably commits the exact proof-prepared record-plus-policy transaction, returns its exact prior
  outcome for an idempotent retry, rejects a new-identity stale token, publishes the derived root,
  and reconstructs the same graph through ordinary journal replay after storage restart.
  Live publication, independent postcommit validation and recovery are still full-memory; no T-20
  or benchmark completion is claimed.
- Added Decision 0048 and a provisional exact-output visitor to the authenticated run merge. The
  graph publisher now validates proof-derived target counts and reproduces the existing canonical
  logical-state digest from the actual merged current/history/policy entries without acquiring the
  complete live snapshot. Only one record's history frames are retained, under a required caller
  budget.
- Storage proves the provisional output equals a later authenticated read. Graph unit coverage
  pins the streamed digest to the canonical reducer digest, rejects malformed secondary-family
  entries/counts and rejects a one-byte history budget.
  The end-to-end terminal-root fixture rejects wrong outcomes, undersized merge/history budgets and
  partial visibility across restart, then proves proof-only and full-state roots reconstruct to the
  same graph. This removes postcommit full-state validation, not the live reducer or recovery
  boundaries; T-20 and BM-01/BM-06 remain open.
- Added Decision 0049. Successful proof-only publication now returns the exact manifest-backed
  `DerivedGraphStateRoot` after terminal validation, rather than discarding it to a numeric receipt.
  The disk-preparation fixture carries that handle across filesystem/coordinator reopen and uses it
  for the next bounded proof without a complete-snapshot root rediscovery. Cold candidate admission
  still requires a resumable authenticated cursor and bounded predecessor lookup.
- Added Decision 0050. Storage, live coordinators and authenticated recovery now expose an opaque
  resumable run cursor whose terminal report requires exhaustion and full count/order/digest/length
  verification. A bounded two-pass predecessor proof resolves the greatest prefix key at or before
  an upper bound without consumer scan caps and assembles only the final value. Coverage interleaves
  cursor and exact reads, exercises fragmentation/corruption/early finish and proves a large prior
  value cannot exhaust a later small predecessor's result cap.
- Added Decision 0051. Live and recovery owners now stream-admit a cold `graph-state-v1` candidate
  into a capability-free `GraphDiskBase`. Admission checks history transitions, historical/current
  reference closure, current/history equality, every derived family, policy history and the
  canonical digest before returning the exact root/counts/policy handoff. The base drives existing
  bounded disk preparation without reconstructing complete graph maps.
- Added bounded exact-key reads and aggregate admission limits for history-group versions/bytes,
  exact/predecessor operations, proof page visits/result bytes and semantic reference comparisons.
  Iterator-oriented shared reference validation charges and resolves one requirement at a time,
  with no intermediate requirement vector. Exact-minus semantic/page/byte fixtures fail closed;
  targeted agent re-review found no remaining high- or medium-severity issue.
- Added Decision 0052 and `GraphDiskLiveState`. A consuming, opt-in coordinator transition proves
  the admitted base equals the complete reducer at the exact scope/revision/policy/logical-digest/
  certificate anchor while preserving journal, retry, transaction and blob-owner state.
- The warm reducer accepts only a disk-proof/terminal-plan bundle. After journal certification it
  hides the stale base and blocks distinct progress while exact idempotent retry remains available.
  A disjoint derived-index capability streams terminal publication without journal append access;
  a failed one-byte history bound leaves the plan pending, and adequate retry installs the exact
  next base and permits the next bounded proof.
- Added Decision 0053. Authenticated recovery retains one opaque final-journal transaction while
  independently validating the complete journal; the final coordinator open accepts only the
  exact admitted base or its one exact successor, revalidates the external graph preparation and
  durable outcome, and rejects mismatches, multi-revision gaps and concurrent journal advancement.
- Graph recovery decodes the captured canonical request internally and reconstructs only its
  caller-bounded current/history/reverse proof closure. Restart coverage reopens a pending suffix,
  preserves exact retry while blocking distinct progress, survives a bounded publication failure,
  installs the recovered root and then proves a ready-root restart. Focused agent review found no
  high- or medium-severity issue. Coordinator maps still replay from journal origin and bounded
  discovery/scrub remain T-20 work; no BM-01/BM-06 result is claimed.
- Added Decision 0039 and a synchronized runtime for every admitted authorized graph root. Its
  privileged report now includes cumulative decrypted-cache occupancy/events, completed authorized
  reads/raw operations, authenticated pages, fragments and logical result bytes. The counters are
  explicitly cardinality-sensitive, not consumer-visible telemetry.
- The authorized path accounts the storage statistics it previously discarded. Tests prove an
  empty handle, page reads on the first query, cache reuse without another page read, zeroizing
  clear with cumulative counters retained and a later miss/read. Current `ManageSchema` checks and
  issuer-instance-bound opaque roots reject unauthorized, revoked and foreign-coordinator access;
  outcome-uncertain coordinators reject diagnostics before possibly stale policy is consulted. This
  controls only USTE's cache; kernel/device caches and qualifying BM-01 remain unclaimed.
- The exact BM-01 engine workload was not launched while the reference host had only 5,835,172 KiB
  available and essentially all 8 GiB swap occupied, so it could not provide the accepted 24 GiB
  reservation. The Btrfs/NVMe volume still had 999 GiB free. This is a transient measurement-only
  blocker; no smaller workload is presented as qualifying evidence.
- Added Decision 0040 and the `engine-check` development verifier. It maps fixture IDs to exact
  scoped graph IDs, adds shared source Evidence, commits proposed then accepted relationships,
  publishes the encrypted index, restarts/replays and loads the persisted authorized root.
- At 20 entities/200 relationships, all 384 measured depth/class/direction queries composed from
  production one-hop reads exactly match the independent oracle after revision-4 recovery. The
  aggregate digest is `46f1bdb3138d6325e4c0f56b5fd3bbf5ff092d816e8a0f6c23acd15687b910b5`.
  The command is capped at 1,000 entities, emits `engine_benchmark:false`, and is not timing/RSS,
  Linux-filesystem, portable-recovery or BM-01 qualification evidence.
- Added Decision 0041 and bounded fixture construction to maximum-10,000-operation transactions.
  The exact qualifying profile has 212 durable revisions: one policy, 11 shared-Evidence/entity,
  100 relationship-create and 100 relationship-accept revisions. The accepted TSV and generated
  manifest pin this plan while continuing to emit `engine_benchmark:false`; it is not a completed
  qualifying run.
- Added Decision 0042 and Linux-only `linux-create`, `linux-resume` and `linux-open` phases using
  the production Btrfs adapter, OS entropy and portable Argon2id recovery. Password files use a
  no-follow descriptor and must be current-user-owned, singly linked, owner-only regular files with
  1–1024 exact bytes. Errors and JSON reports are content-free. Authenticated profile-digest,
  recovered/returned/final revision checks prevent same-frontier profile mislabeling and synthetic
  frontier reporting.
- A release-built 20/200 Btrfs smoke created revision 4 and one current encrypted root, reopened it
  in a new process, idempotently resumed every deterministic transaction without frontier advance,
  then reopened revision 4/root count 1 again. Aggregate phase times were 399/396/357/344 ms. A
  same-frontier 30/300 open was rejected by its authenticated profile binding. This is
  nonqualifying platform/recovery evidence, not BM-01 query timing or a host-cold result.
  This initial smoke covered only completed-frontier retry; Decision 0044 below adds small-profile
  interrupted-prefix process loss. Retries remain limited by the fixed 30-day outcome retention.
- Added Decision 0043, `bm01-oracle-summary-v1` and `bm01-result-v1`. A separate process now emits
  at most 256 KiB of canonical, content-free expectations bound to the engine mapping and measured
  corpus. The exact accepted profile pins 299 successful outputs, zero visit-limit outcomes, 85
  expected result-limit outcomes and summary digest
  `5e9cb81200b2016ab470419021561e0a304e1eb1d6610e0633b35925b27df402`.
- Added Linux `linux-query`: it independently reopens revision/profile/root, clears the USTE page
  cache before each authorized disk traversal, and requires exact visits/counts/logical bytes/
  digest or the exact typed limit refusal. Its content-free report includes outcome counts,
  diagnostic percentiles, RSS and authenticated cache/index counter deltas.
- A release-built 20/200 Btrfs correctness smoke matched all 384 outputs after revision-4 recovery.
  It reported an 891 ms query phase, 2.430/4.892/4.955 ms p50/p95/p99, 5,660 KiB current RSS and
  265,104 KiB peak RSS. The host caches were uncontrolled and graph state remained full-memory;
  `engine_benchmark:false` is preserved and no BM-01 pass is claimed.
- Added Decision 0044 and `linux-create-crash-probe`. It admits only incomplete frontiers, flushes a
  content-free readiness marker after the selected commit returns durable, and parks so an external
  harness can SIGKILL the exact process before ordinary portable recovery/resume.
- Release-built Btrfs children were SIGKILLed after revisions 1, 2 and 3, covering every incomplete
  phase of the 20/200 plan. Each fresh resume completed revision 4/root count 1 with zero repaired
  certificate-tail or ignored journal bytes, and each fresh open admitted that result. This is
  actual process-loss prefix evidence, not exact-scale duration, intra-transaction fault coverage
  or BM-06 qualification.
- Added Decision 0045 and `bm01-oracle-bundle-v1`, constructed outside the future sampler from one
  independent oracle. Bounded canonical sections contain 96 disjoint warm-up expectations and the
  unchanged 384 measured expectations. Exact scale pins warm-up outcomes at 74 successes, zero
  visit limits and 22 expected result limits, with bundle digest
  `d52869f24d635476f86374813e754be364d0d2df470544b71221c24b96145fae`.
- Added Decision 0046 and `linux-sample`. Exact scale has no duration/sample lowering controls: it
  validates one 96-query warm-up, then runs five complete measured windows of at least 60 seconds.
  Every query is correctness-checked as an empty-USTE-cache/retained-cache pair; only the engine
  call is timed, and success/refusal percentiles plus successful-work/index counters stay separate
  by cache state. Depth/topology and all-topology depth aggregates are both retained.
- A release-built 20/200 Btrfs sampler smoke validated the warm-up and one complete 768-execution
  paired round in 1,725 ms, with 5,788 KiB current and 264,916 KiB process-lifetime peak RSS. Empty
  versus retained USTE cache attribution reported 2,815 versus zero page reads. Host caches remained
  uncontrolled and graph state full-memory. The 30-second deadline is post-checked but cannot yet
  preempt a hung synchronous read, so no exact BM-01 budget evaluation or pass is claimed. Exact-run
  preflight found 6.2 GiB available RAM and 212 KiB free swap rather than the accepted 24-GiB
  reservation (with 998 GiB Btrfs free), so the qualifying runner was not launched or downscaled.
- Added Decision 0047's parent/worker deadline protocol. Flushed content-free markers bracket each
  engine call in one persistent worker; absence of finish after 30 seconds kills and reaps the exact
  child without sacrificing retained-cache pairing. A lifetime pipe terminates the child on parent
  death, and the parent validates typed report counts before it marks enforcement true. A real child
  timeout test exercises kill/reap.
- A release-built 20/200 Btrfs CLI smoke supervised all 864 warm-up/measured engine calls, completed
  the measured round in 1,758 ms, set deadline enforcement and post-checking true, and preserved
  2,815 empty-cache versus zero retained-cache page reads. This is still nonqualifying development
  evidence because host reservation and the full-memory graph boundary remain unresolved.
- Pinned `bm01-materialization-v1` with the exact accepted 100k-entity/1m-relationship uniform,
  distributed-hub and ring fixture, typed IDs, disjoint measured/warm-up query corpora and an
  independent adjacency-array BFS oracle. Golden digests are checked, but the manifest says
  `engine_benchmark: false`; no BM-01 timing or T-20 closure is claimed.
- Corrected review findings for normalizer-construction bypass, cap-before-copy behavior, malformed
  date-only inputs and open semantic reason combinations. Final review found no remaining high- or
  medium-severity T-45 finding; dependency unsafe remains explicitly inventoried.
- Modeled scoped entity/evidence/assertion/relationship records, evidence-backed relationship
  lifecycle, explicit correction preconditions, final-state reference closure, bitemporal reads,
  bounded reject/cascade/retract deletion and typed atomic failures.
- Executed every literal assertion transition plus the complete valid/invalid action matrix; added
  deterministic bounded entity and evidence-backed graph histories with an independently asserted
  claim/correction projection.
- Enforced the `limits-v1` 10,000-operation and conservative 100,000-reference request caps before
  state cloning. The operation wire-size cap remains with the later admission/codec boundary.
- Added independent literal R1 goldens for all assigned value tags and numeric/time/reference
  endpoints; malformed length/integer/version/tag/order/UTF-8/depth inputs fail closed.
- Added a fixed 262,144-node aggregate budget and fallible decode allocation after review found
  that per-container/wire limits alone permitted heap amplification. Adversarial nested input now
  rejects before the violating container reserves its declared children.
- Added isolated pinned `cargo-fuzz` targets for arbitrary malformed frames and generated valid
  trees, a reproducible runner and bounded CI smoke campaigns. The native fuzz runtime remains a
  test-only graph outside the safe-Rust product workspace.
- Read the complete current requirement set, both accepted product decisions and every
  current domain specification. No applicable USTE `AGENTS.md` exists.
- Preserved the clean starting worktree and created the requested branch from `be5f7a4`.
- Added Decisions 0003–0010 covering the v1 data/time, storage, cryptography/retention,
  content-worker, benchmark/limit, governance, spatial and physics profiles.
- Added literal R0 acceptance vectors and a safe-Rust standalone vector test.
- Added a storage-publication model that exhaustively tests every record cut and byte
  corruption for the proposed commit frontier.
- Resolved and compiled the pinned strict-profile dependency candidates; recorded a lockfile,
  native-link/license feasibility and the remaining unsafe/advisory review scope.
- Added the `synthetic-v1` Rust fixture-generator kernel with strict seed parsing, domain
  separation, bounded-memory emission and a pinned BLAKE3 golden digest.
- Added reproducible Fedora Kinoite/Toolbox setup and runnable synthetic commands.
- Added a committed dependency policy and ran cargo-deny 0.20.2: zero advisory, license or
  source errors; one documented duplicate-version warning in the combined parser graph.
- Materialized 19 deterministic JSON/PDF/OOXML/archive/image/audio/video fixtures with pinned
  byte counts and SHA-256 values; positive formats and hostile archive metadata are smoke-tested.
- Classified direct cryptographic/parser unsafe boundaries and selected-feature reachability;
  kept JPEG SIMD disabled and parser unsafe outside the privileged engine boundary.
- Verified unprivileged bubblewrap mount/network/user/PID isolation and in-worker address-space,
  CPU-time and descriptor limits on the reference runner; recorded cgroup/seccomp gaps.
- Completed the R0 cross-decision audit, corrected BM-04 from an inadmissible 20 GiB blob to
  a 12 GiB within-cap stream, and checked T-01–T-05/T-46–T-47 with linked evidence.
- Recorded the exact external distribution blocker without claiming disclosure verification or
  any implementation/release gate.

## Verification run

~~~text
rustc --edition=2024 --test tests/r0_vectors.rs -o /tmp/uste-r0-vectors
/tmp/uste-r0-vectors --nocapture
# 12 passed; 0 failed
rustc --edition=2024 --test experiments/storage-publication.rs -o /tmp/uste-storage-publication
/tmp/uste-storage-publication
# 4 passed; 0 failed
cargo build --manifest-path experiments/dependency-audit/Cargo.toml --locked --offline
# success
cargo test --manifest-path experiments/fixture-generator/Cargo.toml --locked --offline
# 4 passed; 0 failed
cargo test --manifest-path experiments/content-fixtures/Cargo.toml --locked --offline
# 4 passed; 0 failed
cargo-deny ... --frozen check all --show-stats
# both lockfiles: 0 errors; parser graph: one documented duplicate warning
cargo fmt ... -- --check; rustfmt --check ...
# success
python3 scripts/check_task_graph.py
# task_graph=ok tasks=62 local_implementation_gate=T-07 distribution_gate=T-62 release_gate=T-44
bash scripts/check.sh
# workspace format/clippy/test/doc pass; 265 workspace tests including 77 uste-storage, 13
# uste-crypto, 43 uste-graph, 4 uste-ingest, 34 uste-spatial, 23 uste-types, 15 uste-time,
# 11 uste-replay, 14 uste-testkit, 4 uste-policy and 27 uste-txn tests;
# docs=ok (116 links, 113 active IDs, 146 definitions); task graph=ok; R0/content/fixture tests
# and 31 isolated T-20 fixture/engine/Linux-runner tests pass; two exact-profile oracle tests are
# intentionally ignored in debug and executed under release-profile acceptance commands
cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline \
  oracle_summary::tests::qualifying_summary_outcomes_and_digest_are_golden -- --ignored --exact
# 1 passed in 5.40 s; exact 299/0/85 outcome split, accepted digest and round-trip
cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline \
  oracle_bundle::tests::qualifying_bundle_outcomes_and_digests_are_golden -- --ignored --exact
# 1 passed in 7.84 s; exact warm-up/measured splits, all accepted digests and round-trip
cargo test -p uste-graph --test disk_index bounded_disk_preparation_supports_current_history_reverse_and_stale_roots -- --exact
# 1 passed; exact proof-prepared commit, retry/stale/mismatch checks, root publication and restart
cargo test -p uste-graph --test disk_index
# 5 passed; 0 failed
cargo test -p uste-txn --all-targets
# 27 passed; 0 failed
cargo test -p uste-graph --all-targets --locked --offline
# 43 passed; 0 failed
cargo clippy -p uste-graph --all-targets --locked --offline -- -D warnings
# passed
cargo test -p uste-memory --all-targets --locked --offline
# 2 passed; 0 failed
cargo test -p uste-spatial --all-targets --locked
# 34 passed; 0 failed
cargo test -p uste-types --test spatial_primitives --locked
# 4 passed; 0 failed
cargo test -p uste-graph --test spatial_replay --locked
# 1 passed; 0 failed
cargo clippy -p uste-spatial --all-targets --locked -- -D warnings
# passed
/tmp/uste-t09-tools/bin/cargo-deny --locked check advisories licenses sources bans
# advisories ok, bans ok, licenses ok, sources ok
CARGO_DENY_BIN=/tmp/uste-t09-tools/bin/cargo-deny bash scripts/check_supply_chain.sh
# all five lockfiles including rustix 1.1.5: zero advisory/license/source errors;
# documented miniz_oxide warnings only
cargo +nightly-2026-08-01 fuzz run decode_v1 -- \
  -max_total_time=60 -seed=1592639215 -max_len=4096 -rss_limit_mb=1024 -print_final_stats=1
# 14,518,800 executions; 61 seconds; 513 MiB peak RSS; no crash artifact
cargo +nightly-2026-08-01 fuzz run structured_v1 -- \
  -max_total_time=60 -seed=1592639215 -max_len=4096 -rss_limit_mb=1024 -print_final_stats=1
# 1,610,094 executions; 61 seconds; 554 MiB peak RSS; no crash artifact
~~~

Reference runner observed: Fedora 44, kernel 7.1.10, Btrfs 7.1/local NVMe, Intel i9-13900KF,
64 GiB RAM, Rust/Cargo 1.95.0. Pinned lockfiles now pass cargo-deny advisory/license/source
policy checks. The T-15 12 GiB component measurement is recorded, but BM-04's throughput target and
remaining mixed workload have not passed.

## Limitations and external prerequisites

- T-62 requires the repository owner/administrator to enable and harmlessly test GitHub private
  vulnerability reporting. The previously observed GitHub CLI token is invalid and was not
  retried; SSH Git access is not administrative access. This blocks executable distribution and
  T-44, but it does not block local implementation, integration or artifact preparation.
- R0 decisions do not provide implementation, achieved benchmark performance or production
  security evidence. Transitive unsafe validation, the T-23 supervisor and actual BM results
  remain later-gate work.
- The canonical kernel through T-49 plus T-20's encrypted current-graph disk projection is
  implemented, but there is no database executable or production qualification. Privileged raw
  disk APIs remain separate from the new authorization-preserving current-graph consumer path.
  Checkpoint discovery and seeded open each authenticate the journal, and publication can now stream
  with a one-new-payload-chunk buffer, and checkpoint transport can recover through a bounded chunk
  stream. Ordinary graph prepare/publish is now change-bounded, but checkpoint decoding, explicit
  snapshots remain full-state boundaries. Ordinary composite writes now retain request-sized
  deltas, but graph-only closure still scans the complete borrowed catalog/job map. Reverse dependencies add an
  in-memory structure with no accepted aggregate/per-target fanout cap.
  The bounded scratch merge and graph terminal planner can rewrite and cross-check all eight
  families for one revision without collecting base runs. Proof-derived publication now validates
  actual merged outputs and the canonical digest with one caller-bounded history bucket rather than
  scanning the complete reducer. Cold semantic admission now returns a bounded `GraphDiskBase`
  without complete graph-map reconstruction. The warm live reducer now advances that base with
  one bounded pending plan and no complete graph map. Journal-anchored restart recovers either that
  ready base or one exact pending suffix without `GraphState`. Decision 0059 bounds graph manifest
  discovery before run scanning; the coordinator's retry/transaction/blob-owner maps still replay
  from journal origin into memory. The explicit-I/O preparation proof now supports
  all graph operation/precondition variants with bounded current/history/reverse proofs and now
  feeds both the authoritative coordinator commit and a separately published terminal-root plan.
  Recovery of those coordinator maps remains fully memory-resident.
  BM-01/BM-06 have not run. Graph policy is
  durable; the trusted adapter must supply its exact
  current copy at authorized open. The oracle
  intentionally scans records and is not scalable. The fuzz runner requires nightly Rust plus a C++
  compiler, both confined to development tooling.
- T-45 normalizes timestamps and preserves their provenance but does not add temporal indexes,
  content-adapter extraction, clock-drift estimation or leap/TAI/GPS conversion tables. T-21 and
  T-24 own those layers. The admitted named-zone behavior is pinned to embedded TZDB 2026c.
- T-48's transform reference remains an opaque same-scope version binding. T-49 now proves its
  target exists as an active graph Entity, but T-50 still owns transform schema/version semantics
  and evaluation. The 64 MiB spatial catalog and T-49 in-memory job ledger are correctness
  baselines; Decision 0034 bounds their ordinary preparation deltas but does not make them
  disk-backed. T-59 owns native disk indexing and the one-million-item BM-10 workload.
- T-49 structural maxima are not simultaneous capacity claims. Its accepted-row cursor has only
  `Open`/`Completed` states; no rejected-row advancement/report, mapping execution, CSV/JSON parser,
  public authorized preview, item/price fixture, throughput/RSS/concurrency or platform-crash result
  is claimed. T-54 owns those tooling and provenance extensions.
- T-13 local acceptance is complete on the reference Btrfs runner and the independently identified
  ext4 mount `/var/mnt/archive_vault` (`/dev/sda1`). These SIGKILL tests do not simulate controller
  cache loss or actual power loss. The certificate log fails closed at 1 GiB pending later
  T-35 maintenance/rollover design.

## Next dependency-permitted work

T-63–T-68 and M1 are complete at implementation `b9689f3`, qualified by Decision 0058 and the
exact-version consumer handoff. The resumed audit confirmed that commit's lockfile digest and
unchanged pilot sources, and the complete gate at `8885da4` included the pilot recovery tests.
The two interruption-pending commits `8885da4` and `808f2fd` are now pushed to origin.
Continue the preserved full-product work below. Large benchmarks still require adequate host
headroom; the resumed preflight showed 4.1 GiB available RAM and 112 KiB free swap.

Continue T-20 by moving coordinator retry, transaction and blob-owner metadata off the full
in-memory journal replay path. The v1 metadata root has retry and blob-owner families but no
transaction-ID lookup family; introduce and validate that disk lookup contract before replacing
the live maps. Explicit-I/O outcome APIs and journal-prefix validation must preserve exact retry,
transaction collision and first-owner semantics. Extend bounded
suffix recovery beyond one revision only with an authenticated streaming design. Run the exact five-sample BM-01 campaign under the accepted host
24 GiB reservation once the implementation boundary is honest, and define/run BM-06's
10-million-event protocol. Then return to T-19's
remaining VT gaps and BM-02/BM-04 work; no failed or absent benchmark is accepted as passing.
T-62 remains independent and must not be represented as complete without owner-administered
evidence. BM-04 performance optimization remains later acceptance work and is not silently treated
as passed.

Persistent-goal continuity: the existing goal remains recorded as blocked by the service following
the capacity interruption. Its API exposes completion/blocking but no resume operation; creating a
replacement is rejected because the existing goal is unfinished. The user's resumed implementation
authorization remains in force. This status is a control-plane limitation, not project completion
or a blocker to local work; preserve the original goal and use this handoff for continuity.
