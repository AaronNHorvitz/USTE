# Decision 0273: Hash-indexed exact-LRU complete-range cache

Date: 2026-09-24

Status: Accepted for the development implementation; no benchmark qualification or default change.

Decision 0267 replaced the positive lookup partition's ordered-map index with a hash index over
interned identities and an intrusive exact least-recently-used list. The complete-range partition
kept the same ordered-map pattern: every hit and miss built a fresh heap key of at least 181 bytes
(the complete 175-byte authenticated identity, direction and both bounds), searched a
`BTreeMap` of `Arc`-indirected keys, and on a hit removed and reinserted a `BTreeMap<u128>`
recency entry plus one more descent to restamp the value. Every adjacency read performs one such
range probe, so the cost is paid once per frontier entity per hop.

Apply the Decision 0267 structure to the range partition. Identities are interned once with a
live count and released with their last range. The hash key is the four-byte interned index,
direction, and the length-prefixed bounds; hits probe it without the 175-byte identity. Slots
retain their complete hash key so eviction unindexes without re-encoding, and slot and identity
indexes are reused through free lists.

The logical accounting is unchanged by construction. The earlier charge used the reserved
capacity of the owned key, which was always 175 + 6 + lower + upper bytes, including the six
framing bytes when the upper bound is absent; the charge helper reproduces exactly that value,
and each retained entry is still charged 128 bytes plus its exact key and value bytes. Bypass,
eviction order (exact LRU, tail first), hit/miss/eviction/evicted-byte counters, the high-water
mark, refusal of a hit whose recorded work exceeds the caller's limits (counted, never promoted),
map-error ordering, duplicate-insert refusal, bound validation and `clear` semantics are
unchanged. The unreachable `u128` recency-clock overflow check no longer exists because there is
no clock. Persistent bytes, graph results, authorization, configuration and cache budgets are
unchanged; this adds no cache or retained plaintext.

Regression coverage walks the intrusive list in both directions, reconciles free lists, live
identity counts, charges and hash-index membership, and checks that each slot's key begins with
its interned index. A new case pins the exact charge for bounded and unbounded upper ranges,
distinguishes direction under one identity, and evicts every range of two identities with a
sized filler so both identities are released. Existing cases cover identity/direction/bound
binding, every limit refusal without promotion, LRU pressure with exact evicted bytes, bypass,
invalid results, `clear` and counter-overflow diagnostics.

Verification of the combined Decision 0271–0274 tree, with one test thread and one Cargo job
under the shared heavy-work reservation: the plain `bash scripts/check.sh` run passed formatting,
strict workspace Clippy, 794 tests across the workspace and the fixture-generator and
content-fixtures experiments, rustdoc with warnings denied, experiment manifest formatting, the
docs and task-graph checks, the R0 vectors, the storage-publication model and the
dependency-audit build. The run was then killed by an unrelated session authentication failure
inside the isolated t20-bench tests and has no final result. On the byte-identical tree the two
remaining steps were rerun: the t20-bench tests with `--no-fail-fast` (137 minutes) passed 141
tests with five explicit ignores and failed one, the same legacy
`recovery_process::bm06_native_resumes_authenticated_prefixes_without_duplicate_commits`
30-second child-marker timeout recorded under Decision 0267, which is environmental in this lane
and occurs on the unchanged `2263dc5` binary too; strict all-target t20-bench Clippy passed. The
gate is therefore not green in this lane, and the failure is retained rather than its deadline
raised.
