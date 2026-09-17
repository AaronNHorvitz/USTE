# R0 cross-decision consistency review

Date: 2026-09-17 · scope: Decisions 0003–0011 and current normative specifications

This is the T-07 technical review and R0 closure evidence. Operational private-reporting
verification is separately open as T-62 and does not block local implementation.

| Concern | Owning decision | Cross-check | Result |
|---|---|---|---|
| identity/schema/time | 0003 | data model, time spec, storage replay, spatial observations | one commit order; normalized UTC values replay without reinterpretation; simulation ticks remain separate |
| durable publication | 0004 | transaction lifecycle, blob visibility, physics atomic groups | blob→group→certificate order covers every record class; complete-certificate corruption fails closed |
| encryption/deletion | 0005 | storage backups, content derivations, spatial/branch dependencies | authenticated context includes scope/role/epoch; purge ledger precedes restore exposure; current authorization governs history |
| parser boundary | 0006 | security threat model, format matrix, strict Rust profile | opaque storage is independent; exact candidates/features/fixtures recorded; workers have no commit authority |
| limits/workloads | 0007 | every Decision 0003–0010 cap and BM-01…13 | requests cannot raise caps; encryption/durability stay enabled; targets remain explicitly unmeasured |
| governance | 0008/0011 | CONTRIBUTING, SECURITY, task/release gates | development roles/policies and selected route are documented; T-62 honestly retains owner-administered operational verification before executable distribution |
| spatial/movement | 0009 | time/query/security/physics contracts | nanounit storage and named float algorithms are explicit; authorization precedes ranking; observed/estimated/simulated states differ |
| physics | 0010 | branch/replay, transaction atomicity, spatial frames | fixed-point profile is branch-local; whole tick is one transaction; UTC mapping is explicit rational metadata |

The review found BM-04 originally specified a 20 GiB stream while `limits-v1` capped a single
blob at 16 GiB. BM-04 was corrected to 12 GiB, retaining a multi-GiB streaming case within the
admitted profile. Stale pre-decision wording in the time, storage, security, content, spatial,
physics and verification specifications was aligned with Decisions 0003–0010; T-04 evidence now
distinguishes closed R0 feasibility from later worker admission. No other contract contradiction
was found. This closes T-07 but does not prove implementation correctness or replace later
independent review.
