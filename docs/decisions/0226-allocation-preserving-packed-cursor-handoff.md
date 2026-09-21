# Decision 0226: Allocation-preserving packed cursor handoff

Date: 2026-09-21

Status: Implemented and locally verified; native performance observation pending.

D0225 leaves the retained traversal latency gate open. Packed secondary scans already return an
owned key and value for every candidate, but graph expansion copied both buffers into a second
`IndexScanEntry` before applying the shared traversal semantics. Transfer those existing
allocations into the scan entry instead of allocating and copying them again.

Add a consuming `PackedCursorEntry::into_scan_entry` conversion. It takes the key and value vectors
out of their zeroizing wrappers and leaves empty wrappers to drop. This does not widen plaintext
lifetime or reduce erasure relative to the previous graph path: the old code immediately copied
both buffers into ordinary, non-zeroizing `IndexScanEntry` vectors that lived for the same scan,
then zeroized only the redundant originals. The new path retains one ordinary output allocation
instead of two simultaneous copies. Callers that do not consume an entry keep the existing
zeroizing-drop behavior.

Keep entry validation before conversion. Cursor proof work, returned-byte accounting, candidate
ordering, traversal authorization, corruption checks, aggregate budgets, cache behavior and all
on-disk/public formats remain unchanged. The consuming conversion is additive; existing borrowed
key/value accessors remain available.

The exhaustive all-byte/prefix cursor test now proves the converted key and value retain the exact
allocation pointers while continuing to match the independent sorted reference. Both page-only and
positive-cache expansion exact-work/narrow-limit tests pass. The final full workspace command
passed 757 tests, strict Clippy and warnings-denied documentation. Its principal long suites were
graph disk 128/21,561.80 s, replay checkpoint 50/4,383.91 s, storage unit 244/1,327.68 s and
transaction integration 118/4,769.96 s. The standalone native release gate passed 136 active tests
with five unchanged opt-in ignores and strict Clippy.

Both heavy gates ran serially with one Cargo job/thread, the inherited 4 GiB process address-space
limit and the verified enclosing 5 GiB high / 6 GiB maximum / 512 MiB swap-maximum scope. The
cumulative shared scope peaked at 5,372,850,176 bytes and 143,224,832 bytes swap, recorded 22,368
soft-limit events, and recorded zero maximum-limit, OOM or CPU-throttle events. These shared figures
are not workload RSS. No latency or T-20 claim follows until the unchanged supervised retained
fixture is measured separately.
