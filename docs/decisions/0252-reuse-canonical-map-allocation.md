# Decision 0252: Reuse canonical map allocation during graph decoding

Date: 2026-09-22

Status: Implemented and locally verified; development observation pending.

Decision 0251 left the zero-I/O retained four-hop path at 522.109698 ms. Inspection of repeated
stored-record decoding found that the generic canonical decoder already returned each map as one
owned vector with unique, byte-ordered bounded-string keys. The graph codec then converted every
such short structured map into a new heap-node `BTreeMap<String, Value>`, moving all existing key
strings into it only to remove every known schema field and discard the tree.

Keep the canonical map's existing vector allocation as the graph codec's field consumer. Field
lookup linearly finds the requested static schema name and `swap_remove`s its owned value. Graph
records and other structured graph payloads have short fixed schemas, so this removes the second
map's node construction while retaining bounded input and output. Ordering is no longer needed
after the generic decoder has validated canonical uniqueness and order.

Missing fields still return the exact missing-field error. Unconsumed fields still make `finish`
return the exact unknown-field error. Wrong types, invalid enums, generic value depth/node/byte
limits, canonical ordering, record contents and persistent bytes are unchanged. The implementation
adds no cache, retained plaintext, authority, format or configuration.

Focused graph codec tests preserve missing/unknown-field refusal, transaction round trips and every
truncation. Checkpoint round trips/truncations and the independent differential history pass, as
does strict graph Clippy. The complete optimized workspace gate passed **764 tests** with strict
all-target/all-feature Clippy. The complete optimized standalone T-20 gate passed **142 active
tests with five unchanged opt-in ignores** and strict Clippy. Logs:
`/tmp/uste-d252-workspace-verification.log` and `/tmp/uste-d252-native-verification.log`.

Both complete gates used one Cargo job, one test thread, locked offline dependencies and the 4 GiB
process address-space limit under the verified enclosing 5/6 GiB high/max and 512 MiB swap caps.
The enclosing scope recorded no maximum-limit, OOM, OOM-kill or CPU-throttle event.

No benchmark ran, so no performance, T-20, M1 or qualification claim follows. After commit and
push, rebuild and pin the release benchmark, then run one unchanged medium-range-pressure
observation. Preserve authorization, canonical decoding, cache defaults and every qualification
prerequisite.
