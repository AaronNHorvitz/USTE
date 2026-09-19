# Decision 0150 — Authorized packed metadata reads

Date: 2026-09-19

Status: accepted and locally verified T-20 restricted read facade; disk-domain integration open.

Expose the packed coordinator's exact own retry/transaction outcomes and current committed-byte
usage through a restricted read-only facade. Reuse Decision 0065's current durable-policy contract
and kernel-instance authentication. Every operation authorizes before clock, filesystem or
cardinality-sensitive admission. Principal identity comes only from the authenticated handle.
Missing, pending, uncertain or mismatched policy fails closed; historical content uses current
authority. Immutable borrows prevent policy changes during a call; construct a new facade after
durable policy publication. No owner lookup, raw state, staging, writes or maintenance escapes.

Privileged packed outcome reads consult the bounded overlay first and then the admitted base.
Transaction ownership is filtered before expiry, preserving foreign-versus-absent concealment.
Exact expiry remains inclusive. The facade exposes no caller-selected per-lookup ceilings or
I/O statistics: fixed limits cover the admitted profile's longest 48-byte key and 136-byte value.
Canonical key encoding has at most 433 path bits; 433 branches plus leaf and one value chunk fit
435 page reads. Select 433 branches, 435 pages, 435 × 20,545 encoded bytes and 136 value bytes.
The canonical admission and typed key/value contracts justify these limits, not a benchmark.
No constant-time claim is made.

Committed usage reads the paired quota head and requesting principal aggregate, then adds only
the bounded disjoint first-owner overlay. Validate all base bindings, exact head and owner ceiling
before returning totals; errors never become zero or silently fall back to a ledger scan. Charges
survive failed rebase and clear only with both installed terminal roots. `InspectQuota` permits
namespace totals plus the requesting principal's total, not another principal's breakdown.
Staged reservations and full quota enforcement are not represented by this read-only surface.

Tests must cover base/overlay/rebase/recovery agreement, first-owner/zero-byte accounting,
expiry and principal isolation, default/foreign/revoked denial before clock/I/O, exact durable
policy equality, pending/uncertain refusal, corrupted metadata and every observed read fault.
Existing v1/M1 interfaces remain unchanged. Authorized packed writes/reconciliation and disk
domain integration remain separate; T-20 and qualifying BM-01/BM-06 remain open.
