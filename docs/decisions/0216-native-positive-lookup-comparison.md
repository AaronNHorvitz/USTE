# Decision 0216: Explicit native positive-lookup correctness comparison

Date: 2026-09-20

Status: Implemented and locally verified; no larger comparison or timing qualification yet.

After D0214/D0215 verification, provide a separate `linux-packed-lookup-query` development
command using the same native source, cold admission, frozen BM-01 materializer and independent
384-query oracle. Preserve the existing page-only command and all size/admission limits.
Select one 64 MiB total query cache with 16 MiB assigned to positive lookups and 48 MiB to pages.
The split is a declared comparison configuration, not a performance optimum or capacity claim.

Clear both partitions before each oracle query, preserving the existing cold-USTE-cache
correctness protocol. Report the selected configuration, exact total accounting and separate
positive-cache observations. Existing hits/misses/evictions remain page-only; do not combine
them into fictitious proof-work or I/O. Keep original output digests, successful visit/result
counts, limit outcomes, adapter counters and vault measurements. Validate report partition
consistency before returning success. No authoritative source writes or index migration.

Verify both modes against the same native fixture and oracle, source-byte preservation,
profile refusal, explicit command dispatch and report validation. The new command is not a
timing campaign: supervised positive-cache sampling and its checked per-state accumulation
remain subsequent work. No change to M1 interfaces, qualifying targets, host reservation,
five-sample/30-trial protocols, distribution gates or the remaining roadmap.

Native release regression passes 124 active tests (five unchanged opt-in ignores) and strict
Clippy. The same 20/200 fixture matches every oracle result in both modes without source-byte
changes; separate-process dispatch, positive hits, exact budget reporting and admission refusal
also pass. PROGRESS.md records exact commands, source baseline, binary hash and resource limits.
