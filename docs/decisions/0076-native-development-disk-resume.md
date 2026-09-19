# Decision 0076 — Native development disk construction and resume

Date: 2026-09-18

Status: T-20 partial implementation; disk query sampling and qualification remain open.

Add separate `linux-disk-create`, `linux-disk-resume` and `linux-disk-open` commands. Retain
the legacy commands and their explicit full-memory disclosure. The new path uses the native
Linux/Btrfs adapter, portable Argon2id recovery, existing owner-only password-file admission,
OS entropy and system clock. It shares the unchanged development fixture mapping/batch generator
and authorized disk commit/rebase path with the memory-adapter oracle check. Its database name
is distinct from the legacy runner, preventing accidental format/profile substitution.
The shared commit helper takes explicit typed batch identities: native commands retain `BM01LIN`
while the memory driver retains `BM01DEV`; canonical fixture operations and ordinals are unchanged.

Only a zero/one-revision bootstrap may reconstruct `GraphState`, with Decision 0075's exclusive
handoff, one outcome, zero owners and 1 MiB encoded budget. Its exact policy installation retry
must match before roots are built. Larger prefixes never fall back to complete graph/coordinator
replay if roots are missing. Root discovery selects the latest graph base at the frontier or one
revision before it, then the latest paired coordinator roots no later than the graph base.
Complete bounded semantic/authenticated admission remains mandatory; a corrupt selected run
is refused rather than silently skipped. One pending graph transaction is prepared against the
admitted disk base and reauthenticated by coordinator suffix recovery. Trusted resume repairs
the pending graph root and rebases coordinator metadata before fresh writes. Partial coordinator
root publication reuses the existing recovery/rebase barriers. Open requires already-repaired
roots and empty overlays; it does not publish missing derived roots.

The fixed profile Evidence record is checked through the authorized reader before resumed data
writes and again at the final expected frontier. Exact retries retain stable identities and the
existing retention/collision semantics; expired retries are not silently refreshed. Recovery open
may still perform the storage layer's documented tail repair. Derived repair is not source migration.

The new commands intentionally retain the development ceiling of 1,000 entities and existing
development proof/merge/admission budgets. This does not lower the accepted 100,000/1,000,000
BM-01 target or its reservation/windows/deadline requirements. Reports say `engine_benchmark:false`,
disclose resident storage metadata and make no query-latency, larger-than-memory, or release claim.
The disk sampler and qualifying-profile admission must be implemented before attempting that
campaign. M1 and all spatial/lifecycle/security requirements remain unchanged.

Native tests cover empty/policy bootstrap, pending graph publication, lagging and partially
published coordinator roots, repeated exact resume, wrong fixture binding, missing derived roots
on a larger prefix, and a graph suffix beyond the one-revision contract. The unchanged independent
memory-adapter oracle remains the full query-equivalence check for this increment; these native
close/reopen tests are not represented as new SIGKILL or hardware durability qualification.
