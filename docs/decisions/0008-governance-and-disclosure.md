# Decision 0008 — Governance, provenance and disclosure route

Date: 2026-09-16

Status: blocked on repository-owner verification; selected policy is complete but the private
route has not been enabled/tested.

Addresses D-07 and T-06 without fabricating external evidence.

## Selected policy

The repository owner is release maintainer and security triage owner until a CODEOWNERS-like
record names additional maintainers. Releases require: protected reviewed changes, passing
required checks, exact Cargo.lock, `cargo deny` license/advisory/source review, generated SBOM,
fixture/evidence manifest, clean Fedora trial, annotated signed Git tag and SHA-256 artifacts.
No release is produced by the current implementation branch and no support SLA is promised.

Dependencies must have a compatible license, pinned registry/source checksum, documented
purpose/features, no unexpected native build, and reviewed unsafe/platform boundary. Git
dependencies and model weights require exact immutable revision/hash and separate provenance.
Contributions retain DCO requirements; automation never invents a sign-off or reviewer.

The selected private route is GitHub private vulnerability reporting for
`AaronNHorvitz/USTE`, linked from `SECURITY.md`. Before T-06 can close, an authenticated
repository owner must enable the feature and perform a harmless end-to-end draft report,
confirming only maintainers can read/respond and recording the date/reviewer in the R0 report.
The current `gh` credential is invalid, and changing repository security settings is outside
the implementation authority. Public issues remain limited to non-sensitive concerns.

## Blocker and consequence

This is a genuine external-authority blocker, not an unresolved technical choice. T-06 and
therefore the aggregate T-07 R0 gate remain open. Design experiments and all other R0 evidence
may continue, but the branch must not claim R0 accepted or distribute an executable alpha.
