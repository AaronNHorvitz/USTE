# Decision 0136 — Separately versioned packed-root manifest framing

Date: 2026-09-19

Status: accepted and locally verified T-20 framing; publication/domain admission open.

Packed trees need their own fixed root manifest; do not reinterpret index-v1 run descriptors or
graph-state-v1's flat logical digest. In particular, introduce an explicit 32-byte state-commitment
profile alongside the reducer profile, state digest and index profile. Future domain admission
must select and validate that profile rather than silently treating an ordered aggregate as the
frozen v1 state hash. This framing decision does not select a new graph/coordinator reducer,
state-digest algorithm, root publication protocol or commit authority.

Use exactly 2,048 plaintext bytes, unsigned big-endian integers and zero reserved/unused bytes.
The header is 224 bytes: `UPRT` (4), major 2 (1), minor 0 (1), family count (1), zero (1), namespace
(16), revision (8), nonzero generation (8), exact certificate digest (32), reducer profile (32),
state-commitment profile (32), state digest (32), index profile (32), nonzero root object ID (16),
eight zeros. Database and namespace are also bound by authenticated encryption.

One through sixteen strictly increasing nonzero family descriptors follow, each 112 bytes:
family (1), seven zeros, entry count (8), logical key/value bytes (8), ordered content digest (32),
root-present flag (1), Decision 0131 locator storage (54), one zero. Empty families are explicitly
representable: flag zero, zero locator storage, zero counts and the exact context-derived empty
commitment. Nonempty families require flag one, a structurally valid nonempty logical summary
and a locator no newer than the manifest revision. Unused descriptors/tail bytes are zero.

Encrypt with unchanged crypto-v1, IndexPage role, object format 2.0, Small4KiB framing and sequence
zero (packed pages use positive sequences). The exact encrypted envelope is 4,161 bytes. The caller
supplies the exact scope, profile, object, key epoch and writer context; authenticate before parsing
or returning any descriptor. Fixed framing refuses length changes before envelope allocation.
All returned values remain raw maintenance data, not authorization or semantically admitted roots.

Required evidence pins literal field offsets and encrypted context separation, every truncation,
trailing/nonzero-reserved byte, ordering/duplicate/count/flag/empty-root and locator bounds, and
encrypted manifest round trips referencing old and new immutable packs. Publication/discovery,
independent journal/domain admission, real process recovery and complete measurement remain
subsequent work. Existing v1, M1, key-vault nonce ceilings and T-35 reclamation ownership are unchanged.
