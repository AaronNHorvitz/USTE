# Decision 0095 — Map-free certificate recovery and proof-bound roots

Date: 2026-09-19

Status: locally verified T-20 increment; blob metadata and qualification remain open.

Add an opt-in journal recovery mode that omits certificate-anchor insertion in both mandatory
validation/replay passes and in subsequent appends. The existing contiguous hash chain and exact
terminal digest comparison bind both passes; no group callbacks or repairs precede complete first-
pass authentication. Initial certificate count/bytes are admitted against explicit limits before
the scans or repairs. The byte ceiling applies separately to each mandatory pass and to later
individual certificate proofs. Blob/inventory/namespace collections remain resident and retain
their existing admission bounds. Default recovery and persisted formats are unchanged.

Recovered root handles may carry a transient Decision 0094 certificate proof. Existing lookup,
predecessor, scan, cursor, scrub, resync and merge paths validate that proof without a historical
anchor map. Bound handles cannot silently fall back to resident anchors after owner mismatch.
Unbound legacy handles retain their existing map-based behavior. Root equality describes all
persisted content, not live-owner evidence; equality alone is never admission or authorization.
No proof is encoded into a root manifest, and deserialization never invents a live-owner binding.

Explicit proven manifest discovery reads at most two fixed slots, applies certificate-proof
limits per candidate and reports aggregate proof work. Future candidates are excluded. A proof
failure returns no candidate collection, rather than silently treating authoritative-certificate
corruption or resource/I/O refusal as a missing cache. Normal root discovery selects this path in
disk-certificate mode. Run/domain semantic admission remains mandatory. Publication and validated
scratch completion attach a fixed owner-bound receipt without extra I/O; historical base merges
use the prior root's proof. Scratch admission gains an explicit-I/O variant. Optional checkpoint
candidate omission behavior remains unchanged, with on-disk anchor checks in this mode.

Committed-range reads re-prove each selected certificate to the authenticated frontier before
exposing its group. All proof certificate re-reads debit the same range byte allowance, in addition
to the original selected certificate and group reads. This simple implementation can do quadratic
certificate work across a complete range; it is not called constant-time or free cold recovery.
Proof construction has constant retained certificate state. A later optimization must preserve
exact chain/owner binding and explicit shared work accounting.

The authenticated recovery owner exposes this mode to the native and memory-adapter disk fixture
drivers. Their profile-derived range allowances add the exact worst-case triangular certificate
proof term to the existing maximum group/certificate allowance. Per-proof limits cover the planned
prefix. These are implementation-work limits, not increased benchmark latency/memory allowances.
Native reports expose actual full-history residency and resident certificate-entry count; the
aggregate storage-metadata-resident flag remains true because blob collections are not removed.
The 10,000/1,000 development caps and qualifying-profile refusal remain intact.

Passing regressions include no-anchor-map ordinary reads/merges, owner mismatch before I/O,
discovery corruption/faults, exact/minus-one range bytes, append/publication with zero retained
anchors, admission/corruption before callbacks or repairs, native multi-revision resume and the
existing independent oracle/process-loss matrix. Optional checkpoint discovery/streaming also
works without anchors, omits candidates on proof refusal, and emits no payload when the explicit
stream encounters proof-budget refusal or later certificate corruption. The added graph recovery
matrix exercises 231 I/O boundaries and 693 attempts (687 actual failures; six optional missing-file
operations have no successful crash-after boundary). The resident-map matrix remains unchanged.
See PROGRESS.md for exact commands and results. Passing these does not close T-20, BM-01/BM-06,
larger-than-memory or release qualification. Next remove remaining storage blob metadata and
complete the documented qualification prerequisites without weakening accepted targets.
