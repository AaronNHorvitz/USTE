# Decision 0209: Single-pass packed lookup authentication

Date: 2026-09-20

Status: Implemented; full workspace and ordinary native regression passed. Excluded from Decision 0208's binary.

Decision 0132 requires complete root binding, not trust in structural parsing. After Decision
0207, packed lookup still hashes every visited node once during decode, compares it against the
trusted root/parent claim, and then repeats the whole path hash fold at the terminal leaf.
Use the already authenticated chain for root binding and retain the identical terminal route
and proof-input admission checks without the second fold. Public logical proof verification
and delta application retain their complete hash fold; this change applies only to packed
lookup after its per-node commitment comparisons have all succeeded.

Equivalence argument: the first decoded node commitment must equal the caller's independently
trusted root. Each branch commitment binds its bit, both child summaries and their order under
the same logical scope/profile/family. Lookup follows the query-selected child, and the next
decoded node must match that exact summary. Inductively the terminal decoded leaf commitment
is bound to the root through exactly the branches the old fold would reconstruct. Every branch
composition has already performed the same entry/byte bound and overflow checks. Physical
locator, encrypted context and owner validation remain mandatory and unchanged at each step.

Cryptographic binding alone does not prove canonical routing. Keep strict increasing bits,
decreasing child counts, terminal leaf bit-length and query/leaf direction agreement. Keep
the existing proof-input admission, including its original 52-byte branch charge and exact
key/value/branch/byte limits. Only equal terminal keys yield membership; valid unequal keys
yield absence. The empty-root path remains unchanged. Present values still require complete
chunk-chain and value-hash validation; no partial output or provisional report escapes.

Extract only the structural route check into a crate-private helper explicitly documented as
NOT a root/membership proof. The ordinary full verifier shares that unchanged predicate and
still checks the reconstructed root. Do not expose the helper as a consumer capability or use
it without a separately authenticated exact node chain. No path allocation, cache policy,
authorization rule, wire format, durability behavior or logical work counter changes.

Tests compare 768 generated membership/absence/prefix cases with an independently encoded tree
and full hash proofs, pin exact/minus-one admission and malformed routing, and explicitly show
that the structural helper alone does not authenticate a changed sibling. Packed lookup adds
768 byte/prefix cases against its sorted reference. Every successful packed unit-test traversal
also executes the old complete hash-fold verifier as an assertion-only cross-check. Existing
self-consistent false partition/repeated-bit, context, ciphertext, budget, fault and restart
matrices remain mandatory. Full workspace and native regression must pass before acceptance.
Any new performance evidence needs a separately pinned executable after Decision 0208 finishes;
no running measurement includes these edits and no benchmark target is relaxed.

The focused storage run passed 229 tests, then the complete assertion-enabled workspace run
passed 730 tests across 47 executables with no failures/ignores. Strict all-feature Clippy and
warnings-denied docs passed. Workspace scope peak was 2,973,106,176 bytes with zero swap.
Native regression passed all 122 active cases, with five unchanged opt-in ignores, and strict
Clippy. Its final scope peak was 565,596,160 bytes with zero swap. PROGRESS.md records exact
commands and resource admission. No qualifying benchmark or performance claim follows from tests.
