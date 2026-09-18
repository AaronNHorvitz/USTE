# Decision 0058 — M1 local memory pilot qualification and handoff

Date: 2026-09-18

Status: accepted M1 qualification for exact implementation commit
`b9689f37d7e728d8ad7e27d50def3541e8609132`. This closes T-68 and M1 only. It does not close R1,
R2, T-19/T-20, production/security review, physical erasure, authoritative migration or the T-62
distribution gate.

## Decision

Accept the bounded `memory-pilot-v1` derived-index experiment against every M1-A through M1-J exit
case in `docs/evidence/memory-pilot-acceptance.md`. The source handoff is pinned by full Git commit,
Cargo lock digest, Rust toolchain, target, filesystem profile and feature set. The generic consumer
checklist is `docs/memory-pilot-handoff.md`.

The accepted measurement profile is the one frozen before implementation in Decision 0055. The
release binary used a 1 MiB opaque source, normal Argon2id recovery wrapping, encryption, policy,
durable blob/journal publication, 100 warmups, 1,000 measured authorized identity reads and a fresh
open. It achieved 55,512,414 bytes/s ingestion, 355 ms cold recovery, p50/p95/p99 below the one-
microsecond reporting resolution, and 265,180 KiB peak RSS. These pass the 1 MiB/s floor, 15-second
cold cap, 100 ms p99 cap and 512 MiB RSS cap. They are local pilot measurements, not BM-01/BM-02/
BM-04/BM-06 qualification or a cross-host performance promise.

## Accepted failure evidence

A real adapter child durably acknowledged revision 4, printed the content-free ready marker and was
killed by its parent. A fresh process recovered the exact authorized source citation. Separate runs
proved wrong-password and authenticated committed-certificate mutation fail closed. The real Linux
ownership conflict, query cancellation and budget exhaustion are pilot-specific tests. The existing
storage/coordinator fault matrices supply deterministic no-space/publication failure evidence for
the same journal/blob path; no host filesystem was intentionally filled.

## Boundaries and next order

Consumers may begin only a separately reviewed shadow integration using synthetic/non-sensitive
data, their existing authoritative source/policy store and their own durable outbox. They must not
copy the demo password, expose raw blob/coordinator handles, claim IPC security, treat USTE as the
sole copy, or interpret logical index disposal as physical erasure.

Decision 0054 requires returning now to T-20 and then T-19 before the remaining roadmap. The large
BM-01/BM-06 campaign was not run during M1 because host swap remained saturated and the milestone
did not weaken those exact targets. Spatial, geographic, navigation, motion, physics, rich-content,
retention/purge, backup/restore and lifecycle requirements remain open in their original order.
