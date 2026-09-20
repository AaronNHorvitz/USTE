# Decision 0183: Packed generation-wide tail materialization

Date: 2026-09-19

Status: Accepted

Share a bounded packed BM-06 generation-tail constructor between model and native drivers. Check
the update-generation range, exact preceding-generation base and profile-derived frontier limit
before requesting a batch. Generate one existing at-most-512-record batch at a time, preserving
the model/native driver's established transaction/retry identities and normal authorization,
clock, encryption and durability behavior. Check each supplied batch's exact revision.

Intermediate batches use ordinary authorized publication and metadata rebase. Only the final
batch deliberately receives the existing insufficient derived-publication allowance. Accept
precisely the acknowledged terminal `CommittedPublication` storage ResourceLimit; authorization,
clock, transaction and other errors are failures, never successful crash probes. Return the exact
last durable outcome. Native recovery now exact-retries every batch in the final generation,
not just the last. Existing one/two-record behavior, frontier guards and caps remain unchanged.

The intermediate roots are real derived caches, not hidden or deleted. A bounded 513-record
model test materializes generation one, then both batches of generation two, leaving revision
five pending. Newest complete roots are at four, but explicit selection of checkpoint three
must replay two groups and return no stale base digest as a terminal digest. It verifies all
1,026 historical payloads, zero retry overlays, exact terminal outcome and the reference v1
digest after another cold open. Invalid generation/base/frontier and wrong batch sequence refuse.
The scripted clock supplies exactly two observations for two fresh transactions.

This does not establish that the native latest-root recovery command replays the whole qualifying
196-batch tail. Explicit native checkpoint selection, interrupted multi-batch-tail continuation,
safe larger construction, complete accounting and qualified trials remain separate work. Neither
the two-record native cap nor the 100-version/100,000-record workload or 120-second target changes.
