# Decision 0114 — BM-06 versioned event materialization

Date: 2026-09-19

Status: accepted workload contract and locally verified fixture implementation. T-20 remains open.

Pin `bm06-materialization-v1` for the previously specified ten-million-event checkpoint recovery
workload. Use the unchanged BM-06 `synthetic-v1` events seed from the R0 registry. The exact-size
profile has 100,000 entity records, each with 100 retained versions: one create followed by 99
property replacements, in generation-major then record-ordinal order. Each event is one real graph
operation with exact version preconditions, not one counter increment standing in for many events.
Policy bootstrap is revision one and is not one of the ten million events.

Each version contains 4096 bytes: the little-endian event ordinal followed by 4088 bytes from a
BLAKE3 XOF with derivation context `USTE BM-06 materialization-v1 payload` and the accepted event
identity as input. This is deterministic synthetic content, not a new cryptographic algorithm.
Record IDs are the ASCII prefix `BM06ENT1` followed by the big-endian record ordinal, under an
explicit database/namespace. Entity type is the profile name, schema version one. ID reuse across
scaled profiles is intentional; a later durable driver must bind the complete fixture profile.

Batches contain at most 512 distinct records and never cross a generation. At exact size there
are 196 batches per generation, 19,601 total revisions, and a checkpoint/root frontier at revision
19,405 after 99 generations. The final 100,000 events occupy 196 suffix revisions. Every batch is
directly reconstructible from its revision without generating its predecessors. Payload alone
is at most 2 MiB per transaction; encoded maximum batches are checked against 3 MiB, below the
unchanged 16 MiB request cap. No normal authorization, encryption or durable-flush rule is relaxed.

Retained history has 40,960,000,000 payload bytes, exceeding the benchmark's 24 GiB allowance.
This is a lower bound on logical historical content, **not** proof that an implemented native
recovery fits that allowance. Independent expectations can calculate record/version/revision and
payload from ordinal; the bounded development test checks all historical versions and equivalence
of sequential reduction with decoded checkpoint plus suffix. The allocating reducer checkpoint is
only that small reference check, not the proposed exact-size recovery endpoint.

The executable manifest hashes the accepted 48-byte synthetic record stream. It explicitly does
not hash constructed canonical transaction bytes or an actual database, reports zero recovery
trials and makes no engine benchmark claim. Scaled profiles keep 100 versions and 4096-byte
payloads; only record count changes. Qualifying fixture dimensions are not qualification.

Remaining implementation includes a native disk-state materializer, exact independent history
verification, checkpoint/root and suffix recovery variants, corrupt/missing-cache controls,
retained-base coverage, process-loss supervision and the 30 independent recovery trials. Their
120-second target and reserved reference host requirements are unchanged. Whole immutable-family
rewrite amplification must be addressed before exact materialization is safely runnable. This
decision neither promotes derived roots to commit authority nor takes over T-35's baseline
switching, compaction, certificate rollover or orphan-reclamation responsibilities.
