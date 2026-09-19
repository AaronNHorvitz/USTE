# Decision 0138 — Bounded append-only certificate-bound packed roots

Date: 2026-09-19

Status: accepted and locally verified T-20 certificate-bound publication; domain integration open.

Index-v1's overwrite-slot protocol must scrub fallback runs before deleting a candidate. Applying
that protocol unchanged to packed trees would reintroduce a full-tree scan on every ordinary
publication. Keep the v1 protocol unchanged. For the new opt-in packed profile, publish only into
create-new, revision-specific opaque attempt slots. Never overwrite, truncate, delete or rename an
existing root or pack. All prior revision roots remain available; T-35 still owns reclamation.

Admit one through 64 attempts per exact revision/profile, with explicit discovery read-byte limits.
Derive slot names under the existing IndexName public-token role from a separately versioned
domain, namespace, index profile, revision and zero-based attempt. Keep the current journal key
epoch/writer binding, as in v1 optional cache names. A changed epoch/incarnation may require cache
rebuild; it does not reinterpret prior names or grant authority to stale files.

Each file contains a fresh nonzero opaque root object ID (16 bytes) and the exact Decision 0136
envelope (4,161 bytes). The authenticated manifest generation is attempt plus one. Write, set exact
length, sync file and sync directory before returning the recovered manifest. Malformed input is
refused before output creation. An occupied name consumes an attempt, never an overwrite decision.
Other operational failures propagate. Failed writes may leave partial optional files; subsequent
attempts preserve them. Exhausting the admitted attempts is an explicit resource refusal, not a
commit, success, false absence or permission to reclaim uncertain files.

Journal publication requires an unpoisoned owner and the exact current revision/certificate/scope.
Discovery accepts an existing bounded certificate-chain proof, validates its owner/frontier before
I/O, and reads only the admitted attempt slots at that exact revision. Exact-size, AEAD, scope,
profile, revision, generation and certificate checks precede returning candidates. Missing or
structurally/authentication-invalid optional files are omitted; I/O, locked-key and resource
failures propagate and cannot classify a candidate as corrupt. Discovery never enumerates a
directory or reconstructs resident certificate history.

The resulting handle is certificate-bound only. Root/family canonical validation, reducer/state
profiles, domain semantics and consumer authorization remain distinct prerequisites. Raw publication
is privileged maintenance and requires the caller's successful domain validation, as with the v1
publication surface; the cache cannot advance or roll back the journal. Ordinary new-revision
publication does no fallback tree reads because it removes no fallback. Exact-retry cache callers
may discover/reuse an admitted candidate; publication itself does not promise unlimited attempts.

Required tests cover exact certificate/scope/owner binding before I/O, immutable fallback retention,
bounded attempt exhaustion, every write/read/error/crash boundary, partial/tampered/context-swapped
manifests, corrupt-candidate versus operational-error classification, and cold discovery with the
resident certificate map empty. Production domain integration and qualifying BM-01/BM-06 remain
open; M1, v1, nonce-session ceilings and release gates remain unchanged.
