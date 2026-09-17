# T-18 deterministic replay and encrypted checkpoint evidence

Date: 2026-09-17 · implementation commit: `595c47a` · local correctness
evidence, not an independent security certification

## Implemented result

- Added the safe-Rust `uste-replay` kernel for contiguous cold replay, pre-publication result-digest
  verification, deterministic genesis/final logical digests and fallible canonical reducer
  checkpoints without filesystem, clock, parser, model or network access.
- Added the graph checkpoint codec for current records, complete record histories, current durable
  policy and policy history. Decode rejects noncanonical order, invalid revisions/transitions and
  historically impossible references, then rebuilds and independently validates derived indexes.
- Added canonical coordinator cache bytes containing the exact journal certificate anchor,
  reducer metadata, retained retry/transaction outcomes and first-commit blob ownership. Encode
  and decode share ordering, uniqueness, revision and count limits.
- Added an opaque authenticated checkpoint carrier and provenance-bound recovery seed. Seeded open
  reauthenticates the full journal, reconstructs and exactly compares all prefix coordinator
  metadata at the anchor, and applies the normal reducer only to the suffix.
- Added an encrypted two-slot cache owned by the journal writer: 1 MiB chunks, 256 MiB total,
  authenticated namespace/object/sequence context, terminal manifest publication, exact historical
  certificate filtering and older-slot/cold-replay fallback.
- Preserved journal authority. Cache publication creates no commit, invalid cache objects are
  optional, and exact current-frontier certificate matching is required before publication.

## Focused verification

~~~text
cargo test -p uste-replay --all-targets --locked
# 6 unit + 2 integration passed; 0 failed
cargo test -p uste-storage checkpoint --locked
# 5 checkpoint-focused tests passed; 0 failed
cargo test -p uste-graph --test checkpoint --locked
# 6 passed; 0 failed
cargo test -p uste-graph --test authorized_graph \
  encrypted_graph_checkpoint_matches_cold_state_and_replays_only_the_suffix --locked
# 1 passed; 0 failed
cargo clippy -p uste-storage -p uste-txn -p uste-replay -p uste-graph \
  --all-targets --locked -- -D warnings
# passed
~~~

The storage matrix injects crash-before and crash-after at fourteen replacement boundaries:
manifest invalidation and directory sync; chunk remove/create/write/size/file sync and directory
sync; manifest create, both writes, size, file sync and terminal directory sync. All 28 cases reopen
to the prior complete slot or the exact new slot and an idempotent retry converges.

Adversarial cases cover missing, corrupted and position-swapped chunks; corrupted manifests;
wrong/future/divergent certificate anchors; noncanonical and over-limit authenticated manifests;
malformed authenticated coordinator counts; future outcome revisions; duplicate transaction IDs;
wrong reducer scope/profile/revision/digest; every graph payload truncation; invalid graph history
and historical reference closure. The encrypted graph end-to-end case checkpoints revision one,
commits a revision-two suffix, restarts, reauthenticates the journal, restores the cache, applies
the suffix and compares the complete snapshot and rebuilt indexes with the pre-restart state.

Review found and the implementation closed: equal-generation split-brain publication, historical
rather than frontier-only anchor matching, public checkpoint/seed forgery, infallible large-frame
copying, and encoder/decoder invariant drift. No high- or medium-severity code or evidence finding
remained after the final review.

## Limits and next owners

The journal is scanned twice during candidate selection and seeded reopen. This favors a closed
ownership race over recovery speed; BM-06 is not claimed. Checkpoints remain capped in-memory
correctness caches and do not satisfy T-20 disk-index/cache pressure. T-35 owns retained-baseline
promotion, compaction and cache cleanup policy; T-38/T-39 own backup/restore epochs and migrations.
Unsupported or damaged caches fall back to an older candidate or cold replay.

T-62 remains an external distribution prerequisite: GitHub private vulnerability reporting has
not been owner-verified. It does not block this local implementation evidence and is not claimed
complete here.
