# Decision 0178: Fresh buffered canonical admission

Date: 2026-09-19

Status: Accepted

Decision 0177 exposes repeated cold-admission authentication. Add an opt-in trusted storage
operation, `JournalStore::admit_packed_tree_buffered`, which creates its own bounded packed-page
cache, binds it to the checked certificate owner and current unlocked key session, and destroys
it before returning. Callers specify the existing allowed byte budget, never supply retained
pages, and receive only the ordinary canonical capability, proof-work report and cache counters.
Empty trees, missing families, foreign owners, stale owners and locked keys retain their checks.
Subsequent admissions start fresh and cannot conceal corruption behind an earlier warm cache.

The existing complete canonical DFS and streamed value hashing are shared unchanged. Each cache
hit consumes one page and 20,545 encoded-byte proof units, exactly as an uncached read; all node,
depth, logical-byte, page and encoded-byte ceilings remain. Counters distinguish actual cache
misses/decrypts from proof-work units. Cache residency uses Decision 0162's fixed accounting,
bounded eviction and secret-page lifetime rules; this is neither an RSS limit nor physical erasure.
Immutable bytes already authenticated within a single admission can be reused. As before, a
completed admission does not promise protection against concurrent or later file modification.

The uncached API remains available and unchanged. No consumer authority, journal format,
commit semantics, benchmark threshold or new persistent index is introduced. This first slice
does not yet buffer graph semantic admission or coordinator journal correspondence; those need
separate integration and tests before any new native performance claim.

Tests compare reports/results at one-page and larger cache budgets, enforce all five exact
proof ceilings and cache bounds, bind cache misses to decrypt diagnostics, reject authenticated
self-consistent noncanonical trees and later ciphertext corruption, and sweep every actual
read's error/crash-before/crash-after boundary with successful fresh-owner recovery. Existing
uncached canonical, content-hash, corruption, owner/session and fault suites remain required.
