# Decision 0077 — Separate-oracle verification of native disk queries

Date: 2026-09-18

Status: T-20 partial implementation; supervised disk sampling and qualification remain open.

Add `linux-disk-query` to the separate native development disk command family. Validate the
existing bounded oracle summary and measured-query profile before opening the database. Reuse
Decision 0076's completed-root admission and fixture Evidence binding; the returned owned session
keeps the native filesystem, disk coordinator and current policy alive without reconstructing
complete graph or coordinator maps. The query process never builds the independent oracle's
adjacency arrays. Its bounded summary remains in memory and is not represented as absent.

Use the existing client-composed traversal and exact outcome/visits/relationship/entity/logical-byte
checks against all 384 measured expectations. Preserve the `linux-query-v1` aggregate domain and
outcome tags so native disk results can be compared with existing independent expectations.
Storage/domain/authorization failures are not reclassified as expected traversal limits.

Select the accepted 64 MiB USTE reader cache and clear it through maintenance authorization before
each query. Reports disclose uncontrolled host caches and resident storage metadata. Cache counters
are not presented as complete authenticated I/O accounting. Single-pass latency percentiles and
whole-process RSS (including startup/key recovery) are diagnostics, not steady-state qualification.
The command explicitly does not enforce a preemptive query deadline and is not the supervised
sampler. It retains the development ceiling and `engine_benchmark:false`; the accepted five windows,
reservation, query deadline and BM-01/BM-06 thresholds remain unchanged.

Tests create a real Btrfs development database and compare every query, aggregate digest and total
visits/logical bytes against a separately serialized oracle summary. They check truthful report
flags/cache settings and reject warm-up, wrong-profile and truncated summaries before database I/O.
Native recovery and the original frozen memory-adapter oracle tests remain in the same suite.
M1 interfaces, its pinned handoff and the remaining full roadmap are unchanged.
