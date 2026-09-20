# Decision 0179: Buffered cold graph admission

Date: 2026-09-19

Status: Accepted

Extend Decision 0178 through namespace/target-bound maintenance into an opt-in
`admit_packed_graph_base_buffered` operation. It performs the same eight full canonical
validations, semantic relationship/history/policy checks, metadata count reconciliation and
streamed v1 digest computation as existing cold admission. No graph-wide map or persisted
receipt shortcut replaces source-backed validation.

Canonical scans use fresh bounded caches sequentially, dropping each before the next family.
Semantic validation then starts another fresh cache of the same caller-selected budget, shared
only within that validation's sequential scans and interleaved exact/predecessor proofs. It
uses the existing checked maintenance reader: scope, target revision, live certificate owner,
key session and sticky cursor failures are unchanged. No externally warmed cache is accepted
or returned. Subsequent admission therefore rereads ciphertext; completed admission still
does not promise immunity to concurrent/later changes.

Canonical and semantic logical proof reports remain identical to uncached reports. Cache hits
consume the same page/encoded-byte budgets. A separate fixed-size cache report records each
canonical phase and the semantic phase; their resident byte counts are not summed as a
simultaneous reservation. Actual miss/decrypt/adapter work remains separate from logical proof
work. The supplied budget is logical cache accounting, not an RSS or physical-I/O guarantee.

Retain the uncached API. Exercise both paths against the same frozen-reference, all 24
independent limit refusals, eight authenticated false-graph variants, source/owner binding,
late-corruption, empty-policy and full actual-read fault/restart cases. Additional tests compare
all proof counters, v1 digest and actual read counts at one-page and larger budgets, repeat
admission to prove fresh reads and bound every cache phase. Native harness adoption and a new
pinned performance measurement remain separate work. No benchmark or task is qualified here.
