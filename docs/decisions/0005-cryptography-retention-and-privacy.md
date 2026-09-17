# Decision 0005 — Cryptography, keys, retention and restore

Date: 2026-09-16

Status: accepted for strict profile `crypto-v1`; OS key-store integration remains an adapter,
not a claim about an unavailable desktop secret service.

Closes D-02 and D-04 and is the decision artifact for T-03.

## Suite and key hierarchy

Use RustCrypto `chacha20poly1305 0.11.0` XChaCha20-Poly1305, `hkdf 0.13.0` with SHA-256,
`argon2 0.6.0` Argon2id for password recovery wrapping, and `getrandom 0.4.3` for OS entropy.
No cryptographic primitive is implemented by USTE. Versions are exact Cargo.lock inputs and
their resolved features/transitives require `cargo deny` plus source/unsafe review.

The random 256-bit database master key is never stored plaintext. A key adapter wraps it;
the portable recovery adapter uses Argon2id with a random 128-bit salt, 256 MiB, 3 iterations
and one lane, followed by an authenticated wrapping envelope. Resource-constrained profiles
require a new decision, never silent weakening. HKDF derives independent keys by database ID,
key epoch, namespace where applicable, object role, object/segment ID and format version.

Every AEAD record uses a fresh random 192-bit nonce from the OS CSPRNG. Encryption fails
closed on entropy failure. Random-nonce collision probability is bounded by the 192-bit space;
tests replace entropy deterministically to prove duplicate detection within each writer
session. Immutable blob chunks additionally have independent per-object derived keys and
their chunk number in authenticated context. Journal and index runs rotate to fresh random
segment IDs/keys; a restored database creates and durably records a new writer incarnation
before accepting writes. Database clones must be explicitly opened as read-only or rekeyed to
a new identity/incarnation; two writable byte-for-byte clones are unsupported and rejected
when the operator supplies the shared clone lease token.

Authenticated data includes database/namespace identity, role, format/profile, key epoch,
object identity, sequence/chunk number and declared public frame fields. Public leakage is
limited to directory existence, coarse file class, padded ciphertext length, modification
timing and access timing. Names, source media types, logical hashes, index keys and payloads
are encrypted. Blob chunks pad to 64 KiB except the authenticated terminal length; small
records use 4 KiB framing. Traffic-analysis resistance is not claimed.

Keys are unlocked into guarded process memory on a best-effort basis and zeroized on normal
lock/drop; Rust/OS copies, swap, crash dumps and a compromised kernel remain limitations.
Rotation writes new epochs and rewrites reachable objects transactionally before retiring an
old key. Key loss is unrecoverable without a valid recovery wrapper. Logs contain stable
redacted error codes, never keys, plaintext, raw source digests or low-entropy equality tokens.

~~~text
OS CSPRNG -> database master key -> wrapped recovery/key-store envelope
                                  -> HKDF(database, epoch, namespace, role, object)
                                     -> per-object/segment AEAD key + fresh 192-bit nonce

active epoch -> write/rewrite reachable objects -> verify new epoch -> retire old wrappers
purge request -> revoke access/leases -> dependency plan -> deletion epoch/receipt
              -> bounded pin expiry -> physical reclamation -> backup expiry/ledger enforcement
~~~

Rotation never retires an old wrapper before every retained reachable object is verified under
the new epoch. Purge access revocation is immediate on commit; reclamation and backup expiry are
separately reported stages.

## Retention epochs and purge

Each namespace has an append-only policy version and database-wide deletion epoch. Default
v1 policy retains accepted history and idempotency outcomes for 30 days, uncommitted uploads
for 24 hours, worker scratch for at most one hour after lease end, cache checkpoints until
superseded, and expired branches for seven days. A namespace may shorten/extend those values
within the D-03 caps. Legal/operational holds are explicit authorized records with an expiry;
an active hold produces `PurgeBlocked`, never false success.

Purge first revokes access and leases, then computes a bounded dependency plan covering
source versions, derivations, indexes, summaries, trajectories, route caches, branches,
staging and backup inventory. It publishes a new deletion epoch and content-minimal receipt
atomically. Physical reclamation may follow after bounded reader/branch pins expire (maximum
24 hours; purge can terminate them). Shared bytes are erased only when no authorized retained
reference remains. Namespace deduplication never crosses namespaces.

Backups contain their deletion epoch. Restore always targets a new path and is quarantined
until its inventory/authentication is verified and a trusted current deletion ledger is
applied. A backup older than the trusted current epoch is `StaleDeletionEpoch`; if no current
ledger is available it cannot be promoted as current. Operator copies and exported old keys
outside engine control are an explicit limitation. Retention compaction establishes an
earliest revision and verified baseline; earlier reads return `HistoryUnavailable`.

## Security implications and acceptance

XChaCha's extended random nonce avoids a crash-sensitive global counter but gives a
probabilistic, not mathematical, uniqueness guarantee. Whole-state rollback is detectable
only with a trusted external epoch/anchor; the local profile makes no stronger claim.
Password strength remains an operator responsibility.

`acceptance/r0/security-lifecycle.tsv` enumerates wrong-context, nonce, clone, key-loss,
hold, purge, stale-backup and shared-reference cases. VT-06/07/13 and crash tests must pass
before the corresponding gates close.
