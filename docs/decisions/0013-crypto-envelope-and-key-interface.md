# Decision 0013 — `crypto-v1` envelope and key interface

Date: 2026-09-17

Status: accepted for format 1.0. This implements the suite selected by
[Decision 0005](0005-cryptography-retention-and-privacy.md); it does not define a new
cryptographic primitive, claim an OS key-store implementation or complete rotation/clone policy.
Changing an assigned byte, derivation string or fixed cost requires a new compatibility decision.

## Derived object keys and authenticated context

The 256-bit database master key is non-cloneable at the public Rust boundary and is retained in a
zeroizing owner while unlocked. HKDF-SHA-256 uses the literal extract salt
`USTE crypto-v1 HKDF-SHA256 extract` and expands the concatenated info parts
`USTE crypto-v1 object-key` and this 88-byte context:

| Offset | Bytes | Meaning |
|---:|---:|---|
| 0 | 16 | database ID |
| 16 | 1 | scope: `00` database or `01` namespace |
| 17 | 16 | namespace ID, or all zero for database scope |
| 33 | 8 | nonzero key epoch, unsigned big endian |
| 41 | 1 | assigned object role |
| 42 | 16 | object or segment ID |
| 58 | 8 | sequence/chunk number, unsigned big endian |
| 66 | 16 | durable writer-incarnation ID |
| 82 | 1 | object-format major |
| 83 | 1 | object-format minor |
| 84 | 1 | padding frame: `01` 4 KiB or `02` 64 KiB |
| 85 | 3 | reserved authenticated zeros |

Roles `01` through `08` are journal group, commit certificate, blob chunk, snapshot, index page,
backup, temporary spill and worker output. Decision 0015 assigns role `09` to the immutable
creation manifest; Decision 0017 assigns `0A`–`0C` to blob inventory, manifest and opaque inventory
name; Decision 0025 assigns `0D` to the opaque index-root-name domain. Free-form role strings are
not admitted. The XChaCha
associated data is the literal `USTE crypto-v1 AEAD`, the complete public envelope header and the
complete context above, in that order. Consequently a wrong database, scope, epoch, role, object,
sequence, incarnation, format or frame fails authentication.

## Encrypted object envelope

The format-1.0 public header is exactly 49 bytes, followed by ciphertext and its 16-byte tag:

| Offset | Bytes | Meaning |
|---:|---:|---|
| 0 | 4 | ASCII `USTE` |
| 4 | 1 | object-envelope kind `80` |
| 5 | 1 | major `01` |
| 6 | 1 | minor `00` |
| 7 | 1 | XChaCha20-Poly1305/HKDF-SHA-256 suite `01` |
| 8 | 1 | public padding-frame tag |
| 9 | 8 | key epoch, unsigned big endian |
| 17 | 24 | fresh random XChaCha nonce |
| 41 | 8 | ciphertext-plus-tag byte count, unsigned big endian |

Authenticated plaintext is `actual-length:u64be || exact bytes || zero padding` to the selected
4 KiB small-object or 64 KiB blob frame. Nonzero padding is invalid. One envelope admits at most
16 MiB of exact plaintext, length arithmetic is checked, and object-envelope byte buffers use
fallible reservation. Decoding requires an integral number of complete padding frames with no
trailing bytes. Public leakage is limited as described in Decision 0005; the public role and object
identity are not in this header.

Each encryption obtains a fresh 192-bit nonce from the injected entropy capability and fails
without output on entropy failure. A writer vault detects repeated nonces in its current in-memory
session and stops after 1,048,576 issued nonces. There is no API to clear that set: exhaustion
requires a durably published new writer incarnation and a new vault. Process restart begins a new
random-nonce session, so cross-restart uniqueness remains the probabilistic guarantee accepted by
Decision 0005. Restore and writable-clone admission must still enforce durable incarnation and
clone rules in T-13/T-36; this crate alone cannot identify two copied directories.
The bounded standard-library nonce registries do not provide recoverable allocator failure; process
allocator exhaustion may abort under the selected Rust runtime policy.

## Recovery wrapper and adapters

`KeyAdapter` is a trusted, narrow wrap/unwrap boundary. It receives the database ID and a borrowed
master key; implementers must own credential prompting, secret-service policy and redacted errors.
USTE currently provides only a portable recovery adapter, not an OS key-store availability claim.

Its 66-byte public header is `USTE`, kind `81`, version `01 00`, suite `01`, Argon2id-v1.3 profile
`01`, memory `262144` KiB as `u32be`, iterations `3` as `u32be`, lanes `1` as `u8`, random 16-byte
salt, random 24-byte nonce and `u64be` ciphertext length. Argon2id derives exactly 32 bytes. The
encrypted 4 KiB plaintext is the 32-byte master key followed by zeros; its AAD is the literal
`USTE crypto-v1 recovery-wrap`, the complete header and database ID. Lower cost, another profile,
wrong database/password, malformed padding or damaged ciphertext never yields a key.
Recovery credentials are nonempty and capped at 1,024 bytes before Argon2 processing; rejected
owned buffers are also zeroized on normal drop.

USTE-owned master-key, derived-key, password and decrypted padded buffers are zeroized on normal
drop as a best effort. Dependency/optimizer temporaries may still exist. Public secret types are
not cloneable and their diagnostics are redacted. Stable errors do not include upstream
diagnostics, plaintext, key material, credentials or raw context.
Rust/optimizer copies, allocator behavior, swap, core dumps and a compromised process/kernel remain
explicit limits; this is not a guarded-memory or side-channel-resistance claim.

## Acceptance

`acceptance/r1/crypto-v1.tsv` pins the profile, bounds and deterministic object-envelope SHA-256.
VT-07 tests exact-byte round trips, both frame classes, every ciphertext-byte mutation and every
truncation, wrong key and each context field, malformed versions/lengths, entropy failure,
duplicate nonce, bounded exhaustion, writer-incarnation separation, lock/unlock, real fixed-cost
Argon2id recovery, wrong credentials, recovery tampering and diagnostic redaction. T-35 still owns
rotation and T-36 owns backup/restore and clone lifecycle completion.
