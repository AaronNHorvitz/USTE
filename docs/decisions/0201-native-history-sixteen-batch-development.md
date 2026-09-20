# Decision 0201: Native sixteen-batch history development check

Date: 2026-09-20

Status: Accepted bounded experiment; ordinary tests and explicit native run passed.

Following Decision 0200's verified 4,096-record run, raise only packed native BM-06 experimental
admission to 8,192 records. Add a separate ignored test for 819,200 historical versions: checkpoint
1,585, terminal 1,601, sixteen 512-record tail batches. Kill the owned child after graph publication
at 1,586, resume the remaining tail, explicitly recover from 1,585, then repeat resume and cold
open. Preserve all literal historical payload/identity/version checks, exact source-certificate
prefixes and digests, plus old 513/4,096-record cases.

This is an admission experiment, not measured capacity until the complete run passes. Update
boundary refusal cases to 8,193 and retain 100,000-record pre-I/O refusal. Model/legacy two-record
caps and frozen qualifying profile remain unchanged. Keep the 1,048,576 nonce/session limit,
explicit cache bounds, 1,800-second per-child/5,400-second overall limits and 3G/4G/512M scope.
No auto-reopen, reset or writer rotation is added. Resource/nonce exhaustion must be preserved as
a failure, not bypassed. Check current host headroom and run no competing heavy workload.

Require ordinary regression tests before the explicit case. Retain the synthetic store on success
or failure and archive exact source/binary/lock provenance, all five phase reports and post-workload
scope statistics. Neither the 3.36 GB logical payload nor a small-cap run satisfies the reference
runner's 24 GiB reservation, 10-million-event/30-trial profile or larger-than-memory qualification.

The fresh run passed all five phases in 1,343.09 seconds, with 819,200 terminal versions,
sixteen replayed suffix groups, equal terminal digests and unchanged source certificate bytes
after repeated resume/open. The ordinary suite passed 118 active tests (five opt-in ignored).
[Exact evidence](../evidence/native-packed-history-8192-development.json) records the tested
binary/sources, full phase reports and process-group peaks. This closes this development
experiment only, not T-20, BM-06 or full-project qualification.
