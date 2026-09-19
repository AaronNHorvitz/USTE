# Decision 0119 — Explicit native BM-06 origin rebuild

Date: 2026-09-19

Status: accepted bounded development implementation; T-20 remains open.

Connect Decision 0118 to `bm06-linux-rebuild`, a separate explicit recovery phase under the
unchanged two-record native ceiling. It requires the complete terminal fixture frontier (101),
reauthenticates/reduces only the inventory-free first transaction under a 1 MiB encrypted-range
allowance, stages private graph/coordinator candidates and applies normal independent admission.
It then streams all 100 later revisions into private graph roots, publishes the exact terminal
graph root, rebases metadata, retries the exact final request and verifies every historical payload
through current authorization. OS entropy, portable credentials and normal durable flushes remain.
Ordinary open/recover still refuse complete graph-base loss; there is no silent fallback.

Reports identify origin reconstruction, the private graph suffix length/output logical bytes,
the admitted metadata overlay count and its explicit ceiling. The origin development case retains
100 outcome overlays against a 101-outcome admission ceiling. This is a disk base plus bounded
suffix overlays, not scalable incremental origin metadata staging or larger-than-memory evidence.
Uncontrolled host caches, partial adapter I/O accounting and zero qualifying trials remain visible.

Explicit reconstruction also works when older valid metadata root slots survive. Ordinary rebase
still refuses roots ahead of its pinned base but behind the terminal frontier: rotating those
slots during a different writer's partial rebase could lose its recovery pair. The narrow origin
exception requires an independently admitted private revision-one metadata base and the complete
authenticated/validated suffix. That reconstruction depends on no discoverable historical pair;
the existing terminal exact-content reuse or normal bounded publication remains mandatory. There
is no exception for an ordinary published revision-one base. A dedicated core test exercises both
sides against retained revision-one/three metadata roots at frontier four and checks unchanged
authority and exact terminal logical digest.

Native tests first rebuild with intact old/current caches, then corrupt every optional root or
move every manifest to a private fixture directory. Open/recover refuse; explicit rebuild verifies
all 200 historical versions, preserves the entire certificate file and committed journal segment
byte-for-byte, and a fresh ordinary open requires zero graph suffix replay. Committed certificate
corruption still rejects rebuild, and premature rebuild at the checkpoint frontier refuses.
Only synthetic test-owned directories are modified. Missing manifests are moved, not deleted;
their test fixture cleanup is unchanged.

No authoritative baseline promotion, physical erasure, AgentMage integration or release claim.
The full source journal must still be retained. General retained-baseline recovery belongs to
T-35; origin staging with bounded incremental coordinator metadata, construction scaling and
the exact-size reserved-host BM-01/BM-06 campaigns remain required.
