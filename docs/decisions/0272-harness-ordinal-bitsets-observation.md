# Decision 0272: Harness ordinal-bitset development observation

Date: 2026-09-24

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0271 once in the standalone lane with the regenerated 20,000-entity/200,000-
relationship fixture, oracle bundle, query order, 96/128/32 MiB page/lookup/range split and
supervised 30-second per-query protocol pinned by Decision 0266. This is a sequential development
observation with uncontrolled kernel/device caches on a three-CPU shared host, run alone under
the shared heavy-work reservation. It is not qualification and does not establish causality.

The parent accepted 96 warm-ups, 768 measured executions, 40 latency groups and output digest
`aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`. Under the Decision 0266
recipe the report hashes to `0334c5cd6b50ef077ae925c457643d25c61410870cd3a762b04fbdbde0b6e4de`,
identical to the lane baseline and Decision 0268. Lookup and range counters, residency, the
29,231,596-byte range high-water, 11,240,354 empty-half adapter reads and zero retained-half
adapter reads are unchanged, as the unchanged read order requires.

Retained all-class successful p99 by depth was **2.372 / 65.824 / 39.905 / 211.094 ms** against
the Decision 0268 median run's 2.370 / 63.707 / 46.042 / 233.320 ms (four-hop -9.5%, three-hop
-13.3%, two-hop +3.3%, one-hop +0.1%). The round took 500,282 ms. Empty-half four-hop p99 was
3,652.327 ms. The measured interval includes the harness's own traversal bookkeeping, so this
reduction is measurement-overhead removal, not engine speed-up; one observation cannot attribute
it causally. Four-hop remains below the unchanged 250 ms target and one-hop below 20 ms at the
20,000-entity development scale in this lane only; BM-01 qualification remains unperformed.

The release process used 265,636 KiB peak RSS, 618.42 user seconds and 675.00 seconds wall with
the 4 GiB address-space limit. The binary hashes to
`12d5ec4b0270ce17508c8a0bbd44b7a41456ca9c8c00e71ce157e0705dc554f5` and contains Decision 0271
over the committed Decision 0267 storage. The complete report is retained in
[`harness-ordinal-bitsets-sampling.json`](../evidence/harness-ordinal-bitsets-sampling.json).
