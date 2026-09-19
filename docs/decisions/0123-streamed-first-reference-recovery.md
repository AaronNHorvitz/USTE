# Decision 0123 — Preserve first references during streamed metadata recovery

Date: 2026-09-19

Status: accepted implementation contract; T-20 and qualification remain open.

Add explicit `recover_with_first_reference_streaming_domain` beside the inventory-free and
primary-only recovery entry points. It preserves an independently admitted first-reference
projection alongside each private primary metadata advancement. A populated base without that
evidence refuses before filesystem I/O; an owner-free base may bootstrap the family. Attached
quota projections still refuse rather than being discarded. Existing entry points keep their
closed contracts, including refusing an attached projection they do not maintain.

Only newly encountered primary owner IDs receive first-reference insertions, at the current
authenticated revision. Exact repeated references preserve the earlier principal and revision.
Each step merges the previous private witness run with current-inventory deltas, checks exact
insertion/output cardinality against primary owners and produces another generation-zero root.
No cumulative reference-history map or extra journal scan is introduced. Revision, owner,
inventory-reference and per-family merge limits are the same explicit bounds as primary recovery.
The private witness work contributes to the existing partial merge report; it is not complete I/O.

`stage_genesis_first_references` streams revision-one witness entries from a reconstructed
first inventory, with owner admission before I/O. Empty inventories return no witness root.
The private result must pass ordinary independent first-reference/primary journal admission;
it cannot be directly installed as a live or consumer-authorized index. This supplies the
matching witness for inventory-bearing origin recovery without publishing historical roots.

Terminal metadata remains private until the existing first-reference-preserving rebase succeeds.
Omitting preservation refuses. A completed streamed suffix has no owner overlay, so that rebase
requires no first-reference suffix scan and can use zero group/owner-overlay allowances. Normal
cold admission checks earliest matches against the authenticated journal; tests admit witnessed
metadata with one prefix pass rather than the legacy per-owner pass allowance.

Reference tests cross ordinary/origin recovery, initially empty/populated owner sets and zero/three
suffix steps. Literal earliest revisions survive later principals, absent inventories, terminal
publication and fresh independent cold admission. The three-step fixture produces 15 private runs,
33 output entries and 3,657 logical key/value bytes. Five new tests include missing-witness refusal,
genesis limits and every observed I/O boundary: 27 genesis staging failures; 678 suffix schedules
(675 failures and three optional no-crash boundaries); 552 terminal-publication schedules
(540 failures and twelve optional no-crash boundaries). Failed suffixes expose no intermediate
roots; interrupted terminal publication retains only old/current roots and origin recovery
restores the exact terminal ledger and earliest-reference values.

No frozen on-disk profile, M1 interface, benchmark ceiling or acceptance threshold changes.
Quota projection maintenance, immutable-family rewrite amplification, complete I/O accounting
and qualifying campaigns remain. This is bounded-memory implementation evidence, not a
larger-than-memory or production qualification claim.
