# Decision 0175 — Privileged per-vault decrypt-work measurement

Date: 2026-09-19

Status: implemented and locally verified. T-20 and benchmark qualification remain open.

Measure cryptographic work at `KeyVault::decrypt`, independently of cached proof-work units and
filesystem adapter calls. A fixed-size synchronized accumulator records successful calls, failed
calls, authenticated encoded-envelope bytes (49-byte header, padded ciphertext and tag), and exact
unpadded plaintext bytes returned by successful calls. No keys, identities, roles, digests or
plaintext content are retained. These counters are still cardinality-sensitive operator data.

Count completed calls, including failed database-context/locked-vault checks, but do not describe
every failed call as an attempted AEAD operation. Successful decrypt includes the existing padding
validation, not higher-level storage framing, root/proof or semantic admission. Structural envelope
decode refusals before the vault, key unwrap, encryption, filesystem/device bytes and other vault
instances are outside this report. Cache hits do not execute decrypt and contribute no new work.
This is a per-owner measurement foundation, not complete authenticated-I/O accounting.

The accumulator holds no key or plaintext, allocates no history-sized state and does not hold its
mutex during cryptography. Reports are coherent snapshots of completed calls. Every addition is
checked; overflow invalidates the complete report permanently. Poison likewise refuses reports.
Diagnostic failures never replace a decrypt result, authorize a read, reset nonce tracking or alter
durability. Counts survive lock/unlock and cache clear; a genuinely new vault starts new counters.
No format, ciphertext, derivation, padding, primitive, entropy or writer-session bound changes.

Raw vault/journal/coordinator reports are trusted maintenance capabilities. Journal poisoning and
coordinator uncertainty refuse reporting. No consumer facade exposes vault counters: a namespace
`ManageSchema` grant does not authorize database-wide historical work. Review removed an initial
uncommitted reader method for that reason. Reports aggregate all operations through the same owning
vault, not just one reader, namespace query or cache. Trusted benchmark operators already own the
raw coordinator; actual queries and cache diagnostics continue through their authorized reader.
Both consumer reader and own-outcome metadata facade remain unchanged.

Tests cover exact small/blob framing sizes, unchanged golden ciphertext and errors, lock/unlock,
concurrency, every counter overflow, poison, actual decrypt/nonce behavior after diagnostic failure,
and before-vault decode exclusion. Authorized packed tests compare misses against exact decrypt
counts/bytes, retained hits against unchanged counts, uncached reads against actual adapter reads,
shared-owner observation, denied/foreign/absent reads, durable read revocation and
five-family late ciphertext corruption. Full verification and resource evidence belong in PROGRESS.

The corrected full workspace passes 664 tests with zero failures/ignored cases, all-features strict
Clippy and warning-denying documentation. Benchmark wiring is a separate, subsequently verified
increment; no complete I/O, scalability or performance claim follows from these counters.
