# Decision 0267: Hash-indexed exact-LRU positive lookup cache

Date: 2026-09-23

Status: Accepted for the development implementation; no benchmark qualification or default change.

Decision 0266 measured retained four-hop p99 at 374.535 ms in the standalone lane with zero
retained-half adapter reads, and its stack profile placed 63% of retained-half samples inside
`LookupCache::get_with`. The positive lookup partition indexed resident values as
`BTreeMap<IdentityKey, BTreeMap<LogicalKey, Value>>` with recency in a separate
`BTreeMap<u128, Arc<EntryKey>>`. A hit therefore paid a 175-byte identity comparison chain, an
ordered search over tens of thousands of `Arc`-indirected keys, a recency removal and insertion,
and two more `get_mut` descents; every adjacency visit performs two such hits.

Index resident values in a `HashMap` keyed by the interned identity index followed by the logical
key bytes, and keep recency in an intrusive doubly linked list over a slot vector. Identities are
interned once per distinct authenticated identity with a live count and released with their last
value. Probes for ordinary keys use a zeroized stack buffer; only keys longer than 60 bytes
allocate. Slots and identity indexes are reused through free lists.

The eviction order is exact least-recently-used, identical to the earlier stamp-ordered map: a
promoted hit moves to the head and eviction pops the tail. Hit, miss, eviction and oversized-bypass
counting, the refusal of a hit whose recorded proof work exceeds the caller's limits (counted,
never promoted), duplicate-insert refusal, the logical charge of 512 + 175 + key + value bytes,
the 4,096-byte fixed allowance, `clear` semantics and counter-overflow diagnostics are unchanged.
The unreachable `u128` recency-clock overflow check no longer exists because there is no clock.
Resident plaintext and keys remain zeroized on drop. Persistent bytes, graph results,
authorization, configuration and cache budgets are unchanged; this adds no cache or retained
plaintext.

Regression coverage now walks the intrusive list in both directions, reconciles free lists, live
identity counts, charges and hash-index membership, checks identity interning and release with
index reuse, the inline/heap probe boundary, every identity byte, variable-length keys up to the
maximum, the 10,000-step independent LRU reference, refused hits not promoting, bypass, clear and
overflow diagnostics.

The exact final tree ran the complete `bash scripts/check.sh` gate in the lane with three Cargo
jobs and one test thread: formatting, strict workspace Clippy, **793 workspace tests**, rustdoc
with warnings denied, experiment manifest formatting, docs and task-graph checks, R0 vectors,
the storage-publication model and the isolated experiment builds/tests all passed. The gate
aborted at its final test binary after 693 minutes 58 seconds of wall time: the legacy BM-06
process test `bm06_native_resumes_authenticated_prefixes_without_duplicate_commits` timed out
waiting 30 seconds for the debug-built `bm06-linux-create-crash-probe` child to report its
durable-prefix marker. Direct measurement of that child in the same debug profile shows the
marker appears after 4.9 / 10.1 / 25.4 / 57.9 / 58.7 seconds for the test's five pause revisions
on this tree and after 25.7 / 58.5 seconds for revisions 50 and 100 on an unchanged `2263dc5`
build, so the failure is the lane's unoptimized three-CPU environment exceeding a fixed test
deadline, not this change: the legacy v1 disk path has no reference to the packed page cache.
The skipped final gate step (strict all-target Clippy of `experiments/t20-bench`) was then run
separately and passed. The failing result and the complete gate log are retained in the lane's
private log store; the test deadline was not raised.
