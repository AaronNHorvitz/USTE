# Decision 0161 — Bounded authorized packed graph expansion

Date: 2026-09-19

Status: implemented and locally verified, including bounded native regression; T-20 qualification remains open.

Extend the restricted packed graph reader with adjacency (incoming/outgoing/either) and evidence
support queries. Share the existing disk-reader's pure candidate/record validation, visibility,
ordering, duplicate handling and result-limit semantics through a private reader abstraction.
Do not replace accepted graph semantics with index-only answers.

Trusted construction fixes aggregate per-query page, encoded-byte, candidate, returned-byte and
record-lookup budgets. Scans use bounded packed cursors and account their actual work before any
subsequent scan or lookup. Candidate collections remain bounded by both count and bytes; all
index rows must have exact shapes. Record proofs share the remaining page/byte budget. Per-call
primitive limits remain independently enforced. Hidden candidates consume work even when filtered;
no cardinality/work telemetry escapes the restricted consumer facade.

Reuse current-policy authorization of every candidate and embedded reference, and sticky observed
cancellation. No partial success on resource, corruption, I/O, cancellation or visible-result-limit
errors. Test reference equivalence, direction/self-loop/parallel-edge ordering, evidence support,
hidden dependencies, exact aggregate bounds, late corruption, every observed read fault and cold
recovery. Packed caching and native qualification remain separate requirements.
