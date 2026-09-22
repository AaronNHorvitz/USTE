# Decision 0239: Small-range-cache sampling profile

Date: 2026-09-22

Status: Implemented and locally verified; development observation pending.

Add a distinct supervised `linux-packed-wide-small-range-sample` parent/worker command after
Decision 0238 rejected the 64/64/128 MiB page/lookup/range split. Preserve the 256 MiB total while
allocating 112 MiB to pages, restoring 128 MiB to positive lookups and allocating 16 MiB to complete
ranges. The range allocation is over 1.9 times D0238's 8,789,091-byte terminal residency without
displacing the previously non-evicting lookup partition. This sizing is an experiment, not a new
default or performance conclusion.

The worker emits the distinct
`bm01-linux-packed-wide-small-range-sampling-v1` schema and
`packed-pages-positive-lookups-small-ranges-256m-v1` cache profile. Its range observations use the
same checked warm-up and empty/retained ledger as Decision 0237. The parent owns the worker and
30-second per-query deadline, requires the exact 112/128/16 MiB partition, conserves total
accounting, reconciles terminal lookup/range counters and rejects the old wide-range schema or a
relabelled page budget. Existing page-only, positive-lookup and 64/64/128 range commands, schemas
and defaults are unchanged.

Focused cache-configuration tests passed three cases and focused supervisor-ledger tests passed
seven cases. The complete optimized standalone T-20 gate passed **141 active tests with five
unchanged opt-in ignores**. Strict all-target Clippy passed with warnings denied. Verification used
one Cargo job, one test thread, offline locked dependencies and the 4 GiB process address-space
limit under the verified enclosing 5/6 GiB high/max and 512 MiB swap caps; maximum-limit and OOM
counters remained zero.

No development sample ran and no performance, T-20, M1 or qualification claim follows. A subsequent
observation must use the unchanged retained fixture/oracle, preserve semantic outputs and separately
report the changed physical/cache work. Keep the 64 MiB page-only default and every qualification
prerequisite.
