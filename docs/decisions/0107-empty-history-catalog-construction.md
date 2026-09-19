# Decision 0107 — Empty-history catalog construction

Date: 2026-09-19

Status: implemented and locally verified. T-20 remains open.

Use the journal's maintained committed reference-binding count as a construction hint when
rebuilding the optional storage blob catalog. When it is zero, construct the metadata-only
candidate at the certified frontier directly, reading that terminal group rather than repeatedly
proving every empty group from the beginning. Keep the existing forward builder unchanged for
nonzero histories. No new format, root authority, resident history collection or public API.

The hint does not admit the candidate. After staging, the existing independent admission still
authenticates every group in the complete prefix using Decision 0105's reverse scan. Any inventory
in that prefix contradicts the zero-binding candidate and fails before root publication. Earlier
certificate/group corruption also fails. A caller cannot request a small suffix budget instead:
the original whole-prefix group ceiling is checked before construction, and the independent
admission still consumes its full certificate/group byte allowance. Resource refusal leaves at
most optional unpublished scratch runs; no cleanup or authority migration is introduced.

Reports count actual work. Empty-history construction discovers one terminal group and stages
one 68-logical-byte metadata record; its terminal certificate proof is still charged separately.
The admission report still contains every prefix group. Configured range byte allowances remain
independent between discovery and admission, as in Decision 0098. Nonempty construction retains
its certificate-proof and immutable-family rewrite amplification. This optimization does not
qualify BM-01/BM-06, remove the native development cap or imply larger-than-memory performance.

Require exact/one-short budgets at a five-revision empty frontier, rejection of a deliberately
false zero hint on nonempty history, corruption of earlier certificates and group ciphertext,
and selected I/O-error/crash-before/crash-after boundaries with exact-frontier restart and
independent validation of any surviving root. Existing nonempty catalog and cold-recovery
regressions remain mandatory. M1's pinned consumer interface and release gates are unchanged.

All five focused tests pass in the unoptimized test profile, including 45 selected fault attempts.
The full 121-test storage suite and graph/coordinator/native regressions pass. The broad core gate
uses optimization level 1 with debug assertions and overflow checks explicitly enabled, one job
and one test thread under the same 3G/4G/512M scope. No fault case or acceptance threshold is
removed. Exact commands/results and the initial corrected fixture-layout assumption are in PROGRESS.
