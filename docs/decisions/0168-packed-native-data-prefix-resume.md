# Decision 0168 — Packed native data-prefix resume

Date: 2026-09-19

Status: implemented and locally verified; no benchmark qualification.

Extend the separate native packed fixture with an explicit `linux-packed-resume` phase. Before
derived output or fresh transactions, require an authenticated revision-two fixture marker and a
frontier within the unchanged profile's transaction count. Policy-only and empty legacy prefixes
remain unsupported: their bootstrap transaction does not bind fixture dimensions. Do not infer a
profile, silently retarget such a prefix, or change retry/expiry semantics to resume it.

Search revisions backwards from the authenticated frontier, bounded by the validated fixture
profile. Select the newest complete same-revision graph/primary/quota manifest triple. Discovery
retains only bounded candidates and uses existing bounded publication-attempt discovery. Missing or
invalid optional manifest attempts follow the storage discovery contract; I/O errors propagate.
Once selected, any canonical, semantic, journal-correspondence or referenced-page failure is fatal;
do not try an older triple after failed admission. Complete cache loss requires separate explicit
origin reconstruction, not implicit resume fallback. Ordinary terminal open remains strict.

Derive exact prefix family counts arithmetically from the frozen three-phase, 10,000-operation
batch plan: current records, all versions, accepted adjacency, created provenance/reverse references
and the single policy head/history. Validate the chosen triple independently, then use existing
authenticated streaming suffix recovery with zero resulting overlays. A base v1 digest is not the
recovered terminal digest: return no digest when a suffix ran, and cold-admit the finished fixture
to obtain its exact reference digest.

Resume streams the unchanged materialization plan, using exact retry for committed batches and
authorized writes with per-batch metadata rebase for fresh batches. Do not retain a request/outcome
map or use the full-memory graph reducer beyond the existing bounded policy bootstrap. Reports
identify selected base and replayed groups, and explicitly disclose policy-only resume limitations.

Validation includes arithmetic against every batch at boundary profiles (including fixture-only
100,000 entities), all four selected revisions of a real 20/200 fixture, native incomplete-prefix
continuation, an older complete triple with a newer unpaired manifest, complete cache loss,
wrong-profile refusal before writes, committed corruption and no-op terminal retries. This is
development correctness, not a qualifying-size materialization or actual process-loss campaign.
Bootstrap binding, owned-child interruption controls, packed BM-06 history, complete authenticated
I/O accounting and reserved-host benchmark qualification remain separate next work.

Full experiment regression passes 83 active tests with two pre-existing ignored campaigns;
strict Clippy and repository documentation/task-graph checks pass. Exact commands and resource
observations are in PROGRESS. Core source and its prior 658-test gate are unchanged.
