# Decision 0130 — Bounded immutable index packs

Date: 2026-09-19

Status: accepted and locally verified T-20 storage primitive; tree/domain integration open.

Connect Decision 0129 framing to handle-relative storage without publishing a root. An immutable
pack contains consecutive fixed-size encrypted pages in one create-new file named with a fresh
random opaque 128-bit object ID. The writer retains one bounded page, aggregate counters and one
file handle, not a collection of previously written records or pages. Page/record/payload budgets
are caller supplied and bounded before file creation. Appending a record that cannot fit the
current page seals and writes that page before beginning the next. A returned address is
provisional until the pack finishes and does not confer authorization or commit authority.

Finish seals the last nonempty page, sets the exact file length, syncs the file and then syncs
its directory before returning a descriptor. Any append error poisons the writer; it cannot
return a successful prefix descriptor. Failed creation, writes or finish may leave unreferenced
objects, never a new discoverable root. Do not remove files on uncertain failure. T-35 retains
orphan reclamation ownership. Fresh create-new names prevent overwriting an existing pack, and
entropy errors, zero identity and collisions fail without fallback or identity reuse.

Each record address binds the complete packed-page context and slot. Bounded reads first check
the address against the descriptor and admit one encoded page's byte budget, then open the exact
opaque name, check exact file length and authenticate the selected page. They return a zeroizing
framing owner and a one-page byte report. They do not enumerate packs, decode tree payloads,
scrub unrelated pages, infer canonical state or silently cache previously read bytes. Future
root/node admission must supply trusted descriptors and verify logical child commitments before
making records visible. Operational failures propagate unchanged; callers must distinguish an
optional cache from authoritative storage at their own boundary.

Required verification: multi-page restart round trips, exact/minus-one admission before I/O,
sticky refusal, all observed create/write/length/sync/read failure and crash boundaries, short
I/O, authentication/context/length corruption, fresh-name collision and unchanged old packs.
The deterministic filesystem model proves only its stated flush ordering. Actual Linux process
tests, typed node/chunk links, copy-on-write publication, graph/coordinator integration, cache
accounting and larger-than-memory campaigns remain subsequent work. No v1 or M1 path is changed.
