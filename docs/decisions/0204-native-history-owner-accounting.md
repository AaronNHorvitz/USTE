# Decision 0204: Native BM-06 history owner accounting

Date: 2026-09-20

Status: Accepted implementation; command/prefix/process regressions and strict lint passed.

Extend Decision 0203's fixed, checked lifetime ledger to successful native packed BM-06 commands.
Add a history-validation slot encompassing that owner's cold open, admission, retained-history
verification and any subsequent retry/tail work. Capture the separate fresh digest-admission
owner before dropping it; retaining only its digest previously lost these diagnostics. Tail-only
commands deliberately have no terminal-admission owner or terminal digest. Record initial
bootstrap, bounded bootstrap resume, construction and origin rebuild before their respective
owners are dropped. Consuming handoffs remain one vault lifetime, not two.

BM-01's existing report values and scope stay unchanged. BM-06 uses a distinct command scope;
all present per-owner completed-decrypt counters sum exactly with duplicate/overflow refusal.
Map reporting failures to a content-free BM-06 error. These counters still exclude encryption
bytes, key unwrap, failed pre-vault decoding, physical-device I/O and unsuccessful commands.
No complete authenticated-I/O or qualifying performance claim follows. Existing nonce/session,
proof/admission, source authority, phase timing, development/qualifying scale and M1 contracts
remain intact. This does not retroactively change earlier measured evidence.

Require ordinary command matrix, source/digest invariance, checked sums and exact slot sets,
bounded complete-generation prefixes, empty/policy/data-prefix process-loss resume and original
corruption/fault controls. Extend the existing opt-in larger tests' report assertions without
claiming another large run until one is explicitly admitted and actually completed.

Initial integration assertions omitted the valid create-prefix/resume-prefix phase names; two
tests failed. Corrected that new mapping without changing runtime behavior or weakening exact
slot/sum checks. All eleven active history process tests subsequently passed, with original
source/digest assertions intact. The 90-test library, legacy process regressions and strict lint
also passed; PROGRESS distinguishes the split runs and concurrent Decision 0205 harness tests.
