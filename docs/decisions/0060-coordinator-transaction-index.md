# Decision 0060 — Coordinator transaction-ID disk index

Date: 2026-09-18

Status: T-20 implementation increment; coordinator maps and journal-origin replay remain open.

The retry-key ordering in `coordinator-meta-v1` cannot answer a transaction-ID collision lookup
without a complete scan. Introduce a separate optional encrypted index profile, SHA-256 of the
literal UTF-8 name `USTE coordinator-transaction-v1`:
`d78e78af45efd9c5a3a061239547cffd8bfbe9c6c18462887a513bee630d79ce`.

Its single family 1 contains transaction IDs as 16-byte ordered keys. Each value is exactly 136
bytes: principal digest (32 bytes) followed by the existing canonical 104-byte outcome value from
Decision 0031. The embedded transaction ID must equal the key. The root binds namespace, journal
certificate/revision, reducer profile and logical state digest. No existing profile is changed.

Publication cross-checks the complete live retry/transaction correspondence, streams the sorted
transaction map and returns an opaque handle after durable root publication. The trusted exact
lookup accepts explicit page/result limits and rejects a changed scope or frontier before I/O.
These are raw maintenance metadata: callers still own current principal authorization and expiry.

Admission after handle loss discovers only bounded manifests, selects the exact current anchor
and reducer digest, checks one-family/count shape, then exhausts a caller-bounded authenticated
run scan. Every entry must equal the independently recovered coordinator transaction metadata.
Wrong principal/outcome, malformed values, extra/missing entries and terminal digest failure expose
no handle. The comparator currently remains the full in-memory map. This step supplies the missing
disk ordering; subsequent work must replace that comparator and live maps with journal-validated
disk metadata and bounded overlays before claiming larger-than-memory recovery.

Verification under one Cargo job/test thread and a 4 GiB cgroup:

~~~text
cargo test -p uste-replay --test coordinator_checkpoint --locked --offline -- --test-threads=1
# 3 passed, including transaction lookup, missing ID, result/entry limits, stale frontier,
# exact re-admission and an authenticated but semantically wrong principal
cargo clippy -p uste-txn -p uste-replay --all-targets --locked --offline -- -D warnings
# passed
~~~

No BM-01/BM-06, T-20 closure, consumer API or production qualification follows from this increment.
