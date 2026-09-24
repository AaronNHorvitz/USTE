# Decision 0268: Hash-indexed lookup cache development observations

Date: 2026-09-23

Status: Development observations complete; no benchmark qualification or default change.

Measure Decision 0267 three times in the standalone lane with the unchanged regenerated
20,000-entity/200,000-relationship fixture, oracle bundle, query order, 96/128/32 MiB
page/lookup/range split and supervised 30-second per-query protocol. These are sequential
development observations with uncontrolled kernel/device caches on a three-CPU shared host, not
a qualifying campaign; three runs bound run-to-run spread, they do not establish causality.

Every run accepted 96 warm-ups, 768 measured executions, 40 latency groups and output digest
`aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`. Under the Decision 0266
recipe (timing, percentile and RSS fields removed), each report hashes to
`0334c5cd6b50ef077ae925c457643d25c61410870cd3a762b04fbdbde0b6e4de`, identical to the lane
baseline and to the retained Decision 0265 report. Lookup hits/misses (39,002,722 / 17,960,242),
zero lookup evictions, 54,114 resident values, 56,945,917 accounted lookup bytes, range
hits/misses (1,074,437 / 1,348,781), the 29,231,596-byte range high-water, 100,661,248 accounted
page bytes, 11,240,354 empty-half adapter reads and zero retained-half adapter reads are all
unchanged. The intrusive exact-LRU index therefore reproduces the ordered-map cache's
observable behaviour exactly.

Retained all-class successful p99 by depth (ms):

| run | round ms | 1-hop | 2-hop | 3-hop | 4-hop | peak RSS KiB |
|---|---|---|---|---|---|---|
| Decision 0266 baseline | 567,391 | 2.765 | 100.721 | 67.027 | 374.535 | 265,876 |
| Decision 0267 run 1 | 504,048 | 2.292 | 63.261 | 45.136 | 227.248 | 265,964 |
| Decision 0267 run 2 | 501,930 | 2.429 | 63.196 | 44.428 | 231.487 | 265,964 |
| Decision 0267 run 3 | 501,468 | 2.370 | 63.707 | 46.042 | 233.320 | 265,892 |

Relative to the lane baseline, the median run changes four-hop by -38.2%, three-hop by -32.8%,
two-hop by -37.3% and one-hop by -14.3%; the round shortened by 11.4%. All three four-hop
values are below the unchanged 250 ms target and all one-hop values are below the 20 ms
target, in this lane, at the 20,000-entity development scale, under the development protocol.
That is a development-profile pass, not BM-01 qualification: the qualifying 100,000-entity
five-sample campaign on the reserved 24 GiB host remains unperformed and the native packed
engine still refuses profiles above 20,000 entities. Empty-half four-hop p99 moved from
3,757.850 ms to 3,576.778 / 3,584.124 / 3,579.396 ms; that half is dominated by physical page
reads and decryption and is outside the warm target.

Process peaks were 265,964 / 265,964 / 265,892 KiB with 623.62 / 620.44 / 620.18 user seconds,
against the baseline's 265,876 KiB and 694.44 user seconds. Each run used the 4 GiB
address-space limit under the lane's 5/6 GiB high/max and 512 MiB swap caps with zero process
swaps. The release binary hashes to
`5c796b60c7e0395b7bf558f815971cb1f2dae9d066dac0584ee7d9f39f80563b`; fixture, oracle and
password inputs are those pinned by Decision 0266.

The complete reports are retained in
[`hash-indexed-lookup-cache-sampling.json`](../evidence/hash-indexed-lookup-cache-sampling.json),
[`hash-indexed-lookup-cache-sampling-repeat-2.json`](../evidence/hash-indexed-lookup-cache-sampling-repeat-2.json)
and
[`hash-indexed-lookup-cache-sampling-repeat-3.json`](../evidence/hash-indexed-lookup-cache-sampling-repeat-3.json).
Keep all qualification prerequisites and existing defaults. The next bounded work re-profiles
the retained path on this binary and removes the next largest zero-I/O cost.
