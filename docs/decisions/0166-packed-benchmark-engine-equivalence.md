# Decision 0166 — Packed benchmark engine equivalence

Date: 2026-09-19

Status: implemented and locally verified; native packed integration and qualification remain open.

Add an opt-in packed engine development verifier alongside, not in place of, the existing v1
benchmark engine. Reuse the frozen BM-01 materializer, transaction mapping and independent query
oracle. Bootstrap only the policy transaction in the ordinary reducer, then explicitly recover
packed genesis without publishing v1 graph/coordinator roots. All subsequent graph writes use
the authorized packed writer, with bounded metadata overlays rebased after each batch.

Cold open must discover and independently admit a paired terminal graph/primary/quota triple;
it must not silently rebuild. Explicit origin reconstruction remains a separate operation and
must reproduce the same logical state and query outputs. Use disk certificate/blob metadata
recovery, bounded packed plaintext caching, and declared fixture-specific work ceilings. Keep
the independent memory oracle and test filesystem outside all performance claims.

Retain the existing development size cap, all older verifiers, native artifacts and qualification
targets. This increment is engine equivalence integration, not native benchmark integration,
larger-than-memory qualification, complete physical I/O accounting or a change to M1 interfaces.

`packed-engine-check` passes the pinned 20/200 digest and the 1,000/10,000 development ceiling.
Both cold paths compare all 384 oracle queries and exact final retries; the second cold admission
also matches the complete v1 logical-state digest. Configuration arithmetic/read-limit validation
covers every accepted entity count without executing a qualifying database. Three new tests and
all 74 active benchmark regression tests pass; two pre-existing exact-oracle tests remain ignored
by their existing ordinary test-run policy. Strict Clippy passes.

The [1,000-entity observation](../evidence/packed-engine-1000-development.json) records 250.81 s
for the whole command including compilation, 431,496 KiB peak RSS, zero swaps and zero query-cache
evictions. It does not isolate construction, admission, queries or rebuild, and supplies neither
native I/O evidence nor cache-pressure acceptance. Exact commands/resource scopes are in PROGRESS.md.
