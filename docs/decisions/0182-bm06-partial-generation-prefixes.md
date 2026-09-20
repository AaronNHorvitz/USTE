# Decision 0182: BM-06 partial-generation prefixes

Date: 2026-09-19

Status: Accepted

Remove the one-batch-per-generation assumption from packed BM-06 prefix counting and historical
verification, without raising either development CLI's two-record cap. The accepted 100 versions,
4096-byte payloads, generation-major order, 512-record batches, seed, IDs and encoded requests
remain unchanged. This is a prerequisite for larger native construction, not its qualification.

For certified revision R in 1..=profile frontier, let B be batches per generation. Completed
generations are `(R - 1) / B`; the partial generation contains `((R - 1) % B) * 512` events.
History cardinality is completed generations times record count plus that partial count.
Current cardinality is the partial count before the first generation completes, otherwise the
full record count. Policy contributes one current and one historical row. Bounded profile
dimensions make arithmetic exact; revision zero, beyond-frontier and maximal integer inputs
refuse before history access. No count-sized allocation is introduced.

The shared packed history verifier now accepts an exact frontier, checks its complete family
cardinalities and streams only its committed event ordinals. Every authorized historical read
still checks ID, version, creation/modification revisions, lifecycle, schema/type and all payload
bytes against the existing literal oracle. The complete-generation entry point preserves its
old validation and delegates to this implementation. Native resume/rebuild/open verification
uses exact frontier rather than treating revision count as generation count; bootstrap verifies
an actual empty-data policy base instead of skipping its shape check.

An independent event-revision scan checks every prefix for 1, 2, 511, 512, 513, 1024 and 1025
records; explicit qualifying-size boundary counts are literal arithmetic tests, not construction.
A separate bounded 513-record model fixture executes the first three data batches, compares
full v1 state against the ordinary reference reducer, cold-opens each prefix, checks all committed
payloads and retries each batch exactly. It exercises partial first and second generations and
refuses stale/future/invalid frontier claims. Its model filesystem has an explicit 512 MiB
capacity; it is not the full 100-generation CLI, native durability or larger-than-memory evidence.

Multi-batch native tail construction/selection, larger-scale safe construction, complete accounting
and qualified BM-06 trials remain open. Do not raise caps merely because prefix arithmetic passes.
