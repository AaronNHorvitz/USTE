# Decision 0106 — Sparse cached-page search

Date: 2026-09-19

Status: implemented and locally verified. Native before/after observation pending; T-20 remains open.

Add a fixed sparse fragment directory to Decision 0090's validated immutable page layout. The
16 KiB plaintext page, 80-byte header and minimum 17-byte fragment permit at most 959 fragments.
Keep one u16 byte offset every 32 fragments: 30 slots / 60 bytes, with no additional allocation
or persisted format change. The directory is built during complete structural validation, not
from an unvalidated header count. All fragment shapes/order and trailing bytes are still checked
before a layout can be memoized. Context binding remains mandatory on every cache hit.

For an exact lookup, binary-search sampled keys, then start one block before the first fence
greater than or equal to the requested key. Starting earlier preserves the first fragment of a
key spanning multiple blocks. Pages with at most 32 fragments retain the original linear path.
Value assembly still checks its starting offset, exact continuation offsets, total length and
result-byte allowance, including cross-page values. Prefix/predecessor selection is unchanged.
Charge sparse key probes plus enumerated fragments to the existing partial fragment-work counter;
page visits, cache touches and logical result-byte semantics do not change.

Cached parsed pages borrow their validated layout instead of copying the enlarged directory on
every binary-search page visit. Standalone validation owns its layout. Both forms share the same
read-only methods; no mutable plaintext access is exposed. Eviction/clear discard metadata with
the zeroizing page buffer. The directory must fit the existing conservative metadata allowance;
the inline-layout and cache-capacity tests remain mandatory. Logical cache accounting is not
claimed to be allocator/RSS measurement, and the process memory limit remains independent.

Reference tests compare sparse starting positions and selected fragments with a simple linear
scan, including repeated-key runs across multiple sampling blocks. Existing malformed-page and
all-byte mutation tests remain. Add encrypted dense-page point-read comparisons with missing keys,
empty values, a multi-page value after a dense prefix, one-page eviction and exact resource refusal.
Require native oracle equivalence and measured before/after work before drawing performance
conclusions. No cache budget, benchmark target, qualification cap or M1 interface is changed.

The complete storage, graph, coordinator/replay, transaction and native-driver regression gates
pass, including parser mutations, resource refusal, publication/recovery fault matrices and
native process-loss tests. Strict workspace/native Clippy passes. PROGRESS.md records exact
commands and results; the pre-change 10,000-entity query observation is archived separately.
