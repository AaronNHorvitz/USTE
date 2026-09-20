# Decision 0177: Native packed development measurement

Status: Accepted

## Context and decision

Measure Decision 0176's native packed path at 1,000 synthetic entities and 10,000 relationships
before increasing scale. Preserve the independent oracle, cache budget, deadlines, accepted
benchmark thresholds and full qualification prerequisites. Archive complete reports in
[native packed evidence](../evidence/native-packed-1000-development.json); exact version,
binary digest, resource limits, command substitutions and results are recorded in PROGRESS.md.

Construction, cold query and supervised paired sampling exit successfully. The v1 state digest
matches the independent Decision 0166 fixture; 384 queries, 96 warm-ups and 768 timed executions
match their typed oracle outcomes. Certificate bytes remain unchanged. The query and sampling
output hashes use different established domains and are not compared to each other.

Cold-open last-owner decrypt accounting exposes 48,895,663,753 encoded bytes and 2,378,825
successful calls. Empty-cache query accounting adds 4,497,300,500 bytes/218,900 calls; retained
identical queries add none. These are repeated authenticated envelope bytes, not physical I/O.
Zero cache evictions mean the run does not establish larger-than-memory behavior.

## Consequences

Next reduce repeated cold-admission reads with a fresh, bounded, operation-local page cache.
Every canonical and semantic check and logical proof-work limit must remain intact. No cache
from earlier reads may conceal ciphertext mutations on a later cold admission. Existing
uncached behavior and fault/corruption tests remain required. Broader accounting and scale
prerequisites remain open; neither this run nor its small process cap qualifies BM-01/BM-06,
completes T-20/T-19, or changes M1, release gates or the remaining roadmap.
