# Decision 0011 — Separate development and distribution readiness

Date: 2026-09-17

Status: accepted by the repository owner. Amends the gating consequence of
[Decision 0008](0008-governance-and-disclosure.md) without claiming that GitHub private
vulnerability reporting is enabled or tested.

## Context

Decision 0008 combined two different obligations in D-07/T-06: locally reviewable project
governance and an owner-administered external reporting-channel test. Because T-07 depended on
T-06 and T-08 depended on T-07, the external operation blocked every local implementation task.
That coupling was stricter than the security purpose of the channel and prevented development
that can be performed without distributing an executable.

The selected disclosure route remains GitHub private vulnerability reporting for this public
repository. GitHub documents that a submitted report is visible to and can be discussed by the
reporter and authorized repository security participants; it is not "maintainer-only."

## Decision

D-07 retains both obligations, but their readiness evidence is separated:

1. **Development governance (T-06).** Record contribution ownership, reviewer/release roles,
   dependency and provenance admission, signing/support policy, the selected disclosure route,
   and honest current limitations. This documentation-controlled work is required by T-07 and
   therefore before production-format implementation begins.
2. **Distribution readiness (T-62).** An authenticated owner or administrator must enable
   GitHub private vulnerability reporting and perform a harmless end-to-end report test. The
   evidence must record the date and tester; demonstrate submission and continued participation
   by the reporter; demonstrate receipt/response by authorized repository administrators,
   security managers or explicitly added collaborators as applicable; and confirm that the
   report was not exposed as an ordinary public issue. No address, response SLA or success may
   be fabricated.

T-62 is not an ancestor of T-07, T-08, or local R1–R3 implementation and acceptance. It is a
mandatory distribution gate: no executable alpha, beta, release candidate or release may be
distributed until T-62 is complete. R4 release decision T-44 depends directly on T-62. Release
pipeline, SBOM, signing and platform-trial preparation may continue independently, but cannot
publish an executable or claim distribution readiness.

Source code, tests and locally built executables may be developed and exercised by authorized
participants before T-62. They remain non-distributed development artifacts and carry no
production-readiness claim.

## Dependency proof and operational status

`scripts/check_task_graph.py` verifies that all task references exist, the graph is acyclic,
T-08 does not depend transitively on T-62, and T-44 does. This prevents the correction from
quietly introducing a cycle or release bypass.

Private vulnerability reporting remains unverified. The previously observed invalid GitHub CLI
credential is not retried by this decision, Git-over-SSH access is not treated as GitHub
administrative access, and no repository setting is changed. T-62 stays open until actual
owner-authorized evidence exists.

## Consequences

T-06 can close on the reviewed governance artifacts and T-07 can close when the technical R0
contracts and evidence are consistent. Local engine implementation can then proceed through its
ordinary security, durability and verification dependencies. Distribution remains blocked only
on the work that actually needs the external disclosure channel (and all other applicable release
requirements).

GitHub behavior references, checked 2026-09-17:

- <https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/report-privately>
- <https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/fix-reported-vulnerabilities/manage-vulnerability-reports>
