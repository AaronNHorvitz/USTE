# Decision 0250: Fallible borrowed range mapping

Date: 2026-09-22

Status: Implemented and locally verified; development observation pending.

Decision 0249 left the zero-I/O retained four-hop path at 475.754743 ms. Inspection found that the
packed graph range mapper produced one `Vec<Result<ExpansionScanEntry, GraphDiskError>>`, then
allocated a second vector and copied every successful compact entry solely to surface a possible
authenticated-index semantic error. The mapper had already reduced each cached key/value pair to
two fixed identifiers, so this conversion added allocation and copy work without adding authority
or validation.

Add a fallible form of borrowed complete-range mapping through storage and the scoped transaction
reader. It admits the exact canonical tree, owner, unlocked session, identity, bounds, direction and
logical cursor work before the mapper observes resident plaintext. Returned mapped values remain
owned and cannot borrow from the cache. The existing infallible mapped API delegates to the new
form, preserving its public behavior.

On a mapper failure, every already-admitted entry is still inspected and the first mapper error is
returned after the normal retained-hit recency update. A cold result is retained before mapping as
before. Internal cache-integrity failures still take precedence, and failed mapping returns no
partial output. Thus the prior packed graph failure order, hit accounting, cache admission and
proof-work charging remain unchanged. The graph reader now maps directly to one
`Vec<ExpansionScanEntry>` and returns that allocation after charging the same report.

The storage regression proves a warm fallible mapper calls all three admitted entries, returns its
original middle-entry error, performs zero adapter reads and decryptions, and increments the same
range-hit counter. The existing graph integration proves exact cold/warm results, work accounting,
bounded configuration and privileged-cache behavior. Focused tests and strict storage,
transaction and graph Clippy passed.

The complete optimized workspace gate passed **764 tests** with strict all-target/all-feature
Clippy. The complete optimized standalone T-20 gate passed **142 active tests with five unchanged
opt-in ignores** and strict Clippy. Logs: `/tmp/uste-d250-workspace-verification.log` and
`/tmp/uste-d250-native-verification.log`. Both gates used one Cargo job, one test thread, locked
offline dependencies and the 4 GiB process address-space limit under the verified enclosing 5/6
GiB high/max and 512 MiB swap caps. The enclosing scope recorded no maximum-limit, OOM, OOM-kill or
CPU-throttle event.

No benchmark ran, so no performance, T-20, M1 or qualification claim follows. After commit and
push, rebuild and pin the release benchmark, then run one unchanged medium-range-pressure
observation to measure the zero-I/O retained path. Preserve authorization, cancellation, exact
failure behavior, cache defaults and every qualification prerequisite.
