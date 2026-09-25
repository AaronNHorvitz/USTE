# Decision 0276: `research-memory-v1` canonical record codec

Date: 2026-09-24

Status: Accepted for the development implementation of DB-R02.2; DB-R02 remains open.

Decision 0275 froze the research memory record contract. Before a reducer can admit those records
(DB-R02.3), each kind needs one canonical, versioned, fail-closed byte form with the contract's
semantic rules enforced at the boundary.

Add `uste_memory::research` with the frozen `RESEARCH_PROFILE`, typed `Source`, `Artifact`,
`Claim` (with `Citation`) and `Edge` inputs, and `encode_research_record` /
`decode_research_record`. Records use a 40-byte header (`URSM`, format 1.0, kind, a zero reserved
byte and the 32-byte namespace) followed by a fixed field order: big-endian integers,
`u32`-length UTF-8 strings, `u16` list counts, strict 0/1 presence and boolean bytes, and closed
enum tags. It reuses the pilot's `SourceVersionId` and `SourceLocator` shapes and the blob
reference layout. Decoding refuses oversize input before parsing, unknown magic/versions/kinds,
non-zero reserved bytes, non-canonical flags or tags, truncation and trailing bytes, and then
re-runs the same semantic validation the encoder applies, so every accepted byte string is the
unique encoding of its record.

Semantic rules: every reference, blob and citation stays in the record's namespace; source
versions start at 1; `Inaccessible` outcomes carry no content and every other outcome does;
truncated content is within its declared limit; `max_age` is non-zero; artifact inputs are
strictly ascending and non-empty; an `Unsupported` claim has no citation and every other support
kind has at least one; excerpts, when retained, hash to their recorded digest; citations are
unique by source and locator; locators are non-empty with 1-based ordered line numbers; intervals
are half-open with start before end; a claim cannot correct itself; an edge cannot be a self-loop;
and every string and list respects the frozen limits.

The codec performs no storage, network, search, tool or model activity and grants no authority.
It is not yet wired to a reducer, query surface or durable profile, so no capability is claimed.

Verification (focused, under the shared heavy-work reservation with one Cargo job and one test
thread): nine new codec tests cover round trips of seven fixtures to identical bytes, a
hand-built byte layout for the edge kind, pinned golden digests, every truncation and trailing
byte, versions, kinds and reserved bytes, every single-bit-7 byte change (refused or a distinct
record that re-encodes to the same bytes), each semantic rule on encode and decode, revalidation
of structurally valid but semantically invalid bytes, and oversize refusal. The golden digests
were pinned from the first reviewed run after the complete-source and edge digests were
reproduced by an independent reimplementation of the specified layout. `uste-memory` tests (14
unit and 1 integration), strict Clippy for the crate, the memory adapter and the workspace,
warnings-denied rustdoc for the crate, the memory adapter tests, formatting and the docs and
task-graph checks passed. The full `scripts/check.sh` gate was not rerun for this additive
module; the next coherent checkpoint gate covers it.
