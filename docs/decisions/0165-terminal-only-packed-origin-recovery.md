# Decision 0165 — Terminal-only packed origin recovery

Date: 2026-09-19

Status: implemented and locally verified; T-20 qualification remains open.

Add an explicit trusted origin-recovery entry point that needs no derived graph, primary or quota
root. Consume one exclusive authenticated recovery owner; obtain its actual already-authenticated
frontier, not a caller-selected rollback target. Reconstruct only the bounded first transaction
with the existing canonical request/inventory/result checks. Stage private packed graph genesis
and exact primary/quota prefixes, then release the resident first-transaction reducer.
The existing graph reducer is inventory-free: inventory-bearing genesis or suffix requests must
reject, never silently discard inventory or pretend to support composite ingest. Generic packed
coordinator first-owner/usage behavior remains unchanged; this graph entry point does not replace it.

Reuse the existing paired packed suffix implementation through private candidate advancement and
terminal-publication helpers. Every remaining transaction must pass authenticated streaming,
graph result validation, retry/transaction-ID collision checks and first-owner/quota staging.
Retain one request/plan and fixed tree handles rather than history-wide maps. Separate genesis
range/entry/byte limits from the suffix's aggregate range/count and per-revision limits. Reports
distinguish first-transaction staging from suffix work; they are not complete adapter I/O accounting.

Publish only the actual terminal graph/primary/quota triple after complete cursor exhaustion and
matching terminal anchors. Individually durable manifests are not an atomic three-file switch.
Any error returns no live coordinator; retained valid roots remain available, and partially
published terminal roots are not treated as a complete paired state. Explicit origin rebuild may
run with absent, corrupt or retained derived roots because none establish its authority. Never
append, truncate, recertify, silently fall back during ordinary open or roll back committed history.

Tests must cover root-free first-only/multi-revision reconstruction, reference state/outcomes and
exact retries, all three terminal roots with no intermediate publication, continued writes, cold
reopen, retained/corrupt caches, limit refusals and every observed error/crash with restart. Preserve
all existing suffix reference/corruption/fault tests while extracting shared code. Authenticated
false results/collisions and committed-source corruption must return no live state. Native fixture
integration, measured scalability and reserved-host BM-01/BM-06 campaigns remain later work.

Implemented by `recover_packed_graph_origin` with separate genesis/suffix limits and reports.
Seven integration tests include 909 observed error/crash cases with restart, exact genesis/suffix
retries with zero overlays, root-free and retained/corrupt-root recovery, full-reducer equality,
cold admission, continued writes, inventory refusal and authenticated invalid-history controls.
The shared suffix regressions remain unchanged. Full workspace gate: 658 tests, Clippy and docs
pass; final restart-fixture checks reran four genesis and seven origin tests. Exact commands,
resource observations and the corrected test-fixture failure are recorded in PROGRESS.md.
