# Decision 0059 — Bounded root-manifest discovery

Date: 2026-09-18

Status: accepted as a bounded T-20 recovery increment at implementation commit `8885da4`. T-20
remains open for disk-backed coordinator metadata, larger-than-memory end-to-end qualification and
BM-01/BM-06.

## Context

The journal-anchored graph recovery path already admitted a disk base through caller-limited,
complete authenticated run cursors. Before those limits applied, however, generic root loading
scrubbed every page of both possible roots with the format's absolute maxima and a default page
cache. Graph recovery then scanned the selected root again for semantic admission. This hidden
pre-scan made recovery work proportional to the entire index outside the domain's declared budget.

Root publication has a different requirement. It must not overwrite the sole usable fallback, so
its compatibility loader must still prove run usability before choosing a slot.

## Decision

Storage separates two operations without changing `index-v1`:

- fully scrubbed `load_index_roots` remains the compatibility and publication-safety path; and
- `load_index_root_manifests` authenticates only the two fixed root slots, validates their bounded
  manifests and referenced run-file shapes, orders generations, rejects an ambiguous equal-
  generation pair and filters against the authenticated journal certificate chain.

Manifest results are explicitly provisional. Graph candidate discovery uses this second path and
does not read or decrypt a run page. Reconstruction and `GraphDiskBase` admission already exhaust
every required family through `IndexRunCursor`, whose page, entry and logical-byte ceilings are
derived from caller-selected aggregate limits. Entries remain staged until each terminal run digest
and all graph invariants, derived-family correspondences and the canonical logical digest pass.

A corrupt referenced run may therefore appear as a provisional candidate, but it cannot become a
disk base or visible reducer state. Operational I/O errors remain distinct; integrity and profile
failures fail that candidate closed. The legacy fully scrubbed path performs its generation-
collision check after unusable roots are removed, preserving the existing good-fallback behavior.

## Consequences and limits

Cold graph discovery is now constant in root-slot count and manifest size before caller limits take
effect, and it avoids the former duplicate complete run scan. The run cursors are resumable within
the operation and retain only one bounded entry; the current graph admission call does not serialize
an in-progress semantic scan across process loss, so a killed admission restarts that candidate.

This decision does not change root publication's deliberate full fallback scrub, compact journal
metadata, move retry/transaction/blob-owner maps to disk or extend journal-anchored recovery beyond
one suffix. It makes no BM-01/BM-06, production or release claim.

## Verification

Subsequent T-20 extension: `publish_index_root_recovered_bounded` authenticates both fallback
slots under caller-selected per-run limits before choosing an overwrite slot. Budget and I/O
failures leave slots unchanged; corrupt runs may be excluded only after integrity failure.
Metadata rebase and graph terminal-root publication use this API. The compatibility API remains
unchanged. Tests cover page/entry/byte refusals and preservation of the sole good older fallback
when the newest run is corrupt. This bounds the scrub; it does not eliminate its read cost.

~~~text
cargo test -p uste-storage -p uste-txn -p uste-graph --all-targets --locked --offline -- --test-threads=1
# uste-storage: 77 passed; uste-txn: 28 passed; uste-graph: 43 passed

cargo clippy -p uste-storage -p uste-txn -p uste-graph --all-targets --locked --offline -- -D warnings
# passed

CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 bash scripts/check.sh
# passed; docs/task graph/vector/fault/model/rustdoc gates passed;
# scaled T-20: 31 passed, 2 exact-profile acceptance cases ignored
~~~
