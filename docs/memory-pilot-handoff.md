# Exact-version M1 consumer handoff

This handoff is for a separately admitted local shadow integration of the bounded derived-memory
pilot. It is not permission to modify AgentMage, migrate authoritative memory, store sensitive
personal data, deploy production services or distribute executables.

## Pin and build

~~~bash
git fetch origin codex/uste-implementation
git checkout b9689f37d7e728d8ad7e27d50def3541e8609132
test "$(sha256sum Cargo.lock | cut -d' ' -f1)" = \
  7ed2b533b3c801250a89e008b4ca26c48f16400e38224d8a63447c0393aaa97b
rustup run 1.95.0 cargo test -p uste-memory-adapter --all-targets --locked --offline
~~~

The pinned packages are `uste-memory 0.1.0` and `uste-memory-adapter 0.1.0`, default features only,
on x86_64 Linux/Btrfs. They are unpublished source components, not a semver-stable release. The
demo requires dependencies already fetched into the local Cargo cache; normal build dependency
acquisition is separate from offline operation.

## Supported shadow operations

- exact immutable UTF-8 or opaque source versions up to the frozen limits;
- evidence-bound facts, explicit links, corrections, contradictions and retractions;
- durable source revocation and fail-closed generation rebuild;
- current or recorded-revision identity, all-term lexical and one-hop queries;
- exact/missing source-event equality filtering;
- exact authorized citations and original source bytes;
- stable idempotent retries, bounded pending-upload recovery and cooperative cancellation.

The consumer calls the embedded Rust API; no socket/service is included. Keep the adapter object
inside one owner process and never expose its internals to model-generated code.

## Required consumer mapping checklist

- [ ] Allocate one USTE namespace solely for the shadow index and map stable consumer source/fact
  IDs without reusing IDs across meanings.
- [ ] Keep the existing source store authoritative and return exact immutable bytes for each
  approved source version.
- [ ] Mint the current policy/principal through the consumer's trusted adapter; map every approval
  change to policy replacement and every synchronization reset to exactly one generation advance.
- [ ] Implement `ConsumerAuthority` in the consumer's existing durable transaction/outbox layer.
  `persist_pending` must fsync/commit before returning; store token, source ID/version, encoding,
  event time and stable retry IDs; cap at eight.
- [ ] Verify complete source length/SHA-256 on every resumed import and leave changed sources closed
  for explicit rebuild.
- [ ] Store the recovery credential using the consumer's approved local key mechanism. Do not use
  the synthetic demo password or log credentials/content.
- [ ] Use an owner-only Btrfs directory and one writer. Treat `LOCKED`, unsupported version,
  wrong-key/integrity, outcome-unknown and stale-generation as visible stop/recovery states.
- [ ] Begin with synthetic shadow writes and compare every answer/citation with the consumer's
  existing authorized path. Do not cut over automatically.
- [ ] Exercise process kill, outbox retry, revocation during read, changed source, policy denial,
  generation rebuild and rollback in the consumer repository before enabling users.
- [ ] Register the exact USTE commit/lock digest and consumer adapter version in that repository's
  own dependency and acceptance records.

## Failure and rollback

On any integrity, key, checkpoint-version, policy-generation or reconciliation error, visibly
disable USTE shadow retrieval and continue only through the consumer's existing authorized local
path. Do not silently reset, downgrade or return stale cached results.

To roll back, stop/drop the adapter, disable shadow queries, preserve the consumer source/outbox,
and quarantine or remove only the exact derived-index directory according to the consumer's data
handling process. A later rebuild uses a fresh empty admitted root and a once-incremented authority
generation. Directory removal is not proof of physical erasure; backups, journal history and media
remnants remain outside M1's guarantee.

## Explicit limitations and remaining roadmap

M1 is capped and memory-resident. It does not provide larger-than-memory queries, arbitrary rich
parsers, embeddings/ranking, interval or derivation-time composition, spatial/geographic/history
queries, navigation, motion, physics, complete purge/backup/restore, authenticated IPC, production
operations, migration or independent security review. Procedures are inert data and grant no
execution authority.

USTE resumes T-20/T-19 after this handoff, then the unchanged R2–R4 roadmap. T-21–T-28, T-34–T-36,
T-50–T-61 and applicable acceptance/release gates remain open. T-62 private vulnerability-reporting
verification remains mandatory before executable distribution. Consumer integration admission,
runtime changes and authoritative migration remain separate consumer decisions.
