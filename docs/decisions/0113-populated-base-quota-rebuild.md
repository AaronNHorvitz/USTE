# Decision 0113 — Bounded populated-base quota rebuild

Date: 2026-09-19

Status: accepted and locally verified. Full regression evidence is recorded in PROGRESS.md.
T-20 remains open.

Extend Decision 0111 with an explicit privileged rebuild from the already admitted primary
first-owner ledger at a fully rebased, certified coordinator frontier. No pending transactions
or owner overlays are allowed. This supports populated bases and replacement of an existing
derived quota projection without replaying a memory-resident owner map or inventing a transaction.
The primary ledger remains authoritative; the quota schema and historical stage ordering do not
change. Existing authorized facade defaults and the pinned M1 interface remain unchanged.

`rebuild_blob_usage_index` streams primary owners in caller-bounded batches (1–4096 owners),
reorders only each batch by principal/owner, and merges it into private current-frontier runs.
Private recovery-stage read handles do not publish root slots. Before terminal publication, a
separate complete validation proves every projected owner/reference/principal against the primary
ledger, exact per-principal totals, namespace bytes and cardinalities. The admitted in-memory
projection is replaced only after successful durable publication. Failures can leave unreferenced
scratch runs or an independently admissible published derived root, never an authoritative commit.
Restart/retry reconstructs from the same primary ledger; this is not physical erasure or migration.

Admission covers source traversal, per-family merge, independent validation, total owner count,
batch count and cumulative logical merge-output bytes. A conservative checked lower bound rejects
impossible output budgets before construction; each batch checks its exact output before writes.
The logical output charge is `37 + 48 * principals + 128 * owners` per batch, including all repeated
rewritten entries. Empty construction charges 37 bytes and one batch. The report identifies source
scan work and logical output, not encrypted/device bytes, all root/certificate validation I/O or
maximum RSS. Scratch sorting has at most 4096 owners plus bounded index/cache buffers; index-v1
still rewrites immutable families, so total construction can be quadratic in owners/batch size.
This bounded path does not establish larger-than-memory performance or BM-01/BM-06 qualification.

Tests cover inclusive batch boundaries, exact/one-short output budgets, invalid pre-I/O admissions,
private construction followed by refused terminal admission, literal multi-batch principal totals,
empty/zero-byte owners, cold disk-certificate admission, repeat replacement without double charging,
and terminal-certificate corruption after private construction. All 210 observed I/O fault attempts
are exercised with error/crash-before/crash-after and exact authoritative restart. Three optional
NotFound operations consume a scheduled CrashAfter without a successful operation or actual crash;
these are not claimed as recovery from injected crashes. Existing quota publication, authenticated
false-cache and cold admission tests remain active.
