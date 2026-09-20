# Decision 0176 — Packed BM-01 vault-work reporting

Date: 2026-09-19

Status: implemented and locally verified. Nonqualifying development instrumentation only.

Connect Decision 0175's trusted per-vault diagnostics to native packed BM-01 query and sampling
reports. Snapshot the selected owner's counters before and after each engine query, outside the
latency interval. Check subtraction and accumulation for all four counters without partial updates.
The operator harness owns the raw coordinator; consumer reader interfaces expose no vault totals.
Actual queries and cache diagnostics retain their existing authorization. Report each empty/retained
population separately, alongside—not in place of—cache and adapter work.
Expected typed-limit queries include their actual decrypt work. Unexpected engine/oracle failures
still fail the campaign; instrumentation never converts failure to an expected limit or success.

Setup reports explicitly cover only the last cold-open vault, including its admission work.
Earlier vaults used during create/resume/rebuild are not silently included. Warm-up and measured
work are deltas on that same owner. Single-pass correctness queries get a separate query delta;
terminal phase reports disclose last-owner-only totals. The schema now labels authentication work
`partial-single-owner-vault-decrypt`. Complete authenticated I/O remains false: key unwrap,
pre-decrypt structural refusals, discarded owners and filesystem/device traffic are not these
counters. Finalization binds the new accounting label and setup scope without upgrading qualification.

No query plan, fixture, digest domain, cache size, deadline, iteration count, admission ceiling,
benchmark threshold or source-store content changes. Tests check every delta/accumulation field for
rollback and overflow, exact packed framing ratios, zero extra decrypt work for retained small-fixture
queries, separate positive setup/warm-up work and unchanged independently computed result digests.

Full standalone regression passes 104 active tests with two existing ignored campaigns, strict
Clippy and format/docs/task checks. PROGRESS records exact commands, resource observations and the
unchanged 664-test core baseline. No benchmark acceptance target is lowered or declared passed.
