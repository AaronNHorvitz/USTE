# Decision 0285: BM-02 development runner over the authorized durable commit path

Date: 2026-09-26

Status: Accepted development tooling; no benchmark qualification, target, profile or protocol
change.

Decision 0284 found that BM-02 (single durable commit p99 ≤ 50 ms and ≥ 2,000 events/s in
batches, Decision 0007) had no runner. This decision adds a development runner and records what
it does and does not measure.

## Runner

`uste-t20-bench linux-bm02-development --root ROOT --password-file PASSWORD --single N
--batches B --batch-events E` (`experiments/t20-bench/src/linux_runner/bm02.rs`):

- **Store.** Creates a fresh encrypted store on the production Linux/Btrfs adapter, with portable
  Argon2id key wrapping and the benchmark policy installed at revision 1. It refuses an existing
  store.
- **Write path.** Every commit goes through `AuthorizedCoordinator::commit` with the v1 graph
  reducer. The latency sample runs from submitting the canonical request to the durable receipt.
  The journal remains the only commit authority.
- **Single phase.** `N` single-operation commits. Three entity creates are followed by one
  `ReplaceEntity` correction of the newest entity, guarded by `Expected::Version`, and the
  pattern repeats. After each commit, a fresh authorized read view serves a point read of the
  touched record. The read's version is checked against a reference count of versions, so a
  lost or reordered write fails the run.
- **Batch phase.** `B` transactions of `E` entity creates each. Events per second are reported
  both over commit time and over phase wall-clock time. The wall-clock rate includes operation
  construction and client encoding.
- **Report.** The report contains:
  - p50, p95, p99 and maximum for all single commits, creates, corrections, interleaved reads
    and batches;
  - transactions, events and request bytes per second;
  - final revision, current and peak RSS, and a BLAKE3 outcome digest over the revision count,
    entity count and every record's final version.

  It is labelled `nonqualifying-development` with `budget_evaluation: not-performed`. Decision
  0007's targets are echoed but not evaluated.
- **Development ceilings.** At most 2,000 single commits, 64 batches and
  `MAX_TRANSACTION_OPERATIONS` events per batch.

## Boundaries

This runner does not qualify BM-02:

- **Readers.** Reads run on the same thread between commits. Concurrent readers, writer queue
  depth and backpressure are not measured, and the report lists them under `not_measured`.
- **Index writer.** Commits maintain the in-memory graph reducer, not the packed disk-index writer
  used by the BM-01 development engine.
- **Manifest.** `acceptance/r0/benchmark-manifest.tsv` pins BM-02's seed and budget but only the
  scale label `mixed_writes_reads`. The qualifying record counts, reader concurrency and duration
  are not pinned. Pinning them is a benchmark-profile decision for the owner; this runner does
  not presume it. Its record identities are deterministic counters, not the seeded `synthetic-v1`
  stream.
- **Host.** Qualification needs the reserved reference runner.

`experiments/t20-bench/tests/bm02_development.rs` runs a nine-commit, two-batch plan end to end
on a fresh Btrfs root. It checks the counts, final revision and labels, and checks that a second
run refuses the existing store.
