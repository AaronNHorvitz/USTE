# Decision 0184: Explicit packed checkpoint recovery

Date: 2026-09-19

Status: Accepted

Add the native development phase `bm06-packed-linux-recover-checkpoint`. Require the full
authenticated BM-06 frontier, select exactly the profile's frozen checkpoint, and stream the
entire final generation through existing authenticated packed recovery. Require the observed
suffix group count to equal the profile's batches per version. Exact-retry every final-generation
transaction and verify all historical payloads as before. Report `checkpoint_tail_replay: true`
only for this explicit phase; ordinary latest-complete-root `recover` remains unchanged.

Newer intermediate or terminal roots never replace the requested checkpoint. Missing/corrupt
selected checkpoint material fails closed, even when terminal roots are valid. No roots are
removed to force replay. Recovery publishes ordinary derived terminal roots and must preserve
the source certificates, journal, manifest and key bytes. Repeated explicit recovery still
replays the tail; repeated ordinary recovery can select terminal roots and replay zero groups.

Native regression coverage corrupts retained checkpoint manifests after terminal recovery,
proves ordinary terminal open succeeds while explicit checkpoint recovery refuses, restores
the test-owned manifests, and verifies successful explicit replay. Separate-process CLI coverage
checks selection, suffix count, digest equality and certificate preservation. Oversized profiles
still refuse before filesystem access. Both full-history CLI caps remain two records.

This supplies explicit native selection, not a qualifying 196-group campaign. Phase duration
includes complete history verification and is not recovery-only latency. Interrupted multi-batch
tail continuation, safe larger construction, complete accounting and reserved-host qualification
remain required. No benchmark target or release gate changes.
