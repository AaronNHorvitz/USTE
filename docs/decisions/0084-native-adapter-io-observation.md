# Decision 0084 — Native adapter I/O observation

Date: 2026-09-18

Status: T-20 partial implementation; authenticated-index accounting and qualification remain open.

Wrap only the native disk experiment's filesystem in a fixed-size observer. Retain fourteen
operation call/failure counters and requested/returned read/write byte totals, not paths, file
names, payloads or an operation log. Forward each adapter operation once, preserve its exact
result, and keep ownership guards, handle types, ordering, retry policy and durability unchanged.
EOF and short operations count their actual returned bytes; interrupted/failed operations count
as failed calls, not successes or implicit retries. Counter overflow or an adapter over-report
invalidates measurements, never substitutes a storage result. Report/delta/aggregate operations
reject invalid or decreasing counters and use checked arithmetic.

Collect adapter-lifetime setup totals, query-only deltas, separate warm-up deltas, and paired
empty/retained per-sample deltas. The existing timing encloses actual query execution, including
observer work inside adapter calls. Snapshot subtraction and JSON construction occur outside each
timed query. No counter resets or unbounded history retention are needed. Reports are privileged
synthetic benchmark diagnostics and contain no consumer-visible hidden-candidate fields.

These are filesystem-adapter calls after receiving the opened root capability. They are not
physical-device traffic, internal syscall counts, handle-drop/close counts, credential/oracle-file
reads or complete authenticated-index statistics. Cache hits are still reported separately.
Keep `authenticated_io_accounting:not-measured` and explicit false completeness/device flags;
never relabel returned ciphertext bytes as successfully authenticated pages. Existing workload,
oracle, timeout, memory-residency and nonqualification disclosures remain intact.

Tests cover all fourteen forwarded capabilities, lock lifetime, exact operation counts, short
reads/writes, EOF, unchanged interrupted errors and over-reported counts. Overflow must invalidate
reports while actual writes/syncs still succeed. Native query and supervised CLI tests verify
nonzero setup/empty-cache adapter reads, zero query writes and zero retained-cache returned bytes
on the small fixture. Existing process-loss/recovery and independent oracle assertions remain.
This adds measurement capability, not a BM-01/BM-06 pass or permission to lift the development cap.

Review also corrects Decision 0083's journal byte formula: `visit_committed_range` charges the
4,161-byte certificate in addition to each group envelope. Prefix/suffix allowances now include
both. A maximum-plaintext real encrypted group test rejects the old group-only allowance and
exact-total-minus-one before callbacks, then succeeds at the exact 16,785,538-byte total. Small
fixture success had not exercised this worst-case boundary; no workload threshold is lowered.
