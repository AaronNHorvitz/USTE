# Decision 0191: Writer nonce headroom diagnostics

Date: 2026-09-20

Status: Accepted; privileged diagnostics, not writer rotation or benchmark qualification.

Expose `VaultNonceReport` with the exact distinct issued nonce count, session limit and
remaining count for one owning vault. Read the existing bounded registry; introduce no second
registry, allocation, reset or encryption-path behavior change. A nonce reserved before a
later encryption failure remains counted. Duplicate/entropy failures and pre-admission refusals
reserve nothing. Decryption and lock/unlock do not change the report. The registry's limit
remains Decision 0013's 1,048,576; the count is not successful encryptions, allocator bytes,
whole-process work, key-adapter wrapping work or a forecast of how many transactions fit.

Forward this trusted report through the journal owner and packed coordinator. Poisoned journal
owners refuse with NeedsRecovery and uncertain coordinators with OutcomeUnknown. These APIs
do not perform consumer authorization: a trusted adapter must authorize before exposing
cardinality-sensitive diagnostics. No nonce values or key material are returned.

Native history construction must sample its actual construction owner before dropping it,
not its later cold-read owner. Label the exact scope and omissions. A repeated construction
target may still perform derived-index work; do not assume exact transaction retry means zero
nonce consumption. Headroom observations alone never admit a larger fixture or permit an
automatic vault restart after exhaustion. Durable writer-incarnation rotation, complete I/O
accounting and reserved-host qualification remain separate requirements.

The bounded 513-record run observes 37,127 issued construction-owner nonces at checkpoint 199;
interrupted-tail resume uses 370 and repeated terminal resume zero. Exact source/binary hashes,
resource limits, retained fixture and raw reports are in the
[development evidence](../evidence/native-packed-nonce-headroom-development.json). This does not
establish a per-event bound or permit extrapolating the qualifying workload's session demand.
