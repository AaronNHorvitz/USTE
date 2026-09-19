# Decision 0073 — Bounded first-reference maintenance with root-set repair

Date: 2026-09-18

Status: T-20 partial implementation; storage metadata and qualification remain open.

Retain Decision 0072's admitted first-reference root inside the disk metadata base. Add
`rebase_metadata_with_first_references`, whose extra owner/group/encoded-byte limits apply to one
authenticated post-base journal pass. Collect first revisions only for the already bounded,
disjoint new-owner overlay; match exact references and first principals and require every overlay
owner to appear. No pre-base journal scan or complete owner map is reconstructed.

Merge those sorted insertions into the prior first-reference run using the existing bounded native
merge. Require exact output count and insertion count, with no replacement/deletion. A suffix with
no new owners still advances the proof anchor while preserving its contents. An owner-free base
can bootstrap its first proof from its bounded suffix; a legacy base with existing owners must
first admit independent evidence and cannot silently manufacture history during rebase.

Publish metadata, transaction and optional first-reference roots as one recoverable derived set.
Keep the old base and all overlays until all required publications succeed. Existing current-root
reuse compares exact logical content and resynchronizes it without rotating away the old matching
set. Partial publication blocks fresh writes, not exact retries; restart may admit the retained old
set and replay the bounded suffix before repair. Detect an intermediate newer root across all
three profiles; the existing explicit-rebuild barrier remains, not destructive cache cleanup.

The legacy `rebase_metadata` entry point refuses a base retaining first-reference evidence instead
of silently dropping it or inventing an unbounded suffix budget. Uncertain coordinators still fail
with `OutcomeUnknown` before maintenance admission. No M1 interface or pinned handoff changes.

Verification covers bootstrap from an empty owner set, owner-free suffix advancement, exact
first-owner retention across later-principal reuse, one-short suffix budgets, cold single-pass
readmission, exact retries and complete repair. Publication faults are enumerated from a successful
fixture across create/write/length/file-sync/directory-sync/removal operations, each with an error,
crash-before and crash-after. Every injected boundary must fire; old state/overlays remain usable
for repair, and the new three-root set must subsequently pass independent cold admission.

This maintains optional first-reference evidence without the legacy full-owner bridge after
bootstrap. It does not remove storage's resident certificate/blob maps, turn full-run merge into
an asymptotically incremental tree, supply garbage collection, or qualify BM-01/BM-06.
