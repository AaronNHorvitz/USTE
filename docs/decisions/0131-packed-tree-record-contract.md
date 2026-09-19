# Decision 0131 — Typed packed tree records

Date: 2026-09-19

Status: accepted and locally verified T-20 typed codec; traversal/domain integration open.

Decisions 0128–0130 provide logical commitments and encrypted immutable transport, not a tree.
Add a closed typed record grammar for that transport. All integers below are unsigned big endian.
Scope, profile and family are inherited from the authenticated containing page and are never
silently taken from a linked record. A locator is 54 bytes: nonzero pack object ID (16), nonzero
creation revision u64, nonzero key epoch u64, writer incarnation (16), page u32, slot u16.
Page and slot must satisfy Decision 0129 bounds. A child/chunk locator cannot have a creation
revision later than its containing page. A locator remains a physical claim, not a capability.

A tree-node payload starts with version 1, then kind 1 (leaf) or 2 (branch). A leaf appends key
length u16, nonempty key (at most 4 KiB), value length u32 (at most 16 MiB), value commitment
digest (32), then a zero/one first-chunk flag and, if one, a locator. Empty values have no chunk
and must have the exact Decision 0128 empty-value hash. Nonempty values require a first chunk.
Branches append bit u32 then left and right child references. Each reference contains a locator
and a logical summary: entries u64, key-plus-value bytes u64, digest (32). Children are nonempty,
bounded by the Decision 0128 entry/byte ceilings and have distinct physical locators. Composing
their summaries yields the branch commitment; parsing alone does not establish canonical key
partitioning or prove the referenced child actually has those contents.

A value-chunk payload starts with version 1, remaining-value bytes u32 (including this chunk),
data length u16, zero/one next flag, optional locator and exact data. The maximum chunk data is
15,729 bytes so even a nonterminal chunk fits one packed record. Data length is exactly the
minimum of that maximum and remaining bytes; remaining is nonzero and at most 16 MiB. A next
locator exists exactly when remaining exceeds data length. Traversal must match remaining bytes
to the expected leaf length and then decrement exactly; it must validate the complete value hash
before declaring successful value recovery. Nonzero padding, extra fields and trailing bytes
are not part of either payload grammar.

Decoding is borrowed, nonrecursive and allocation-free with fixed field checks before slicing.
Encoding uses bounded, fallibly allocated zeroizing plaintext. Physical location, pack layout,
epoch and writer do not enter the logical content commitment. Changing those fields may preserve
logical identity but must still pass exact authenticated context and referenced-content checks.
An imported logical summary is explicitly an untrusted claim; structural validation is not
canonical-root or journal admission.

Tests must pin exact typed bytes, round trips, all truncations/trailing bytes, bad tags/flags,
future or invalid locators, key/value/count/byte boundaries, chunk partition/termination rules
and independence of logical roots from physical layout. Subsequent work must implement bounded
authenticated traversal, whole-value checks, reachable-node copy-on-write batches, independent
root admission, publication/recovery and Linux process tests. No v1 format, M1 result, graph
digest, authorization rule or benchmark target changes with this typed codec.
