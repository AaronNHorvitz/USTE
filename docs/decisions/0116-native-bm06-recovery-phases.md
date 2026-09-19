# Decision 0116 — Native BM-06 development recovery phases

Date: 2026-09-19

Status: accepted and locally verified development implementation. T-20 remains open.

Add separate x86_64 Linux/Btrfs processes for the Decision 0114 workload using OS entropy,
portable Argon2id recovery, current authorization and normal durable flushes. The native record
ceiling is two, checked before root/credential access; constructing exact-size limits does not
remove it. `bm06-linux-create` writes and verifies the 99-generation checkpoint/root prefix.
`bm06-linux-tail` first verifies that complete prefix, then certifies the final generation while
deliberately refusing only derived publication. `bm06-linux-recover` admits the certified suffix,
publishes its terminal roots, repairs metadata, retries the exact final request and verifies all
100 generations. `bm06-linux-open` requires already repaired terminal roots and verifies them
without initiating graph suffix repair. Ready-root resynchronization and optional storage catalog
recovery retain their existing behavior; “open” is not a promise of zero cache writes.

Each phase has an explicit accepted frontier; duplicate create, duplicate tail, premature terminal
open/recover and wrong record-count substitution fail. Create does not overwrite an existing
database. A partial creation/materialization is retained; arbitrary-prefix creation resume is
not implemented here. No larger-prefix full-memory fallback is introduced. The shared historical
oracle additionally verifies scoped IDs, creation revision, entity type/schema and active lifecycle.
Only policy bootstrap temporarily uses `GraphState`; all event history uses admitted disk state.

`bm06-linux-tail-crash-probe` flushes a fixed content-free marker only after the final certificate
has been acknowledged and derived publication has returned the expected resource refusal. It then
waits while holding ownership. Tests kill/reap only this owned child, prove a second owner is
refused while it lives, and recover/verify from a fresh process. This is process-loss evidence,
not controller power-loss or whole-directory rollback protection. Wrong credentials and changed
committed certificate bytes must fail closed, never select an older valid cache frontier.

JSON distinguishes initial admitted graph/metadata revisions, final frontier, the separately
verified historical frontier, logical verified bytes, open/repair/construction time, verification
time and total phase time. Adapter observations cover explicit storage calls, excluding credential
and root-descriptor setup, internal syscalls and handle drops. Verification has a separate delta.
These are not complete authenticated-index accounting or physical-device traffic. Host caches
are uncontrolled. Every report says nonqualifying development, zero qualifying trials and no
engine benchmark; a two-record single-tail test cannot satisfy the ten-million-event/196-tail-
revision/30-trial/120-second acceptance protocol.

Native one-record recovery found a fixture admission defect: shared merge bounds used only graph
family counts, while the 101-entry coordinator retry/transaction families exceeded its 100 graph
versions. Shared merge bounds now include the coordinator group count plus metadata headroom.
One-record and two-record tests exercise the fix; BM-01 numeric allowances remain unchanged.
This corrects admission rather than weakening the failing recovery check.

Native arbitrary-prefix resume, broader cache/rebuild/retained-base controls, scalable immutable
construction and the reserved-host campaign remain open. The source journal remains authoritative;
there is no authoritative baseline promotion, physical erasure or production qualification claim.
