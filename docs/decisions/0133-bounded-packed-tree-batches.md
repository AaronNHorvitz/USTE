# Decision 0133 — Bounded reachable-node packed-tree batches

Date: 2026-09-19

Status: accepted and locally verified T-20 private mutation primitive; domain integration open.

Extend Decisions 0128–0132 with an opt-in raw copy-on-write batch, not a journal commit or root
publication API. The caller supplies an independently admitted canonical root and strictly
key-sorted, unique exact before/after index deltas. Preserve the existing IndexDelta contract.
Admit at most 512 deltas and 64 MiB aggregate key/before/after bytes, with separate caller ceilings
for changed-node metadata, path depth, authenticated page reads/bytes and pack writes. The write
scope/profile/family must match the base, and its creation revision must not precede the base
view. Equal revisions are permitted for private construction of an optional derived cache; this
does not establish new commit authority.

Verify each selected disk node against its parent commitment and terminal key-route proof. Use
Decision 0128's exact bounded value-length/hash compare-and-swap transition to obtain the expected
new logical root. Retain unchanged subtrees as immutable disk links. Dirty branches can be updated
in a caller-bounded arena; inserting splits at the canonical first differing bit, replacing retains
shape, and deletion collapses the unary parent. Root equality after each delta is checked against
the independent logical transition. Identical before/after bytes still require valid preconditions
but need not dirty the tree. Do not load complete existing values merely to compare their already
authenticated content commitments.

Complete all precondition/path validation before creating an output pack. Serialize only dirty
nodes reachable from the final root, in bounded iterative postorder; do not use recursion driven
by adversarial key depth or write every intermediate root. Write new value chunks in reverse
link order using Decision 0131's canonical partition, then leaves and branches. Reuse old physical
links without rewriting their pages. Return a staged root only after the new pack's file and
directory sync succeeds, or with no new pack when the result is empty/unchanged. Errors expose no
successful prefix root. Failed output may leave rebuildable unreferenced packs; do not delete
uncertain files or assume T-35 reclamation is already implemented.

The arena, temporary path/proof frames, postorder stack and location table all have explicit
hard and caller bounds. Report authenticated page work, admitted input bytes, allocated dirty
nodes, reachable serialized nodes/chunks and finished pack geometry separately. These are logical
operation counters, not complete allocator/RSS or physical-device measurements. The existing
key-vault nonce-session ceiling is unchanged; it is not silently reset to finish a large job.
Hard ceilings are 65,536 dirty nodes and 36,864 path branches. Conservative combined reservation
accounting for arena, path/proof, location table, traversal stack and fixed scratch must fit
64 MiB; this is not allocator/RSS telemetry. Read ceilings are 512 times 36,865 pages and the
corresponding encoded bytes; callers normally select far smaller work allowances. Pack limits
retain Decision 0130 bounds and are validated even when no output is ultimately necessary.
The reservation counter covers metadata and its fixed scratch allowance, not the separately
bounded crypto-v1 page/envelope buffers or the caller-owned admitted delta payloads.

Required tests compare generated multi-batch insert/replace/delete histories and roots with an
independent sorted reference, prove unchanged-subtree reuse and final-only serialization, exercise
same-value/empty results, exact/minus-one admission, late conflicts/corruption and every observed
input/output error/crash boundary while retaining the prior root. Linux process tests, bounded
prefix/range queries, independently admitted root manifests, graph/coordinator integration,
complete measurement and qualifying BM-01/BM-06 remain necessary. No M1 or v1 profile changes.
