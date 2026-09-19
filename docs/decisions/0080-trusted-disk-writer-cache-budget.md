# Decision 0080 — Trusted disk writer cache budget

Date: 2026-09-18

Status: T-20 partial implementation; profile admission and qualification remain open.

Add `AuthorizedDiskWriter::new_with_cache_budget` for trusted adapter construction, retaining
the existing constructor's 64 KiB default and storage's fixed minimum/maximum validation.
Configuration is not a request override. A read-only budget accessor exposes only the fixed
constructor setting, not candidate-dependent cache counters. Proof, publication, authorization,
retry, collision and durability limits remain independent of cache residency.

Both benchmark disk adapters explicitly select 64 MiB for transaction preparation, matching the
accepted benchmark cache setting already used for queries. The writer owns its cache for one
batch and drops before metadata rebase or query sampling. This does not assert a process RSS
bound or account for storage metadata, proof buffers, OS caches or complete authenticated I/O.
Native commands remain development-capped; no qualifying campaign is enabled by this change.

Regression coverage rejects an oversized cache without filesystem reads, checks custom/default
budgets, and exercises custom-budget authorization, quota, precommit faults and reference-checked
durable writes. The existing default-budget exact retry still bypasses an insufficient preparation
budget. Native restart, real-process-loss and separately generated oracle tests retain their
accepted assertions. No durable format, M1 interface or acceptance threshold changes.
