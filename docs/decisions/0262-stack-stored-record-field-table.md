# Decision 0262: Visit stored-record roots into a fixed stack field table

Date: 2026-09-22

Status: Accepted for the development implementation; no benchmark qualification or default change.

Decision 0261 left retained four-hop p99 at 386.697762 ms while the retained half performed zero
adapter reads. After the preceding key, string, nested-map and record-reference-list changes,
stored-record decoding still allocated one root `Vec` for every record, linearly searched that
collection for each fixed field and removed entries as the record was assembled. The complete
stored-record schema has a closed 22-name union and does not need a heap collection at that layer.

Add an opt-in canonical root-map visitor with the same selected nested-map and record-reference-list
behavior as the existing collected decoder. Stored records visit validated root entries directly
into a 22-slot `Option` table on the stack and consume fields by fixed index. Missing fields retain
their named error; recognized-but-inapplicable fields and unknown names remain unknown-field
errors. Nested `valid_time` maps, direct evidence lists and dynamic values keep their existing
representations. Existing collected and generic decoder APIs remain unchanged.

The visitor retains complete frame, canonical key order and uniqueness, UTF-8, tag, identity,
collection-length, depth, node and byte validation before accepting the record. Valid non-map roots
remain fully validated before returning the ordinary wrong-type result. Persistent bytes, graph
results, authorization and configuration are unchanged. This adds no cache or retained plaintext.

Regression coverage compares visitor and collected selected-list results, checks every truncation,
and exercises missing and unknown stored-record fields through the fixed table. The stored
half-open interval with nonempty evidence still round-trips. The targeted release oracle
equivalence test passed. The exact final tree passed the complete optimized workspace gate with
**769 tests** and strict all-target/all-feature Clippy. The standalone optimized T-20 gate passed
**142 active tests** with five unchanged explicit scale/oracle ignores and strict all-target/all-
feature Clippy. Logs are retained at `/tmp/uste-d262-workspace-verification.log`,
`/tmp/uste-d262-release-oracle.log` and `/tmp/uste-d262-native-verification.log`.

All gates used one Cargo job, one test thread and the 4 GiB process address-space limit under the
verified enclosing 5/6 GiB memory high/max and 512 MiB swap caps. From the post-Decision-0261
observation baseline through all D0262 verification, shared soft-limit events increased by 3,244
and the cumulative socket-memory-throttle counter increased from two to four. Memory peak remained
5,373,222,912 bytes; swap peak rose from 286,691,328 to 459,055,104 bytes. Maximum-limit, OOM,
OOM-kill and CPU-throttle counters did not increase. These shared cumulative changes are not
attributed solely to an individual test workload.

No performance observation has run, so no latency target, T-20, M1 or qualification gate passes.
Next rebuild and pin the release benchmark, then run one unchanged medium-pressure observation
against the retained fixture and oracle. Preserve existing defaults and all qualification
prerequisites.
