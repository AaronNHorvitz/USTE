# Decision 0222: Allocation-free positive-cache key probes

Date: 2026-09-20

Status: Implemented and locally verified; native performance observation pending.

D0221 shows that comparison ordering alone does not close the positive-cache latency gap. The
existing cache still allocates and copies a complete logical-key/175-byte-identity vector before
every hit or miss. Replace that flat retained-key map with an exact two-level private structure:
select the complete retained identity first, then borrow the caller's logical key directly for the
inner lookup. This removes transient cache-key allocation from probes. Returning a successful value
still makes the existing fallible zeroizing owned copy required by the public lookup contract.

Retain one shared zeroizing identity per active identity bucket and one shared zeroizing logical key
per resident entry. The stamp map keeps exact global LRU across identity buckets. Remove an empty
identity bucket immediately on eviction. Equality remains exact over all identity and logical-key
bytes; no digest, probabilistic match or trusted admission shortcut is introduced. The outer lookup
may compare identities, while logical keys within the overwhelmingly common same-root bucket retain
their direct ordering. This supersedes D0220's temporary flat byte representation, not its security
or accounting requirements.

Preserve the conservative per-entry charge as if all 175 identity bytes were retained for every
entry, even though a bucket shares them. Preserve budgets, reported resident/accounted bytes,
counters, oversized bypass, work-limit rechecks, authorization/session clearing and all on-disk or
public formats. Require the 10,000-step independent variable-byte LRU, every identity-byte
substitution, prefix/maximum logical keys, shared-identity ownership, exact counter/accounting and
all existing fault/corruption/authorization tests before measurement. No performance claim follows
until a separate same-protocol native observation completes; default and qualification gates remain.

Verification passes 761 workspace tests, strict workspace Clippy, warnings-denied documentation,
and 136 active native release tests with five unchanged opt-in ignores plus strict native Clippy.
The enclosing scope peaked at 5,370,458,112 bytes, briefly crossing its 5 GiB soft watermark and
throttling without swap, maximum-limit, or OOM events. This shared scope peak is not process RSS.
