# Decision 0181: Buffered native development measurement

Date: 2026-09-19

Status: Measured development evidence; no qualification.

Remeasure the retained Decision 0177 fixture using pushed `edc0845` and release binary SHA-256
`0a5b2fa572647ac0c1bb2ad8ada7814daf3a8a05d85383878c0c4d28f36c6579`.
Archive complete reports in [buffered native evidence](../evidence/native-packed-buffered-1000-development.json).
Preserve the previous binary/version attribution and all original artifact files. No reconstruction
or fixture mutation is used to obtain the comparison; certificate bytes remain unchanged.

Cold query setup takes 2,389 ms versus 51,261 ms before buffering. Last-owner successful decrypts
fall from 2,378,825 to 6,381 and authenticated encoded bytes from 48,895,663,753 to 153,801,773.
These count repeated envelopes, not physical device traffic. Standalone open takes 2.53 s wall.
The 384-query command takes 72.63 s wall versus 121.23 s; its query-only time is 70,228 ms versus
69,953 ms. Paired sampling takes 155.83 s wall versus 203.67 s; sample execution alone is
135,278 ms versus 134,026 ms. Thus the observed improvement is setup, not query throughput.

The v1 digest, query digest, paired digest, all 384 typed query outcomes, 96 warm-ups, 768 timed
executions and cache/decrypt work are unchanged. Query evictions remain zero. Peak process RSS
is 265,272/265,380/265,888 KiB for open/query/sample, all zero process swaps. Scopes retain
3G high/4G maximum/512M swap limits; no accepted-host reservation or controlled host cache is
claimed. PROGRESS records exact invocations, resource preflight and observed scope peak.

This supports adopting fresh bounded graph admission, not larger-than-memory qualification,
complete I/O accounting or completion of T-20/T-19. Continue coordinator/accounting work and
multi-batch BM-06 prefix/tail construction prerequisites without changing targets or native caps.
