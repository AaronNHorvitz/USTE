# Decision 0264: Borrow singleton selected string maps without a collection

Date: 2026-09-22

Status: Accepted for the development implementation; no benchmark qualification or default change.

Decision 0263 left retained four-hop p99 at 381.871066 ms while the retained half performed zero
adapter reads. The accepted fixture stores each relationship's valid time as the one-entry map
`{kind: "unknown"}`. Decision 0258 borrowed that selected map's strings, but still allocated a
one-entry `Vec` for every decoded relationship. Half-open unbounded interval bounds have the same
singleton string-map shape.

Represent an explicitly selected one-entry map whose key and value are both strings directly as two
borrowed slices. Other selected maps retain the recursive collected representation. Stored-record
valid-time and interval-bound decoding consume the singleton form without allocating a collection,
while preserving missing-field and invalid-enum distinctions for malformed schemas. Generic and
unselected maps remain owned, and existing multi-entry selected maps are unchanged.

The singleton path retains map framing, collection length, canonical key, UTF-8, depth, node and
byte validation. Nested truncations still fail. Persistent bytes, graph results, authorization and
configuration are unchanged. This adds no cache or retained plaintext.

Regression coverage verifies the selected singleton representation and borrowed input pointers,
every truncation, a half-open interval with an unbounded singleton bound and the fixture's direct
unknown valid time. The first focused graph test build exposed a moved test fixture in the new
second round-trip assertion; the assertion was corrected before verification, and the repeated
focused release tests and strict Clippy passed. The targeted release oracle equivalence test passed.
The exact final tree passed the complete optimized workspace gate with **769 tests** and strict
all-target/all-feature Clippy. The standalone optimized T-20 gate passed **142 active tests** with
five unchanged explicit scale/oracle ignores and strict all-target/all-feature Clippy. Logs are
retained at `/tmp/uste-d264-workspace-verification.log`, `/tmp/uste-d264-release-oracle.log` and
`/tmp/uste-d264-native-verification.log`.

All gates used one Cargo job, one test thread and the 4 GiB process address-space limit under the
verified enclosing 5/6 GiB memory high/max and 512 MiB swap caps. From the post-Decision-0263
observation baseline through all D0264 verification, shared soft-limit events increased by 1,720.
Cumulative peaks remained 5,373,222,912 bytes memory and 459,055,104 bytes swap; maximum-limit,
OOM, OOM-kill, socket-memory-throttle and CPU-throttle counters did not increase.

No performance observation has run, so no latency target, T-20, M1 or qualification gate passes.
Next rebuild and pin the release benchmark, then run one unchanged medium-pressure observation
against the retained fixture and oracle. Preserve existing defaults and all qualification
prerequisites.
