# Decision 0198: Fresh bounded graph proof preparation

Date: 2026-09-20

Status: Accepted and locally verified opt-in primitive.

Add an opt-in `prepare_packed_graph_transaction_buffered` trusted primitive beside the unchanged
uncached API. Allocate a fresh bounded packed-page cache for each complete preparation. Share
the existing current-record lookup, retained-reference closure, history/reverse proof scans and
pure reducer. Use the existing certificate-owner/key-session checked reader methods for cached
lookup and cursor steps. Never accept caller-warmed pages or retain plaintext across requests.

All logical lookup/page/byte/candidate and proof-retention limits remain unchanged, including
cache hits. Invalid cache sizes fail before I/O; no failure returns a partial prepared result.
Return fixed cache counters separately from logical work, not physical I/O or process RSS.
The API remains privileged proof preparation, not authorization, a commit or root publication.

Require full-reducer success/rejection equivalence, every exact/minus-one proof limit, one-page
eviction/larger-cache bounds, repeated-call fresh misses, current/history/reverse corruption,
foreign owner/scope refusal and every observed read fault/crash with cold reference recovery.
Keep the original uncached 270 fault cases. Domain/native selection and performance measurement
are separate work; no benchmark threshold, native admission cap or M1 contract changes.

Five new primitive tests passed, followed by all 710 workspace tests at `3acead5` plus this
increment. The full test executables exclude later Decision 0199 integration edits. Strict graph
Clippy passed; the later workspace Clippy invocation found test-helper qualification errors in
that separate integration work, recorded and repaired in PROGRESS. No test was weakened.
