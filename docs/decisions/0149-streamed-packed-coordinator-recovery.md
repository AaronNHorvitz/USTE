# Decision 0149 — Streamed packed coordinator recovery

Date: 2026-09-19

Status: implemented and locally verified T-20 streamed packed recovery; qualification open.

Consume an exclusive authenticated recovery owner with independently admitted historical packed
primary/quota prefixes and a ready ordinary reducer at that exact base. Validate published family
receipts, live-owner bindings, quota pairing and domain claims before replay. Admit exact suffix
group and encoded-byte limits before reducer callbacks; reuse the bounded certificate-window
cursor and copy-on-write primary/quota construction. Do not reconstruct complete coordinator maps
or retain post-base outcome/owner maps merely to advance recovery.

Each transaction is fully authenticated and canonically decoded before preparation. Compare the
reducer's prepared result digest against the certified outcome before publishing only to private
recovery state. A trusted recovery-state hook installs that exact prepared value and binds its
ready-state anchor to the opaque authenticated transaction receipt; ordinary `publish` alone has
no certificate argument and must not be treated as that binding. The hook cannot expose provisional
state or append authoritative transactions. Stage retry/transaction/first-owner metadata and accounting at the exact successive
certificate; duplicate retry/transaction IDs or changed first-owner references must fail closed.
Late read, budget, corruption, preparation or staging failure returns no live coordinator and
publishes no intermediate roots. Previous complete roots remain optional fallback caches.

After exact suffix exhaustion require terminal ready-domain claims with the same reducer and
state-commitment profile as the admitted base. Publish only the terminal primary/quota pair and
install through the live admission checks, starting with empty bounded overlays. A publication
failure can leave an authenticated primary-only cache, never a partially installed coordinator.
The caller may retry from a complete older pair or independently rebuild derived quota data.
Current bases need no synthetic transaction or root rotation. No ownership-release/reopen race
or fallback to the fully memory-resident coordinator is permitted.

This entry point serves ordinary reducers; disk-domain pending-state and authorized consumer
integration remain explicit follow-on work. A caller-supplied reducer's own residency is not
bounded merely by eliminating coordinator maps; qualification still requires a disk-backed domain.
Tests require reference equivalence, zero retained
suffix maps, exact budgets, duplicate/corrupt/false-result rejection, terminal-only visibility,
fault/restart coverage and successful exact retry after cold recovery. It does not qualify T-20,
alter graph-state-v1/M1 contracts, claim complete I/O accounting or replace required benchmarks.

Verification: six new recovery tests passed, covering 702 injected fault/restart cases, zero
overlay ceilings, 1/2/64 certificate windows, exact range limits, current-base no-op, cold paired
admission/charges/retries, authenticated collision/false-result suffixes, late reducer and anchor
failures, and certificate/derived-family corruption. The full gate passed 566 tests across 47
executables plus warnings-denied Clippy/docs. PROGRESS.md records exact commands and resource
observations; no partial live coordinator or intermediate root is accepted as successful recovery.
