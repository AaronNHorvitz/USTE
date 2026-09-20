# Decision 0207: Retain validated packed-record commitments

Date: 2026-09-20

Status: Implemented; full workspace and ordinary native regression verified.

T-20's native packed lookup decodes every visited tree node, then compares its logical commitment
against the authenticated root/parent claim. Decode already computes that same commitment as
part of mandatory validation. Return it from a crate-private decoding helper and reuse it for
the immediate comparison. The public decoder remains a wrapper with identical validation.
Apply the same reuse to ordered cursors, full-tree validation and private batch inspection,
which have the identical adjacent decode/recompute/compare pattern. Keep their comparison
and all subsequent traversal, staging and admission checks in their original order.

This removes one redundant hash per visited tree node without introducing a cache or weakening
admission. The independently trusted root, authenticated page/link checks, context binding,
parent commitment comparison, strict path ordering/decreasing counts, complete logical proof
verification, value-chunk checks and exact logical resource charges remain unchanged. Returned
commitments alone do not authenticate a root. No wire format, public API, authorization, retry,
durability or consumer-handoff change is involved.

Extend record regression checks to compare the retained commitment with explicit recomputation,
including malformed encodings, byte mutations and context changes. Run storage corruption/fault,
lookup/budget/reference tests and workspace regressions before acceptance. Any performance claim
requires a separately pinned measurement. Decision 0206's active binary predates this change and
must not be rebuilt or relabeled while its sampling workload runs.

Full assertion-enabled workspace verification passed 726 tests across 47 executables, including
225 storage tests and the retained-commitment mutation/context regression. Strict all-feature
workspace Clippy and warnings-denied docs passed. The scope peaked at 3,221,450,752 bytes with
zero swap. Native regression passed all 122 active cases (five unchanged opt-in ignores) and
strict Clippy; its scope peaked at 596,852,736 bytes with zero swap. That regression followed
the completed Decision 0206 sampling timeout; the prior measurement remains pinned to its
original binary. PROGRESS.md records exact commands and limits. No speedup is claimed yet.
