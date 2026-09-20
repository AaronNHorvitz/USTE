# Decision 0164 — Private packed graph genesis staging

Date: 2026-09-19

Status: implemented and locally verified; origin orchestration and T-20 qualification remain open.

Stage all eight packed graph families directly from the opaque, authenticated first-transaction
reconstruction. Borrow its bounded genesis snapshot; do not clone it, reconstruct later graph
history, require a v1 derived root, or treat a caller-supplied snapshot as certified input. Existing
genesis recovery owns canonical request/inventory/result-digest validation and encrypted-range
admission. This stage binds its receipt to the exact exclusive recovery owner before output.

Preflight aggregate entry/logical-byte counts and batch count over the immutable genesis state.
Reuse existing family encoders, metadata counts and packed copy-on-write staging in batches of
at most 512 deltas, with explicit per-batch input/path/node/pack limits and aggregate read/write
page ceilings. Keep at most one bounded batch and eight family handles. Empty families remain
explicit. The one first-transaction reducer is still resident and must fit its existing admitted
request bounds; this is not a claim that arbitrary genesis state is constant-sized.

Return a private typed packed base with exact owner, first revision/certificate, reducer profile,
current policy, ordered commitments and frozen v1 digest. Never publish a root manifest or append,
truncate or recertify journal data. Failure can leave unreachable immutable scratch packs, never
a discoverable partial root. Physical reclamation remains lifecycle work.

Verify reference equivalence and partition independence, policy-only/record-bearing/empty-family
genesis, exact/minus-one limits, owner binding, late corruption and every observed staging I/O
error/crash with restart. Explicit packed origin orchestration will separately stream all suffix
transactions, preserve coordinator retry/collision/first-owner semantics and publish only a
complete terminal graph/primary/quota triple. Neither genesis staging alone nor its bounded
fixtures qualify BM-01/BM-06 or complete T-20, T-19 or the full roadmap.

Four integration tests pass, including 219 observed staging error/crash cases with exact restart,
a multi-page first-transaction value, batches of 1/2/512, all five aggregate exact/minus-one limits,
foreign owner and reopened-handle refusal, late packed ciphertext corruption and independently
admitted cold publication. Sixteen graph library tests, workspace Clippy and graph docs also pass.
The existing 647-test full workspace baseline is `a32693d`; this additive API's targeted verification
does not claim a new full-workspace run or native qualification. See PROGRESS for commands.
