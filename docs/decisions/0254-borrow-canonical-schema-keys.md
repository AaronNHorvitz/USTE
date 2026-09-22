# Decision 0254: Borrow canonical schema keys during stored-record decoding

Date: 2026-09-22

Status: Implemented and locally verified; development observation pending.

Decision 0253 left the zero-I/O retained four-hop path at 421.852798 ms. After Decision 0252
removed the graph codec's second map, every stored-record decode still allocated an owned
`BoundedString` for each top-level canonical schema key even though those fixed keys are consumed
only while the encoded record input remains live.

Add a canonical root-map decoder that borrows its validated top-level UTF-8 keys from the input
frame while keeping every nested `Value` fully owned. The generic decoder and the new decoder share
the exact frame parser. Stored-record decoding uses the borrowed-key path and a shared record-field
consumer; checkpoint/result-record decoding continues to use the existing fully owned generic
decoder. A valid non-map root remains distinguishable and the graph layer preserves its exact
wrong-type error.

The borrowed keys cannot outlive their input. The implementation adds no cache or decoded-plaintext
retention. Frame version/kind/length checks, canonical key uniqueness and byte order, UTF-8 checks,
depth/node/byte limits, missing and unknown fields, nested values, graph semantics and persistent
bytes are unchanged. The new regression proves key slices point into the encoded input, values are
owned, every truncation fails, valid non-map roots return `None`, and duplicate, descending and
invalid-UTF-8 root keys retain their exact failures.

The first optimized workspace gate passed because debug assertions were enabled. The first
standalone release gate then failed 23 of its first 109 tests: the new root-map path consumed its
map tag only inside `debug_assert_eq!(payload.byte()?, MAP_TAG)`, so release builds skipped that
state-changing expression and decoded from the wrong byte. This was a real release-only gate
failure, not accepted verification. The tag read was moved to an unconditional statement and only
the comparison remains a debug assertion. A targeted release regression then passed.

After the correction, the complete optimized standalone T-20 gate passed **142 active tests with
five unchanged opt-in ignores** and strict Clippy. The exact final implementation tree passed the
complete optimized workspace gate with **765 tests** and strict all-target/all-feature Clippy.
Logs: `/tmp/uste-d254-native-verification.log` (the preserved failing release gate),
`/tmp/uste-d254-native-verification-fixed.log` (corrected native gate) and
`/tmp/uste-d254-workspace-verification-final.log` (final exact-tree workspace gate).

The successful gates used one Cargo job, one test thread, locked offline dependencies and the 4 GiB
process address-space limit under the verified enclosing 5/6 GiB high/max and 512 MiB swap caps.
The enclosing scope recorded no maximum-limit, OOM, OOM-kill or CPU-throttle event.

No benchmark ran, so no performance, T-20, M1 or qualification claim follows. After commit and
push, rebuild and pin the release benchmark, then run one unchanged medium-range-pressure
observation. Preserve canonical decoding, authorization, cache defaults and every qualification
prerequisite.
