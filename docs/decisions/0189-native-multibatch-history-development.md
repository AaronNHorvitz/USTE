# Decision 0189: Native multi-batch history development

Date: 2026-09-20

Status: Accepted; bounded scale locally verified, not benchmark qualification.

Raise only the packed native BM-06 development admission ceiling from two to 513 records,
the smallest profile with two 512-record-bounded batches per generation. Preserve 100 versions,
4096-byte payloads, exact identities, authorization, encryption and durability. The model and
legacy native ceilings remain two; qualifying 100,000 records still refuse before filesystem
access. The existing `development_record_limit` reports admission policy, not measured capacity.

Add `bm06-packed-linux-tail-prefix-crash-probe --pause-after-revision R`, accepted only for a
revision strictly after the frozen checkpoint and at or before the terminal frontier. The
shared generation-tail helper observes each intermediate batch after graph publication and
before metadata rebase, and the final batch after its certified acknowledgement with deliberately
pending derived publication. Normal callers use a no-op observer. An observer error stops without
pretending that certification was undone. Existing construction/final-tail probes remain intact.

The new probe parks after a flushed content-free marker. Only an owning test supervisor may run
it; that supervisor kills/reaps its child, including on timeout/panic. Default CLI tests retain
their existing 60-second marker/90-second completion limits. An explicit, serial, opt-in scale
case allows 1800 seconds per phase, constructs 513 records through checkpoint 199, kills its
child at intermediate tail revision 200, and resumes through 201. Verify all 51,300 versions,
preserve certificate prefixes, replay both tail groups explicitly from 199 despite terminal
roots, compare terminal digests and prove repeated resume appends nothing. Retain the synthetic
database and content-free reports on success or failure. This optional scale test must actually
pass before the increased ceiling is recorded as verified; it is not skipped evidence.

Run with one build job/test thread/heavy workload, fresh RAM/swap/disk checks, the existing
3 GiB high/4 GiB maximum/512 MiB swap process-group bounds, and unchanged nonce/session limits.
Stop a failed unchanged workload rather than repeatedly retrying it; preserve its artifacts.
The scale case is development correctness/resource evidence, not physical power-loss proof,
recovery-only timing, complete I/O accounting or the 30-trial qualifying BM-06 campaign.
The reserved 16-CPU/24-GiB/200-GiB benchmark profile and all release gates remain unchanged.

The explicit scale run passes after correcting a test-only certificate-header length assertion.
Exact source/binary hashes, retained success/failure roots and phase reports are recorded in the
[development evidence](../evidence/native-packed-history-513-development.json); PROGRESS records
the full standalone regression command and result separately.
