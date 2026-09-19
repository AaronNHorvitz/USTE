# Decision 0135 — Bounded packed-tree range cursor

Date: 2026-09-19

Status: accepted and locally verified T-20 raw range cursor; domain integration open.

Extend Decisions 0132–0134 with a raw streaming cursor over an independently admitted canonical
root. Bounds are inclusive lower and exclusive upper byte keys; an empty lower bound means the
beginning, and an absent upper bound means the end. Bounds themselves have the 4 KiB key ceiling.
The cursor is not an authorization capability, root admission or a historical-policy decision.
Consumer facades remain responsible for candidate filtering and their separate result budgets.

Seek through the Patricia path for the lower key, authenticate the terminal route proof, then
compare the reached leaf with that key. Compressed skipped prefixes matter: use their first
differing bit to identify the entire too-low or eligible subtree before advancing or descending
to its leftmost leaf. Do not scan the complete preceding keyspace to implement a lower bound.
Retain a depth-bounded path and move to successors by ascending past completed right children,
then descending left in the next right subtree. Check every selected node against its expected
parent summary, increasing branch positions and the terminal root proof. Each returned value
requires complete chunk and content-hash verification first.

Return at most one owned zeroizing key/value entry per successful step. The caller controls
retention; the cursor retains only bounded path, range and continuation metadata. Cumulative
caller ceilings cover candidates, returned logical bytes and authenticated pages/encoded bytes;
individual values retain the 16 MiB hard bound. Refuse resource exhaustion rather than reporting
false end-of-range. Every error permanently poisons the cursor; no continuation can conceal a
failed read or late hash mismatch. End-of-range is sticky and performs no more I/O.

The hard candidate ceiling is one billion plus two (including seek probe and upper-bound witness);
the hard returned-byte ceiling is one billion times the per-entry key/value maxima. The hard page
ceiling is Decision 0134's full traversal ceiling plus one maximum-depth seek path; its encoded-byte
ceiling multiplies by 20,545. These are admission maxima, not campaign reservations or facade query
budgets. The path/proof reservations are bounded by 36,864 branches, with fallible allocation,
three retained 4 KiB keys and one in-flight key/value. Conservative metadata reservation (path,
proof, four keys and 64 KiB fixed scratch) must fit 32 MiB; this excludes separately bounded
page/crypto/value buffers and is not allocator/RSS telemetry. Counters distinguish visited leaves,
returned entries/bytes and successfully authenticated pages/chunks; failed adapter I/O is not
silently represented as complete measurement. Reuse the existing exact lookup page/value reader.

The cursor is in-process only, bound to its exact supplied root/context, with no unauthenticated
serialized resume token. A prior structural validation receipt cannot authorize it or immunize
later reads from corruption. Exact-reference range tests must cover bounds falling inside skipped
prefixes, every prefix/key boundary, empty and singleton trees, bounded seek work, cumulative
limits, values crossing chunk boundaries and observed read/error/crash faults at multiple steps.
No M1/v1 changes, root publication, domain integration or benchmark qualification are implied.
