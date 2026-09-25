# Decision 0274: Hash-indexed range cache development observations

Date: 2026-09-24

Status: Development observations complete; no benchmark qualification or default change.

Measure Decision 0273, on top of Decision 0271, twice in the standalone lane with the fixture,
oracle bundle, query order, 96/128/32 MiB split and supervised protocol pinned by Decision 0266.
Both runs ran back to back under one shared heavy-work reservation with no other heavy work in
the lane. They are sequential development observations with uncontrolled kernel/device caches,
not qualification, and two runs bound spread without establishing causality.

Both runs accepted 96 warm-ups, 768 measured executions, 40 latency groups and output digest
`aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`, and both reports hash to
`0334c5cd6b50ef077ae925c457643d25c61410870cd3a762b04fbdbde0b6e4de` under the Decision 0266
recipe. In particular the range partition's terminal accounting (8,789,091 bytes), high-water
mark (29,231,596 bytes), 3,041 resident ranges, 37,388 resident entries, 1,074,437 hits,
1,348,781 misses and zero evictions are identical, which confirms that the new charge helper
reproduces the earlier reserved-capacity accounting exactly. Before measuring, review found and
corrected a two-byte under-charge for unbounded upper ranges in the first draft of that helper.

Retained all-class successful p99 by depth (ms):

| run | round ms | 1-hop | 2-hop | 3-hop | 4-hop | peak RSS KiB |
|---|---|---|---|---|---|---|
| Decision 0272 (D0271 only) | 500,282 | 2.372 | 65.824 | 39.905 | 211.094 | 265,636 |
| Decision 0273 run 1 | 501,670 | 2.320 | 59.547 | 40.537 | 208.501 | 265,936 |
| Decision 0273 run 2 | 504,034 | 2.466 | 60.453 | 41.401 | 199.612 | 265,552 |

The four-hop change against Decision 0272 (-1.2% and -5.4%) is within the spread seen between
repeated runs of one binary in Decision 0268, so no speed-up is attributed to the range change;
two-hop moved by about -9%. The change is kept because it removes a per-probe 181-byte key
allocation and two ordered-map updates without any observable behaviour difference, and because
range probes grow with frontier size at larger scales. Empty-half four-hop p99 was 3,638.259 and
3,583.489 ms. Every four-hop value remains below the unchanged 250 ms target and every one-hop
value below 20 ms at the 20,000-entity development scale in this lane only; BM-01 qualification
remains unperformed.

The runs used 621.23 and 622.37 user seconds and 676.60 and 677.94 seconds wall. The release
binary hashes to `c350f7b844c603b8ded752e62f3ef2ae0d0a98e6672ffb28fa9b1db744dd7e3e`. Reports are
retained in [`hash-indexed-range-cache-sampling.json`](../evidence/hash-indexed-range-cache-sampling.json)
and
[`hash-indexed-range-cache-sampling-repeat-2.json`](../evidence/hash-indexed-range-cache-sampling-repeat-2.json).
