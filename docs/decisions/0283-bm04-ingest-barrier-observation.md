# Decision 0283: BM-04 ingest is bound by per-chunk durability barriers

Date: 2026-09-26

Status: Development observation and analysis; no benchmark qualification, target or protocol
change.

BM-04's recorded large-object ingest is 95.923 MiB/s against the unchanged 250 MiB/s target (T-15
evidence). Before designing any change, establish where the time goes.

Analysis. `write_upload` handles each 1 MiB chunk as follows:

- **Recovery probe.** It first probes both terminal manifests (`recover_terminal_state`).
- **Chunk publication.** `flush_buffer` publishes the chunk with a create, write, `set_len`, file
  sync, directory sync, rename and directory sync.
- **Progress witness.** `publish_progress` writes the authenticated progress witness through its
  own create, write and rename sequence. That function contains two file-sync and four
  directory-sync sites.
- **Totals.** Every megabyte of ingest therefore pays roughly three file syncs and five to seven
  directory syncs, plus several opens and renames.
- **Crypto.** XChaCha20-Poly1305 encryption and SHA-256 at 1 MiB per call are orders of magnitude
  cheaper than that barrier sequence on local NVMe.

Observation. The existing `blob_stream_probe` example was run once in the standalone lane under
the shared heavy-work reservation. It used a 1 GiB stream (not the BM-04 12 GiB or mixed
profile), the production Linux adapter, portable Argon2id wrapping and normal encryption and
durability. It ingested 1,073,741,824 logical bytes (1,145,215,578 encrypted disk bytes) in
9.668 s, 105.9 MiB/s. Recovery took 2.543 s and full verification 2.230 s. The whole process used
7.24 user and 3.68 system seconds over 14.44 s wall, 75% of one CPU, at 264,812 KiB peak RSS. The
release example hashed to
`5e20c57cdf6b105cd09d1155a4771804d40f3f8f9cef5592478fa4e93866a3b4`. This is one sequential
development observation with uncontrolled caches, not qualification.

The CPU fraction and the barrier count agree: ingest spends most of its time waiting on
durability barriers. Reaching 250 MiB/s needs about 4 ms or less per chunk, which the current
per-chunk protocol cannot provide on this class of device. Encryption or copy tuning cannot reach
it either. The algorithmic change is to amortize barriers: publish one authenticated progress
witness per group of chunks, with resume re-verifying the unwitnessed staged chunks, or stage
chunks without per-chunk renames.

Either is a change to the Decision 0017 staging and durability protocol. It needs its own
decision, the complete T-15 publication fault matrix rerun, and resume, abort and corruption
vectors. No such change is made here, and the 250 MiB/s target and 1 MiB chunk format are
unchanged. BM-04's mixed 100,000-small-object workload still has no runner.
