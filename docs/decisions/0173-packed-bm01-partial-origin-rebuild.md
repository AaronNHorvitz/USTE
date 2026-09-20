# Decision 0173 — Packed BM-01 partial-origin rebuild

Date: 2026-09-19

Status: implemented and locally verified; no benchmark qualification.

Extend explicit native packed BM-01 rebuild to the actual authenticated fixture prefix, rather
than requiring complete materialization. This closes the retained incomplete-prefix/all-cache-loss
case: ordinary resume still refuses absent paired bases, explicit rebuild restores only the
existing prefix, and a subsequent resume finishes the frozen batch plan. No automatic fallback,
authoritative append, rollback, migration or fixture retargeting is introduced.

For data-bearing prefixes, authenticate the unchanged revision-two evidence marker before output.
For policy-only prefixes, authenticate the first canonical policy request, absent inventory and
Decision 0169 profile-bound transaction identity via a fully finished bounded journal cursor.
This is read-only binding, not a retry, and introduces no dependency on receipt expiry or a fresh
clock. Empty stores and legacy unbound policy-only prefixes remain refused. Ordinary resume retains
its stricter exact retry/principal/key/expiry checks before continuing from a policy-only prefix.

Reconstruct with the existing zero-overlay packed origin path, check replayed groups against the
actual frontier and cold-admit the exact prefix family counts. At policy-only there is no evidence
record yet; its independently authenticated policy marker is the binding. Reports include the
actual frontier and `complete_fixture`, and the digest belongs to that exact reconstructed prefix.
Open/query continue to require complete materialization. Existing complete-store behavior, caps,
oracle and all benchmark targets are unchanged.

Tests remove every packed manifest at data frontiers two and three, require ordinary resume and
wrong-profile reconstruction to refuse, prove repeated explicit reconstruction preserves source
bytes/frontier/digest, then resume to the same terminal reference. Real owned-child policy-only
interruption also exercises explicit reconstruction without appending a transaction; empty and
legacy unbound policy prefixes refuse. These are development correctness cases, not qualification.

Verification passes all 20 packed library cases plus four packed BM-01 CLI cases, strict Clippy
and documentation/task checks. PROGRESS records exact commands, resource limits and prior baselines
for unchanged suites. Core source and its 658-test gate remain unchanged.
