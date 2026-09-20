# Decision 0195: Buffered staging through scoped maintenance

Date: 2026-09-20

Status: Accepted and locally verified maintenance bridge.

Expose Decision 0194's fresh per-call packed staging cache through
`PackedIndexMaintenance::stage_buffered`. Preserve the ordinary uncached `stage` method.
Check the optional base's namespace and target ordering before delegating to the journal's
certificate-owner/key/profile/family checks. No raw journal, vault, arbitrary target receipt,
caller-warmed cache or publication authority is exposed.

Return the ordinary staged capability and its separate bounded cache counters. Proof-work
budgets and exact transitions are unchanged. A later call starts fresh; no hidden shared cache
or session reset is introduced. This is privileged derived-index maintenance, not consumer
authorization. Graph/coordinator call-site integration and native measurements remain separate.

Verify uncached/buffered root and work equality, multiple transitions, actual hits/misses,
repeat-call fresh counters and pre-I/O future/foreign/budget refusal. Existing ordinary
maintenance retry semantics and storage fault/corruption tests remain required.
