# Decision 0212: Privileged vault encryption-work measurement

Date: 2026-09-20

Status: Accepted implementation; focused and full workspace verification passed.
Excluded from Decision 0211's binary.

Complement Decision 0175's decrypt boundary with a separate fixed-size per-vault encryption
report: completed successful/failed calls, encoded-envelope bytes represented by successful
results (header, padded ciphertext and tag), and exact unpadded successful input bytes.
Success does not imply the returned envelope was serialized, admitted or durably written.
Failed calls are not all attempted AEAD operations; failed byte work is explicitly excluded.

Keep encryption's original body/order in a private helper, then observe its unchanged result.
Do not change primitives, framing, padding, entropy, nonce reservation or error precedence.
Nonce counts remain separate because a failed encryption can reserve a nonce. Lock/unlock
retains diagnostics; a new vault starts fresh. No histories, identities, roles, keys, digests
or plaintext content are retained. Preserve the vault's synchronized-report/Sync properties.

All additions are checked, overflow is permanently invalid, poison refuses reports, and neither
diagnostic failure can alter a cryptographic result. The mutex is not held during encryption.
Keep decrypt report shape/semantics unchanged. Raw reports remain trusted maintenance capabilities;
do not expose database-wide lifetime work through a namespace consumer or own-outcome facade.
Journal/coordinator propagation must preserve poison/uncertainty guards and avoid snapshots/I/O.

Tests must cover exact small/blob sizes, unchanged golden ciphertext/errors/nonces, pre-admission,
entropy/duplicate/session failures, lock/unlock, all counter overflows, poison and noninterference.
This boundary alone is not complete authenticated-I/O accounting: key wrapping/unwrapping,
pre-vault decode refusals, failed-command lifetime aggregation, other owners and device I/O
remain distinct gaps until separately implemented/measured. Native wiring needs its own verified
increment; earlier binary reports and M1's pinned consumer handoff remain unchanged.

All 164 focused crypto/coordinator tests passed with one job/thread and a 3G/4G/512M process
group, peak 859,082,752 bytes/zero swap. This includes four new crypto tests plus strengthened
owner lifecycle/no-I/O/poison/uncertainty checks and unchanged ciphertext/retry/fault tests.
Full all-feature workspace verification subsequently passed 736 tests in 47 executables, strict
Clippy and warnings-denied docs. Peak process-group memory was 3,221,434,368 bytes/zero swap.
Exact commands and timings are in PROGRESS. Native wiring remains the separate Decision 0213
increment and must be verified before accepting its new report fields. No full-I/O or performance
qualification claim follows from the core boundary.
