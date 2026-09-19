# Decision 0134 — Streaming packed-tree structural validation

Date: 2026-09-19

Status: accepted and locally verified T-20 structural/content validation; domain admission open.

Decisions 0132–0133 require an independently known canonical tree. Add a complete raw structural
validation pass before future domain/root admission: authenticate every reachable node and value
chunk, recompute each node's logical commitment, enforce increasing branch positions and exact
child summaries, and verify every canonical key partition. This validates structure and content,
not authorization, domain semantics, journal authority or durable publication. A caller-supplied
self-consistent root is not thereby an authoritative database root.

Traverse iteratively in key order with a bounded depth stack, one prior key and one plaintext
page. At each left-to-right subtree boundary, the first differing bit between the prior subtree's
last key and the next subtree's first key must equal the boundary branch position. Together with
strict byte-key order, increasing descendant branch positions and leaf route checks, this rejects
noncanonical partitions without retaining all keys or nodes. Check aggregate leaf counts and
key/value lengths against the claimed root. Stream value hashes with the unchanged Decision 0128
domain and length framing; never collect a complete value merely for validation.

Admission has separate hard and caller bounds on depth, node count, value bytes and authenticated
page/encoded-byte work. Enforce work ceilings before I/O and return a private-field validation
receipt only after the entire traversal succeeds. No callback, provisional receipt or partial
success escapes on late corruption, exhaustion or I/O failure. Receipts describe a completed
read, not protection against subsequent file mutation; later operations still authenticate reads.

Required tests compare empty/singleton/generated trees and large chunked values with the reference,
pin exact/minus-one budgets and context refusal, reject authenticated self-consistent noncanonical
trees, and inject every observed read/error/crash boundary. Independently journal-admitted root
manifests, range queries, Linux process verification, graph/coordinator integration and qualifying
campaigns remain separate T-20 work. Preserve M1 and the existing v1 formats unchanged.
