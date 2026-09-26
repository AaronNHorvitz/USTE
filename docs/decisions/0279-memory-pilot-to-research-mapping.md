# Decision 0279: Read-only memory-pilot to research-memory mapping

Date: 2026-09-25

Status: Accepted for the development implementation of DB-R02.5; not a migration.

The [research memory contract](../research-memory-records.md) promised a read-only mapping from
the frozen `memory-pilot-v1` records, with equivalence fixtures, before any migration is
qualified. The pilot's M1 profile and evidence must stay unchanged.

Add `map_memory_pilot`, which plans an ordered list of research mutations, each with the exact
blob inventory a mapped source needs, from a ready pilot generation. It performs no I/O, never
mutates the pilot, and applies nothing; committing the plan into a research store is a separate,
unqualified step. Events follow pilot revision order, so replaying the plan after `BeginRebuild`
rebuilds the same lifecycle: each source version maps to an immutable (`Pinned`) supplied
document with its original blob and media type; each fact maps to a `Paraphrase` claim with one
citation over the same source version and locator and its original correction link; each pilot
link maps to a `Related` edge asserted by the linking fact, with a deterministic hash-derived
identity; retractions and source revocations map to the same research mutations.

Nothing unstated is invented. The pilot recorded no retrieval time, license or locator text, so
mapped sources carry the consumer-declared mapping instant as `retrieved_at`, a fixed mapping
route label, and an explicit "unrecorded" license label. Pilot event times have no research field;
the mapping reports how many it drops instead of relocating them. It refuses, with a typed error,
a pilot that is not ready, a fact citing an opaque source (its excerpt digest would need a blob
read), and a fact citing an empty span (which the research contract cannot represent). Excerpts
are copied only when the cited bytes are valid UTF-8 within the excerpt limit; the digest always
covers the exact cited bytes.

Verification (focused): a pilot state built through the pilot's own transactions (two source
versions, a second source, facts with a link, a correction, a retraction and a revocation) maps
deterministically, replays into `ResearchState`, and matches record by record: blobs, revocation
and supersession of every source version; subject, predicate, value, correction, citation source
and locator, and status of every fact; the exact excerpt and digest; and the link edge. Readiness,
opaque-citation and empty-span refusals are covered. All `uste-memory` tests, strict workspace
Clippy, warnings-denied rustdoc and memory-adapter tests passed; the full gate was not rerun.

With DB-R02.1 through DB-R02.5 implemented, DB-R02 still stays unchecked until a full gate and
review cover it; DB-R03's adversarial lifecycle matrix remains its own package.
