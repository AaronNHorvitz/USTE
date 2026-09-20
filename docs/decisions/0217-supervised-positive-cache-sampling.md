# Decision 0217: Supervised positive-cache sampling and separate cache work

Date: 2026-09-20

Status: Implemented and locally verified; no larger sampling measurement or qualification.

Add separately named positive-cache sampling/worker commands using D0216's explicit 64 MiB
total (48 MiB pages, 16 MiB positive lookups). Preserve existing commands and D0046/D0047/D0174's
frozen oracle, 96 disjoint warm-ups, empty/identical-retained pairing, complete rounds, latency
populations, sample counts, execution bounds and 30-second owned-worker deadline. No lowering flags.

Use a distinct positive-sampling schema and require the parent to validate its exact cache
configuration and separate result-cache work, alongside every existing packed sampling check.
Retain the lifetime-pipe watchdog, bounded protocol, exact child kill/reap and the parent's sole
authority to set deadline enforcement true. Refuse cross-mode, missing or inconsistent reports.

Capture full cache snapshots outside timed engine execution. Keep existing page hit/miss/eviction
observations separate from positive hits/misses/evictions/oversized bypasses. Total cache residency
already includes the result partition; never sum it twice. Accumulate checked monotone deltas
independently for empty and retained states, retaining latest bounded gauges. Reject budget/mode
changes, counter underflow/overflow and absent observations without partial accumulator mutation.
Disabled lookup caching reports absent work, not fabricated positive-cache counters.
The parent also reconciles warm-up plus paired-state observations with the final reader totals
and requires final residency to match the last retained observation.

Require arithmetic/reference/failure tests, strict parent report validation, real owned-child
deadline cleanup, and a separate-process 20/200 comparison preserving the oracle digest, all
96+768 executions, 32 latency groups, authority and current authorization. Larger observations
remain separate from implementation verification. No complete I/O, physical erasure, production,
M1 migration or qualifying BM-01/BM-06 claim follows; all accepted gates and targets remain open.

Native release regression passes 131 active tests with five unchanged opt-in ignores, strict
Clippy, formatting and documentation/task checks. Both separate-process modes preserve every
oracle and latency-population assertion; the positive mode reconciles the complete cache ledger.
Arithmetic failures are atomic and the parent rejects missing/mismatched configuration, overflow,
false accounting/deadline claims and cross-mode reports. PROGRESS records exact verification and
resource evidence. D0216's measured 16 MiB partition regression remains valid: page-only stays default.
