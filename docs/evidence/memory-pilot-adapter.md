# T-67 restricted adapter and offline demo evidence

Implementation under test: `a07bdec` (`feat(memory): add restricted local consumer adapter`).
Decision 0057 fixes the embedded boundary; the runnable procedure is
`docs/memory-pilot-demo.md`.

## Implemented handoff surface

- `LocalMemoryAdapter::{create,open}` binds one Linux/Btrfs root, portable recovery key, exact
  namespace, current policy/principal and consumer authority generation.
- `ConsumerAuthority` makes exact source bytes and the durable upload checkpoint consumer-owned.
  Recovery resumes and idempotently commits pending sources; both UTF-8 and opaque bytes must match
  the consumer's current full-source SHA-256.
- Mutations cover fact insertion/correction, source revocation and fail-closed begin/complete
  rebuild. Queries reuse the bounded authorized memory requests and cancellation.
- `resolve_citation` authorizes first and returns exact bounded bytes without exposing a generic raw
  blob API. Owned results prevent cached view handles from escaping the adapter.
- The strict checkpoint rejects unknown version, scope/count/source errors and duplicate tokens.
  The Linux writer lock rejects a competing adapter.

## Verification

All commands ran sequentially under the established 4 GiB memory / 512 MiB swap process-group cap:

~~~text
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 \
  cargo test -p uste-memory-adapter --all-targets --locked --offline
# 2 passed; 0 failed

CARGO_BUILD_JOBS=1 \
  cargo clippy -p uste-memory-adapter --all-targets --locked --offline -- -D warnings
# passed

scripts/run_memory_pilot_demo.sh
# M1_DEMO created/reopened/corrected/revoked/rebuilt; final generation=2 revision=11

CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 bash scripts/check.sh
# passed; documentation=ok (127 links, 124 active IDs, 152 definitions)
# task_graph=ok (68 tasks); scaled T-20 experiment: 31 passed, 2 qualifying cases ignored
~~~

The demonstrated Btrfs root was
`target/memory-pilot-demo-runs/run.m1mcwL`; it remains ignored under `target/` for inspection.
The demo used the normal Argon2id portable recovery envelope, encrypted durable journal/blob path,
policy checks and real exclusive Linux ownership. It made no network request and used no model or
provider credential.

T-67 does not close M1. T-68 still owns forced termination at an acknowledged memory commit,
lost-response/process recovery oracle checks, disk-full/corruption/wrong-key cases, measured cold/
warm/ingest/RSS limits and the exact-version integration checklist.
