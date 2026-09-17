# T-11 encryption and key-adapter evidence

Date: 2026-09-17 · Product toolchain: Rust/Cargo 1.95.0 · Target: x86_64-unknown-linux-gnu

Reviewed implementation: commit `bbe434138ac58c23c99378c7fa726cfecf7bc62f` · tree
`af0aedc33660fa4bea8aa8a99c6bb567e8c7eb3c`. Two read-only Codex agent audits compared the
implementation, tests and exact bytes with Decisions 0005/0013, FR-10 and the T-11 slice of VT-07.
Their findings about infallible object-sized allocation, rejected-password zeroization, context
coverage, authenticated recovery padding, guarded-memory wording, literal profile binding and
resource-error classification were corrected before the recorded commit. Final delta audits found
no T-11 blocker. This is automated implementation review, not independent human security review or
production certification.

This record closes T-11's local encryption/key-adapter boundary. It does not close full VT-07:
T-13/T-36 own durable incarnation, clone/restore admission and crash integration, T-35 owns key
rotation, T-39 owns hardening campaigns and T-41 owns independent assessment.

## Implemented contract

- Safe first-party `uste-crypto` code wraps pinned RustCrypto XChaCha20-Poly1305, HKDF-SHA-256,
  Argon2id and OS entropy. It depends on `uste-types`, contains no network/filesystem/database
  access, forbids first-party unsafe code and adds no native `links` package.
- Decision 0013 assigns the exact 49-byte object and 66-byte recovery headers, big-endian fields,
  version/suite dispatch, derivation/AAD domain strings and fixed 88-byte context. The context binds
  database, scope, namespace, epoch, role, object, sequence, writer incarnation, object format and
  padding frame.
- Exact plaintext bytes use an authenticated inner length and zero padding to 4 KiB or 64 KiB.
  One envelope admits at most 16 MiB. Lengths are checked before fallible caller-sized buffer
  reservation; encryption/decryption is in-place and decrypted owned buffers zeroize on drop.
- A vault generates a non-cloneable master key, exposes it only to the trusted adapter boundary,
  supports explicit lock/unlock and fails while locked. Master, derived, password and decrypted
  USTE-owned buffers are redacted and best-effort zeroizing.
- Every encryption consumes a 192-bit entropy-supplied nonce. Entropy failure produces no envelope;
  same-session duplicates fail, and 1,048,576 nonces exhaust the session without a reset API.
- The portable recovery adapter fixes Argon2id v1.3 at 262,144 KiB, three iterations, one lane,
  random 128-bit salt and a random XChaCha nonce. Recovery credentials are 1..=1,024 bytes. Wrong
  password/database, tampering, downgrade and authenticated nonzero padding never yield a key.
- Stable public errors contain no dynamic upstream text or secret context. Resource failures remain
  retryable and are distinct from credential/tamper failure without revealing which secret failed.
- `acceptance/r1/crypto-v1.tsv` pins every public limit/profile value and the deterministic envelope
  SHA-256 `3ae2282b8e08bc0564bd863c0b10675508f4cf643b3ce905c9b7d7cea8796c46`.

## Verification

~~~text
cargo clippy -p uste-crypto --all-targets --all-features -- -D warnings
cargo test -p uste-crypto --all-features
# passed: 2 unit, 10 integration and doc tests; real fixed Argon2 profile took about 25 seconds

bash scripts/check.sh
# passed: format/clippy/test/doc, documentation/task graph, R0/publication/content suites;
# 45 workspace tests passed

CARGO_DENY_BIN=/tmp/uste-t09-tools/bin/cargo-deny bash scripts/check_supply_chain.sh
# all five lockfiles: 0 advisory/license/source errors; only the two previously documented
# miniz_oxide duplicate warnings in parser experiment graphs
~~~

The negative matrix changes every authenticated context field, every ciphertext/tag byte and every
truncation point. It also covers wrong key, locked encrypt/decrypt, failed unlock remaining locked,
duplicate nonce, entropy failure, writer-incarnation separation, unknown versions, malformed and
overflowing lengths, exact maximum plaintext, both padding classes, recovery parameter downgrade,
wrong password/database, ciphertext damage, and an authenticated malformed-padding fixture.

## Deliberate limits

`KeyAdapter` is a trusted interface; a malicious adapter can copy a key it is authorized to wrap.
No OS key-store adapter or guarded/locked-page implementation is claimed. Standard-library bounded
nonce registries do not provide recoverable allocation failure, so process allocator exhaustion may
abort. Zeroization cannot promise removal of compiler, dependency, allocator, swap or crash-dump
copies and does not protect a compromised process or kernel.

Random nonces provide probabilistic cross-restart uniqueness. This crate authenticates a supplied
writer incarnation but cannot detect copied directories, durably publish a new incarnation, rotate
epochs or prove whole-state freshness. Those remain storage/lifecycle responsibilities; no data is
durable merely because this boundary can encrypt an envelope.
