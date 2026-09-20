# Decision 0172 — Packed BM-06 prefix and process recovery

Date: 2026-09-19

Status: implemented and locally verified; no benchmark qualification.

Extend the separate native packed BM-06 runner with explicit incomplete-prefix resume and
owned-child construction/tail interruption controls. Keep the two-record cap, frozen fixture,
existing phase semantics, credentials, authorization, encryption and durability unchanged.

Resume may reconstruct an authenticated zero/one-revision bootstrap with at most one outcome,
zero blob owners and 1,048,576 replay bytes. Empty stores have no certified profile yet. For a
policy-only store, require the sole receipt's exact principal, profile-bound retry key, transaction
ID and revision; commit the same canonical policy only as an exact retry. Wrong identity/request
or expired retry is a refusal, never a fresh replacement policy. Larger prefixes cannot use this
ordinary reducer path.

For data-bearing prefixes, authenticate the fixture binding, select the newest complete packed
triple, independently admit it and stream any certified suffix. Verify every existing historical
payload before fresh appends. Stream original batches with exact retries and per-batch metadata
rebase: incomplete construction ends at checkpoint 100; an already certified frontier 101 remains
101. Do not implicitly append the final generation during construction resume. No full request,
outcome or history map is introduced.

Complete cache loss still refuses resume. Explicit rebuild now accepts every authenticated
profile-bound prefix from policy-only through terminal, reconstructs only that actual frontier,
and does not append events. Report zero verified record versions at policy-only, otherwise the
exact existing prefix. A separate subsequent resume may finish construction. Retain the first
opener's repair counts and report initial frontier, bounded-bootstrap use and resume base/groups.

The construction probe parks after durable creation (0), policy acknowledgement (1), or successful
graph publication before metadata rebase (data revisions up to checkpoint). The tail probe parks
after certifying the final generation with deliberately refused derived publication. Emit only a
content-free flushed marker. Tests kill/reap only children they own; timeout/panic cleanup also
owns those children. These are real Linux SIGKILL controls, not physical power-loss qualification.

Test construction boundaries 0/1/2/50/99/100, wrong certified dimensions, incomplete certificate
tails, exact source-prefix preservation, no duplicate terminal commits, policy-only reconstruction,
partial-prefix all-cache-loss refusal/rebuild/resume, final-tail SIGKILL/recovery, future-pause
pre-I/O refusal and foreign bootstrap principal/key/request refusal before root publication.
The prior storage/graph fault suites remain required finer-boundary coverage. Native packed
sampling, complete authenticated I/O, resource-safe scale work and reserved-host campaigns remain
open; this does not qualify BM-06's ten-million-event workload or its 120-second target.

Verification passes all 77 active experiment library tests with two pre-existing ignored campaigns,
five packed history CLI cases (eight actual SIGKILL cases), strict Clippy and documentation/task
checks. Exact commands, resource limits and prior unchanged-suite baselines are recorded in PROGRESS.
