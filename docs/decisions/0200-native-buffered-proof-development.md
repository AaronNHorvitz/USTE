# Decision 0200: Native development proof buffers

Date: 2026-09-20

Status: Accepted; local development verification and measurement passed.

After Decisions 0198–0199's primitive/domain checks, select a fresh 64 MiB preparation cache
for native packed BM-01/BM-06 writer requests and suffix/origin revisions. Keep model/default
constructors uncached and all logical proof, staging, certificate, nonce and native admission
limits unchanged. Existing 64 MiB staging/admission caches retain their separately bounded
sequential lifetimes. Declare `proof_cache_bytes` and `proof_cache_scope` in native reports.

Run the standalone regression suite, then the unchanged fresh 4,096-record/eight-batch
crash/recovery development fixture after checking resources. Preserve exact source/binary/lock
provenance, all phase reports, source prefixes and history digests. Compare with Decision 0197
without treating host/device cache state as controlled or adapter bytes as physical I/O.
No qualifying campaign, workload-cap increase, writer rotation or M1 contract change is implied.

All 118 active standalone tests and strict Clippy passed; four opt-in cases ignored. The separate
4,096-record run passed all five phases/658.16 seconds, preserving logical digests, source prefixes,
written bytes and nonce counts. Creation adapter reads fell from Decision 0197's 169,026,198,092
to 55,545,418,057 bytes; creation elapsed from 273,191 to 152,380 ms. These are uncontrolled-host
development comparisons, not complete authenticated I/O or qualifying recovery latency.
Post-workload scope peak was 3,222,016,000 bytes with swap peak 16,269,312 bytes. See the
[exact archive](../evidence/native-buffered-proof-4096-development.json).
