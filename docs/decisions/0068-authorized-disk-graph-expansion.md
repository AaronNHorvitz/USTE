# Decision 0068 — Authorized disk graph expansion

Date: 2026-09-18

Status: T-20 partial implementation; no qualifying benchmark result.

Decision 0067's disk reader now supports `Adjacent` and `SupportedBy` in addition to point/history
reads. Its development-only configuration is renamed `GraphDiskReadLimits`; optional expansion
limits let an adapter continue issuing point-read-only capabilities. The pinned M1 API is unchanged.

`GraphDiskExpansionLimits` is checked at trusted adapter construction and supplies one aggregate
page-visit, secondary-entry, encoded-byte and record-lookup allowance for a whole query. Both
directions share the allowance, as do subsequent relationship/neighbor/claim loads. Page visits
include search probes and cache hits. Encoded bytes include secondary keys/values and loaded record
values. Duplicate self-loop entries consume scan work twice but produce one relationship result.
Parallel relationships remain distinct and result order matches the reference reducer.

Current policy authorizes the starting record and graph expansion before any I/O. Relationship
and neighbor read/expand permissions are checked before their respective loads, and all embedded
references are filtered before publication. Provenance reads require candidate read permission
and reference filtering. Secondary entries are checked against record identity, revision,
accepted relationship status, direction/neighbor identity or exact evidence membership. The
ready root was semantically admitted against its journal anchor; queries do not rebuild full
graph maps. Candidate collections and outputs are bounded by explicit admission and existing
format/result maxima. Resource failure never returns partial success.

The consumer cannot inspect cache/work counters or change the adapter's configured admission.
Visible result limits still count only visible results. A cancelled candidate walk is reported as
cancellation, not an empty/partial successful answer. No constant-time guarantee is claimed.

Synthetic reference tests cover both directions, cycles, self-loops, parallel edges, provenance,
hidden records and embedded references, history permissions, absence, cancellation and visible
result refusal. A five-neighbor fixture admits exactly 24 page visits, six scanned entries,
ten record lookups and the calculated encoded bytes; one-less page/entry/lookup/byte budgets
refuse on repeated calls including populated caches. Every observed cold adjacency read fault
refuses provisional output, then retries exactly; denied expansion leaves an armed read fault
untouched. Full disk-aware writes, upload quota/reconciliation, scalable first-owner admission,
storage recovery metadata and BM-01/BM-06 qualification remain unfinished.
