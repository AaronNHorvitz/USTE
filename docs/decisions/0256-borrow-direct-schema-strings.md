# Decision 0256: Borrow direct schema strings during stored-record decoding

Date: 2026-09-22

Status: Implemented and locally verified; development observation pending.

Decision 0255 left the zero-I/O retained four-hop path at 363.516682 ms. Decision 0254 borrowed
top-level map keys, but the generic value decoder still allocated every direct string value before
the graph codec immediately compared and discarded fixed stored-record discriminants: `kind` on
all record shapes, `lifecycle` on entities and `status` on assertions and relationships.

Add a separate canonical root-map decoder whose keys and direct string payloads borrow from the
input frame. Every non-string payload and every nested value remains in the ordinary owned `Value`
representation. The existing generic decoder and Decision 0254's key-only decoder remain
unchanged. Stored-record decoding consumes fixed discriminants through borrowed-or-owned text and
re-owns any direct string that is actual record data, including entity types, locators, predicates
and relationship types, before the input can be released. Checkpoint/result-record decoding keeps
the existing fully owned path.

Borrowed strings cannot outlive the input. Frame parsing, version/kind/length checks, canonical key
uniqueness and byte order, UTF-8 validation, depth/node/byte limits, missing/unknown fields, enum
validation, nested values, graph semantics and persistent bytes are unchanged. The implementation
adds no cache, retained plaintext, authority, format or configuration. Regressions prove direct
string pointers lie inside the encoded input, nested strings remain owned, valid non-map roots
return `None`, every truncation fails, and duplicate, descending and invalid-UTF-8 map keys retain
their exact errors. A targeted standalone release oracle test also passes, guarding the optimized
tag-consumption path independently of debug assertions.

The complete optimized workspace gate passed **766 tests** with strict all-target/all-feature
Clippy. The complete optimized standalone T-20 gate passed **142 active tests with five unchanged
opt-in ignores** and strict Clippy. Logs: `/tmp/uste-d256-workspace-verification.log` and
`/tmp/uste-d256-native-verification.log`.

Both complete gates used one Cargo job, one test thread, locked offline dependencies and the 4 GiB
process address-space limit under the verified enclosing 5/6 GiB high/max and 512 MiB swap caps.
The enclosing scope recorded no maximum-limit, OOM, OOM-kill or CPU-throttle event.

No benchmark ran, so no performance, T-20, M1 or qualification claim follows. After commit and
push, rebuild and pin the release benchmark, then run one unchanged medium-range-pressure
observation. Preserve canonical decoding, authorization, cache defaults and every qualification
prerequisite.
