# Decision 0008 — Governance, provenance and disclosure route

Date: 2026-09-16

Status: development-governance policy accepted. The private route remains unverified and is
tracked as distribution-readiness task T-62 under [Decision 0011](0011-development-and-distribution-readiness.md).

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
`AaronNHorvitz/USTE`, linked from `SECURITY.md`. Before an executable is distributed, an
authenticated repository owner or administrator must enable the feature and perform a harmless
end-to-end draft report. The test must cover the reporter's access and participation plus the
authorized repository administrators, security managers or explicitly added collaborators who
triage/respond; the earlier phrase "only maintainers can read/respond" was inaccurate. Record the
date and tester without fabricating an address or SLA. The observed `gh` credential was invalid,
and Git-over-SSH access is not repository-administration authority. Public issues remain limited
to non-sensitive concerns.

## Blocker and consequence

The external operation remains a genuine distribution blocker, not an unresolved technical
choice. Decision 0011 separates it from the locally reviewable part of D-07/T-06. T-06 may close
on the governance policy above; T-62 remains open until the route is actually enabled and tested.
No executable alpha, beta, candidate or release may be distributed while T-62 is open.
