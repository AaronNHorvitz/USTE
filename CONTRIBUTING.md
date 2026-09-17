# Contributing

The repository contains accepted R0 design evidence, a Rust workspace, canonical type/codec
kernel and development experiments, not a released or supported database executable. Reproduce
them using
[the Fedora Kinoite setup](docs/development-setup.md).

## Work sequence and completion

Read [Decision 0001](docs/decisions/0001-product-direction.md),
[Decision 0002](docs/decisions/0002-spatial-world-model.md),
[Decision 0011](docs/decisions/0011-development-and-distribution-readiness.md), [PRD](PRD.md),
[architecture](architecture.md), and the relevant domain specification before work.
Follow [TASKS.md](TASKS.md) dependencies. Resolve contradictory contracts before implementing.
Design experiments may inform R0; production-format implementation follows its decision gate.

Every change should identify requirements/tasks, tests or documentation checks performed,
results, unresolved limitations and compatibility implications. Check a task only when its
specified artifacts and acceptance evidence exist. Preserve user changes and unrelated work.
Do not push, publish, alter project permissions, or create external accounts without authority.
Local build and release-artifact preparation do not authorize executable distribution.

## Implementation principles

- Prefer small independently testable components and safe Rust.
- No copied upstream code without exact origin, version, license and notice records.
- Conceptual inspiration is not permission to reuse source or claim patent clearance.
- Preserve upstream copyright, license and required modification/NOTICE information.
- Review runtime, build-time, transitive, optional worker and model-weight dependencies.
- Record native-code/unsafe exceptions and assess whether they violate the selected profile.
- Do not invent cryptographic algorithms, silently change durability, or bypass authorization.
- Keep document fixtures synthetic or redistributable; do not import private customer files.
- Benchmark under normal encryption and durability; keep claims tied to measured versions.
- Add regression fixtures for security, recovery and parsing bugs.

## Licensing and provenance

First-party contributions are offered under the repository's MIT OR Apache-2.0 terms.
Contributions must carry a Developer Certificate of Origin sign-off from their actual
contributor. Do not invent identities or sign off on someone else's behalf.
Third-party files retain their own license terms and cannot simply be relicensed by inclusion.
Record a software bill of materials, source/version hashes and licenses before releases.
Independent legal review may be needed for actual commercial distribution.

## Documentation ownership

README explains the product; PRD owns requirements/releases; architecture owns component
boundaries; focused specifications own detailed semantics; TASKS records implementation
status. Historical documents are explicitly non-normative. New decisions preserve rationale
when changing contracts rather than rewriting history.

## Required automation

Task T-08 establishes formatting/lint/tests, documentation links, requirement/task references
and dependency/license checks. T-09 adds canonical-codec fuzzing; later work adds concurrency,
crash and benchmark jobs.
Release jobs must not imply independent security review when only automated checks ran.
See [SECURITY.md](SECURITY.md) for the current distribution-only disclosure-readiness gap.
