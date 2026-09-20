# Decision 0188: Native coordinator admission buffering

Date: 2026-09-20

Status: Accepted

Select Decisions 0186/0187's buffered primary and quota admission in the shared packed development
engine, including native BM-01 and BM-06 phases. Give each canonical-family/correspondence phase
a fresh 64 MiB logical cache budget. Graph, primary and quota admission execute sequentially;
none of these caches survives its admission operation, shares pages with another admission or
warms the consumer query cache. Preserve all existing proof ceilings and exact fixture checks.

Report `coordinator_admission_buffered: true`, `coordinator_admission_cache_bytes: 67108864` and
`coordinator_admission_cache_scope: fresh-per-canonical-family-then-fresh-correspondence` in native
setup/phase output. Keep the existing graph-cache declarations and partial vault-work boundary.
Require the exact new setup declarations in the packed sampler supervisor; absent, null, wrong
budget/scope or unbuffered declarations cannot be relabeled as the new implementation.

This is a development integration change, not a format change or qualification. Archived native
measurements retain their exact implementation hashes and are not retroactively attributed to
this code. Native/model caps, seeds, workloads, latency targets, authority/durability semantics,
M1 consumer interfaces and release gates remain unchanged. Resource-safe larger construction,
complete accounting and reserved-host campaigns still remain required for T-20.
