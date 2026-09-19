# Decision 0091 — Unpublished roots for certified multi-step recovery

Date: 2026-09-19

Status: T-20 storage capability; transaction/graph recovery integration remains open.

The graph recovery path currently permits a ready base or one pending revision. Extending it by
publishing older intermediate roots would violate the exact-current-frontier publication contract
and could replace a usable fallback prematurely. Preserve that contract and instead add explicitly
unpublished scratch roots for trusted recovery orchestration.

`IndexRecoveryStage` admits one existing certificate digest/revision and database scope, pinned to
the exact open journal frontier. Scope/frontier/poison checks precede work. A stage permits the
existing bounded, authenticated, exact-before-value run merge at its certified revision, including
an older revision than the current frontier. A supplied base must have the same scope and a strictly
earlier certified revision. Merge retains the existing per-run read/delta/output limits, complete
source authentication, terminal digest checks and provisional visitor semantics.

Finishing a stage validates input scope/revision/certificate and all run bindings, then assembles a
bounded `StagedIndexRoot` without filesystem access. Its borrowed read handle has generation zero
to distinguish the absence of a published root-slot generation. Only complete successful stage
outputs may be used for the next private recovery step. A handle is neither semantic validation,
authorization, a durable receipt nor a discoverable root. It must not escape as recovered consumer
state until all transaction/domain/coordinator checks and terminal publication succeed.

Intermediate run files may be durable but never change either root slot. Current-frontier root
publication uses the unchanged existing API and old/new fallback rules. A stale/poisoned stage
refuses further work; failed stages can leave only unreferenced derived files. Their reclamation
remains T-35, not destructive cleanup performed by this capability. No journal revision, persisted
format, nonce domain, accepted transaction meaning or durability requirement changes.

This is trusted storage functionality, not a consumer authorization capability. It does not remove
resident certificate/blob maps, validate domain result digests on its own, or complete multi-revision
graph recovery. Decision 0088's authenticated transaction cursor is the orchestration prerequisite;
the next increment must connect staged graph preparation/publication with existing exact retry,
collision and first-owner replay before exposing any coordinator.

Tests build two consecutive staged revisions behind a later frontier, read them through the normal
authenticated index path, verify historical durable publication still refuses, and restart after
terminal current-frontier publication. Every observed merge I/O occurrence is tested with an error,
crash-before and crash-after, preserving the original published base and authoritative frontier.
Additional cases reject wrong input/run bindings, non-forward bases, frontier changes, poisoned
writers, corrupt staged ciphertext and visitor failure without replacing a published root.
