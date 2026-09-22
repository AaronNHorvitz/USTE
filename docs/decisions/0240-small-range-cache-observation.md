# Decision 0240: Small-range-cache development observation

Date: 2026-09-22

Status: Development observation complete; no benchmark qualification or default change.

Measure Decision 0239 once with the unchanged retained 20,000-entity/200,000-relationship fixture,
oracle, query order and supervised 30-second per-query protocol. Preserve the 256 MiB total as
112 MiB pages, 128 MiB positive lookups and 16 MiB complete ranges. This is a sequential
development observation with uncontrolled kernel/device caches, not a causal or repeated campaign.

The parent accepted the complete `bm01-linux-packed-wide-small-range-sampling-v1` report: 96
warm-ups, 768 measured executions, 40 latency groups and output digest
`aec16fdc8a1630a97eace654862a192929550c5fb771aed0862a9bed67d81584`. The D0238 semantic subset
again matches Decision 0233 with SHA-256
`1ee6e2472d2cf96e80492d72ad1588c9133eb3be2014e8f02562bbaec51f76e1`. Lookup work also matches
D0233 exactly: zero evictions, 14,323,455 empty misses and 25,266,802 retained hits with zero
retained misses. Changed page/range work cannot and does not match.

The 16 MiB range partition is not sufficient for the measured working set. Warm-up recorded 70,636
evictions. The measured empty half recorded 257,206 evictions and 1,074,437 misses; the retained
half recorded 868,946 evictions, 205,491 hits and 868,946 misses. The retained range hit rate is
only 19.13%. Terminal residency was again 3,041 ranges / 37,388 entries / 8,789,091 bytes, proving
that terminal residency alone materially understates capacity pressure. Do not choose the next
partition from that terminal gauge.

Adapter reads were 9,600,066 empty and 1,557,673 retained: 16.12% above and 34.94% below D0233,
respectively, while their sum was 4.65% higher. The round took 638,531 ms, 1.99% below D0233 and
40.59% below the rejected D0238 split. Retained all-class successful p99 by depth changed from
D0233's 8.940083 / 568.149641 / 363.751144 / 1263.669322 ms to **4.008341 / 123.235164 /
84.051509 / 1294.381737 ms**. Depths one through three improved 55.16%, 78.31% and 76.89%; depth
four regressed 2.43% and remains over five times the unchanged 250 ms target. No target passes and
one mixed observation does not establish causality.

The release process used 265,656 KiB peak RSS, 759.16 user / 52.59 system seconds and 813.56
seconds wall time with zero process swaps. It ran alone under the 4 GiB address-space limit and
verified enclosing 5/6 GiB high/max and 512 MiB swap caps. Cumulative cgroup peaks remained
5,372,850,176 bytes memory and 286,691,328 bytes swap; soft-limit events stayed at 100,852 and
maximum-limit, OOM, OOM-kill and CPU-throttle counters remained zero. Source certificate
`b8fea4e223947316555069db1869b7ef57e121077e84ceff02a1930061f1e6cd`, oracle bundle
`f667f4420fce69e773d55e4c8ace4ed16878f063f4afc11ce1e1c6a234e5797b` and executable
`a28a4c81a655d0da60efeef158f1ccf823024e3d636d9892065a6a07f67beb39` stayed unchanged.

The complete report is retained in
[`small-range-cache-sampling.json`](../evidence/small-range-cache-sampling.json). Keep every T-20
qualification prerequisite and existing default. Before another split, add checked range-cache
high-water and evicted-byte accounting so capacity decisions use actual pressure rather than the
misleading terminal gauge.
