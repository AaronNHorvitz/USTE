# Decision 0258: Borrow selected nested stored-record schema

Date: 2026-09-22

Status: Implemented and locally verified; development observation pending.

Decision 0257 left the zero-I/O retained four-hop path at 368.605719 ms. Stored assertions and
relationships still decoded `valid_time` through owned nested canonical maps. The graph codec then
consumed and discarded the fixed `kind`, `start`, `end` and interval-bound schema keys plus their
fixed string discriminants. Dynamic `properties` and opaque `object` maps must remain owned.

Add an opt-in canonical root-map decoder that recursively borrows maps only for explicitly selected
root fields. Root keys and direct strings retain Decision 0256's borrowing. Unselected nested values
remain fully owned. Stored-record decoding selects only `valid_time`; its map and nested interval
bound maps consume borrowed keys and discriminants directly. Actual dynamic maps, strings and
record data retain ordinary owned `Value` representations. The existing generic, key-only and
direct-string decoders remain unchanged.

Every borrow is bounded by the input lifetime. Frame parsing, version/kind/length checks, canonical
key uniqueness and byte order at every selected depth, UTF-8 validation, depth/node/byte limits,
missing/unknown-field and enum errors, graph semantics and persistent bytes are unchanged. The
implementation adds no cache, retained plaintext, authority, format or configuration. Regressions
prove that one selected nested map borrows recursively while an adjacent unselected map stays
owned, borrowed pointers lie inside the frame, every truncation fails and a nested duplicate key
retains the exact canonical error. A stored half-open interval with bounded/unbounded endpoints
round-trips and rejects every truncation. A targeted standalone release oracle test guards the
optimized nested map-tag path with debug assertions disabled.

The complete optimized workspace gate passed **768 tests** with strict all-target/all-feature
Clippy. The complete optimized standalone T-20 gate passed **142 active tests with five unchanged
opt-in ignores** and strict Clippy. Logs: `/tmp/uste-d258-workspace-verification.log` and
`/tmp/uste-d258-native-verification.log`.

Both complete gates used one Cargo job, one test thread, locked offline dependencies and the 4 GiB
process address-space limit under the verified enclosing 5/6 GiB high/max and 512 MiB swap caps.
Maximum-limit, OOM, OOM-kill and CPU-throttle counters remained zero. The shared scope's cumulative
socket-memory throttling counter increased from one to two during the combined work; that shared
counter is not attributed to this increment.

No benchmark ran, so no performance, T-20, M1 or qualification claim follows. After commit and
push, rebuild and pin the release benchmark, then run one unchanged medium-range-pressure
observation. Preserve canonical decoding, authorization, cache defaults and every qualification
prerequisite.
