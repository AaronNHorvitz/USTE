# Decision 0186: Buffered primary coordinator admission

Date: 2026-09-20

Status: Accepted

Extend Decision 0143 with opt-in `admit_packed_coordinator_prefix_buffered`. Keep the uncached
entry point and all certificate, shape, canonical, journal-correspondence and first-owner checks.
Use the Decision 0178 fresh buffered canonical admission for each of the four families, dropping
each cache before the next. After canonical admission, create a separate fresh bounded cache for
retry, transaction-ID, blob-owner and first-revision correspondence lookups while streaming the
authenticated journal prefix. The caller supplies only a byte budget, never a warmed cache.

Share point-value decoding with existing uncached metadata reads. Cached reads still validate
namespace/target, exact certificate owner and unlocked-key session. Charge unchanged logical
page/encoded-byte proof work on hits as well as misses. Invalid cache budgets refuse before I/O.
Return the existing proof report plus fixed four-family/correspondence cache reports; these phase
residencies are sequential, not additive simultaneous memory. Drop all cached plaintext before
returning the independently admitted prefix. No query cache or consumer authority is supplied.

Extend the existing tests through both entry points: exact/minus-one budgets, authenticated false
metadata, wrong shape, late certificate corruption, historical-prefix continuation and every
observed read error/crash boundary followed by restart. Keep the uncached 798-case fault assertion;
derive buffered cases from its actual operation trace. Additional one-page/larger-cache checks
compare logical proof work, family commitments and actual read counts, require fresh repeat
behavior and bounded phase reports, reject invalid budgets before I/O, and mutate ciphertext
after a successful admission to ensure a later call reauthenticates it.

This is primary coordinator buffering only. Quota admission and harness integration remain
separate work. It does not replace authenticated journal correspondence with root trust, change
retry/collision/first-owner semantics, establish complete I/O accounting or qualify BM-01/BM-06.
The M1 consumer contract and accepted roadmap/release gates remain unchanged.
