# Decision 0197: Native packed development staging buffers

Date: 2026-09-20

Status: Accepted; local development verification and measurement passed.

After Decision 0196's domain checks, select a 64 MiB fresh per-private-tree-batch staging
cache in native packed BM-01/BM-06 development paths. Set graph genesis, suffix graph,
live graph publication and coordinator suffix/rebase staging explicitly. Keep model/default
limits uncached and all certificate, logical proof-work, write, owner, delta and nonce limits
unchanged. Admission and reader caches remain separately declared, sequential operations.

Report the staging selection and its per-batch lifetime separately from canonical/semantic
admission caches. Existing adapter counters and phase partitions describe actual observed
calls, not physical-device or complete authenticated I/O. Do not attribute previously pinned
measurements to this implementation or advertise a benefit before measuring it.

Run ordinary native corruption, source-binding, prefix/retry and supervised process-loss tests.
Then rerun the resource-admitted explicit eight-batch development case with fresh synthetic
storage, retained artifacts, exact source/binary provenance and unchanged expected source/history
semantics. Compare its digest and measurements to Decision 0193 while disclosing uncontrolled
host caches and resource limits. No workload admission increase or qualifying campaign follows.

Results: all 118 active standalone tests and strict Clippy passed; four opt-in cases ignored.
The separate fresh 4,096-record eight-batch crash/recovery case passed all five phases in
780.74 seconds. Creation reads were 169,026,198,092 adapter bytes versus Decision 0193's
219,650,187,522; creation elapsed 273,191 versus 321,264 ms. Digests, written bytes and nonce
counts match. Recovery elapsed times do not consistently improve. Post-workload scope peak
was 3,222,011,904 bytes with zero swap. See the
[exact development archive](../evidence/native-buffered-staging-4096-development.json).
The initial compile-time JSON macro recursion error was repaired by separate field insertions,
without raising compiler limits. No benchmark qualification or T-20 completion is implied.
