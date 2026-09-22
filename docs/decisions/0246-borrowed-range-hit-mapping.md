# Decision 0246: Borrowed range-hit mapping

Date: 2026-09-22

Status: Implemented and locally verified; development observation pending.

Decision 0245 established that the retained half of the 96/128/32 MiB profile performs zero
filesystem-adapter reads while four-hop p99 remains 547.486501 ms. Inspection found that every
range-cache hit still allocated and copied each complete cached key/value into a
`PackedCursorEntry`; packed graph expansion immediately reduced those buffers to fixed 16-byte
record and neighbor identifiers.

Add a mapped complete-range read alongside the existing owned-range API. The range cache admits the
stored proof-work against every caller-independent cursor limit before invoking the mapper. The
journal still validates the live certificate owner, unlocked key session, authenticated tree,
direction and bounds first. Mapped values must be owned and cannot borrow resident plaintext after
the call. Cache misses retain the same complete authenticated range before mapping; the existing
owned API and all persisted formats remain unchanged.

Packed graph expansion now maps borrowed key/value slices directly into `ExpansionScanEntry`.
Warm hits therefore avoid allocating and copying intermediary key and value buffers. The compact
result vector, graph validation, aggregate admission, authorization filtering, cancellation and
range/page/lookup accounting remain unchanged. The uncached fallback uses the same slice mapper so
both paths retain one structural validator.

The storage regression proves an exact mapped warm result and report with zero adapter reads and
zero new decrypt work. A one-less page budget refuses before the mapper observes any plaintext and
still performs zero I/O. The packed graph integration proves cold and retained outputs match,
retained reads remain zero and privileged cache accounting remains bounded.

Focused storage and graph tests passed, followed by strict Clippy for storage, transaction and
graph crates. The complete optimized workspace gate passed **764 tests** with strict
all-target/all-feature Clippy. The complete optimized standalone T-20 gate passed **142 active
tests with five unchanged opt-in ignores** and strict Clippy. Logs:
`/tmp/uste-d246-workspace-verification.log` and `/tmp/uste-d246-native-verification.log`. Both gates
used one Cargo job, one test thread, locked offline dependencies and the 4 GiB process
address-space limit under the verified enclosing 5/6 GiB high/max and 512 MiB swap caps. The
enclosing scope recorded no maximum-limit, OOM, OOM-kill or CPU-throttle event.

No benchmark ran, so no latency, T-20, M1 or qualification claim follows. After commit and push,
rebuild and pin the release benchmark, then run one unchanged medium-range-pressure observation to
measure whether removing intermediary copies changes the zero-I/O retained path. Preserve the
existing cache defaults and every qualification prerequisite.
