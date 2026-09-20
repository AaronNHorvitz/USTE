# Decision 0171 — Native packed BM-06 terminal pipeline

Date: 2026-09-19

Status: implemented and locally verified; no benchmark qualification.

Connect the verified packed history engine to separate Linux/Btrfs native phases in
`bm06-linux-packed-engine`: create the checkpoint, open a complete checkpoint/terminal triple,
certify the final generation with deliberately refused derived publication, recover that tail, or
explicitly rebuild from journal origin. Keep the native development cap at two records; reject
larger profiles before filesystem/credential access. Preserve existing BM-01 and v1 BM-06 stores.

Native policy bootstrap IDs are `BM06PK1\0` plus the big-endian record count; data-batch IDs are
`BM06PD1\0` plus the big-endian sequence. Authenticate the first journal transaction's exact policy,
profile-bound transaction ID and absent inventory before derived reconstruction or a fresh tail.
Use existing credential descriptor checks, OS entropy, real clock and disk certificate/blob recovery.
Only policy bootstrap uses the ordinary graph reducer. Share packed limits, writes, cold admission,
bounded complete-triple selection and the exact historical payload oracle.

Keep phase frontiers explicit: create ends at checkpoint 100; tail acknowledges frontier 101 but
reports history verification only through 100, a pending derived terminal and no terminal digest.
Open requires a complete same-frontier triple. Recover selects the newest complete triple, fails
closed on selected-page/semantic corruption, streams the authenticated suffix and exact-retries the
certified tail. Repeated recovery of an admitted terminal is a no-op. Complete cache loss requires
explicit origin rebuild; there is no silent fallback. Rebuild works at either complete frontier and
does not change authoritative bytes. Cold admission supplies each reported full v1 digest.

Reports disclose zero qualifying trials, incomplete authenticated I/O and uncontrolled host caches.
The actual native fixtures contain two records/200 versions, not ten million events. This increment
does not resume incomplete construction prefixes or provide packed BM-06 process-loss controls;
those are the next implementation work, followed by complete I/O accounting and qualified campaigns.

Verification covers checkpoint/tail/recovery separation, all historical payloads, exact retry and
no-op root preservation, wrong key/profile, committed-certificate corruption, complete cache loss
at both frontiers, selected-pack corruption, explicit origin repair and separate-process CLI phases.
Production fault/reference coverage remains in the previously verified packed graph/transaction
suites; these native tests do not claim physical power-loss or larger-than-memory qualification.

All five native history cases and two separate-process CLI cases pass. Full experiment regression
passes 96 active tests with two pre-existing ignored campaigns; strict Clippy and repository
documentation/task checks pass. Exact invocation, baseline and resource observations are in PROGRESS.
