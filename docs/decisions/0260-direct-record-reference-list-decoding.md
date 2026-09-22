# Decision 0260: Decode selected record-reference lists directly

Date: 2026-09-22

Status: Accepted for the development implementation; no benchmark qualification or default change.

Decision 0259 left retained four-hop p99 at 379.813938 ms while the retained half performed zero
adapter reads. Inspection of the repeated stored-record path found that assertion and relationship
`evidence` lists were first decoded into `Vec<Value>` and then consumed into a second,
final `Vec<RecordRef>`. The retained fixture relationship carries one evidence reference, so this
intermediate representation is exercised even when storage I/O is absent.

Add an opt-in canonical root-map decoder selection for record-reference lists. Stored-record
decoding selects only `evidence`; a structurally valid list of record references is decoded
directly into `Vec<RecordRef>`. Existing generic decoding, the owned stored-record path and all
unselected lists retain their prior behavior. If a selected field is not a record-reference list,
the decoder rewinds its cursor and node accounting and uses the ordinary owned-value path, so the
graph layer continues to report its existing structural and wrong-type errors.

The specialized path retains canonical frame, tag, identity, collection-length, depth, node and
byte validation. Malformed identities and truncations fail rather than falling back. Persistent
bytes, graph results, authorization and configuration are unchanged. Dynamic properties, opaque
objects and unselected lists remain owned; this adds no cache or retained plaintext.

Regression coverage verifies direct selected decoding, adjacent unselected-list ownership, every
truncation and wrong-element fallback. The stored half-open interval round trip now carries a
nonempty evidence reference through the graph path. The targeted release oracle equivalence test
passed. The exact final tree passed the complete optimized workspace gate with **769 tests** and
strict all-target/all-feature Clippy. The standalone optimized T-20 gate passed **142 active tests**
with five unchanged explicit scale/oracle ignores and strict all-target/all-feature Clippy. Logs
are retained at `/tmp/uste-d260-workspace-verification.log`,
`/tmp/uste-d260-release-oracle.log` and `/tmp/uste-d260-native-verification.log`.

All gates used one Cargo job, one test thread and the 4 GiB process address-space limit under the
verified enclosing 5/6 GiB memory high/max and 512 MiB swap caps. Cumulative cgroup peaks remained
5,373,222,912 bytes memory and 286,691,328 bytes swap. From the post-Decision-0259 observation
baseline through all D0260 verification, shared soft-limit events increased by 15,385; maximum,
OOM, OOM-kill, socket-memory-throttle and CPU-throttle counters did not increase. This cumulative
shared counter is not attributed solely to an individual test workload.

No performance observation has run, so no latency target, T-20, M1 or qualification gate passes.
Next rebuild and pin the release benchmark, then run one unchanged medium-pressure observation
against the retained fixture and oracle. Preserve existing defaults and all qualification
prerequisites.
