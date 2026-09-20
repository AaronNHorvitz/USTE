# Decision 0162 — Owner-bound packed page caching

Date: 2026-09-19

Status: implemented with local reference/fault verification; T-20 qualification remains open.

Add an opt-in bounded decrypted cache for packed lookup/cursor reads. Preserve uncached APIs for
cold admission, corruption checks and reference testing. Reuse the safe slot-addressed LRU
implementation behind the existing v1 cache through an internal generic representation; preserve
its policy, admission limits, tests and behavior.

Packed cache identity includes the complete physical context: database/namespace, key epoch,
writer, creation revision, profile, family, object and page. Bind each populated cache to the exact
live journal certificate owner and unlocked vault session; a different owner or lock/unlock cycle
cannot reuse plaintext even with identical physical identities. A new opaque, process-local
vault-session token changes only on successful unlock/create; locking invalidates it. This adds
no key material, crypto derivation, persisted format or nonce-session reset. Key lock prevents
further access immediately; the independent cache drops retained pages on its next checked
access or explicit clear/drop, not synchronously at vault lock. Private proof readers can retain
a bounded transient page handle across eviction; resident accounting excludes this reader work.
Clear drops cached page ownership and
owner/session binding. Every operation still checks its
opaque tree/cursor owner, key availability, scope and revision before a cache lookup. No cache
entry establishes journal authority, canonical semantics or caller permission.

Only authenticated, structurally valid immutable pages enter the cache. Keep zeroizing page
ownership, bounded fixed/entry metadata allowances and exact LRU eviction; no history-sized
recency log. Logical accounting is not process RSS or physical erasure. Cache hits may retain
previously authenticated immutable bytes despite a later disk mutation; explicit clear/uncached
reads reauthenticate disk. Failed reads never insert partial pages. Returned slots and typed
records are still validated; no raw cache capability escapes the authorized consumer layer.

Keep proof-work admission independent of cache warmth: each successful page access, including a
hit, consumes the same page/encoded-byte work allowance. Document these as work units, not
physical reads. Separate cache hit/miss/eviction observations from adapter I/O and do not claim
complete physical accounting. Preserve sticky cursor failures and zero-I/O pre-admission refusal.

Test encrypted cold/warm equivalence, exact LRU and bounded allocation, every context component,
owner changes, locked keys, empty/exhausted cursors, corruption before insertion/after clear,
faults, and cache-independent resource refusal. Graph facade configuration and qualified native
cache-pressure campaigns remain subsequent integration work. No benchmark threshold changes.

Local verification adds eleven tests: eight storage tests (including 27 cold lookup/reopen and
54 directional cursor injected read/crash cases), two scoped transaction-wrapper tests and one
vault-session lifecycle test. The 20,000-access independent LRU trace checks exact hits, misses,
evictions, context separation and metadata allowance. Full workspace verification passes 641
tests across 47 executables, Clippy and documentation builds. See PROGRESS for exact commands,
candidate baseline, resource limits and native regression disposition. No persisted format,
crypto primitive, nonce registry, M1 consumer interface or benchmark target changes.
