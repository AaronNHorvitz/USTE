# Decision 0075 — Exclusively owned bounded bootstrap replay

Date: 2026-09-18

Status: T-20 partial implementation; Linux disk runner integration remains open.

Recovering before bootstrap roots are published must not silently reopen a large journal into
complete coordinator and graph maps. Add `AuthenticatedIndexRecovery::into_bounded_coordinator`
for trusted callers supplying genesis reducer state and explicit outcome, owner and encoded-byte
budgets. It consumes the existing exclusive journal owner without a close/reopen ownership gap.
The journal range count is checked before reducer preparation. Each canonical group is reauthenticated,
owner admission precedes preparation/insertion, result digests must match, and existing retry,
transaction-collision and first-owner replay rules remain authoritative. Only terminal success
returns the coordinator; late failures discard provisional state and release ownership.

This is deliberately a small-prefix full replay, not large-history disk recovery. The caller's
reducer memory and storage's certificate/blob metadata are not bounded by these coordinator counts.
Encoded-byte limits cover certificates/groups; inventory sizes retain independent format limits.
No consumer authorization boundary or M1 interface is changed.

The disk development oracle now restarts immediately after policy certification, before any roots
exist, then consumes this handoff with one outcome, zero owners and 1 MiB encoded bytes. It publishes
bootstrap roots and proceeds through the existing disk-only fixture path. Linux resume must use
equally explicit bootstrap admission and otherwise the admitted disk-base/pending-suffix path;
this decision does not claim that Linux integration is already implemented.

Tests cover empty genesis with zero budgets, exact one-outcome/one-owner admission, refusal before
preparation for short count/owner/byte budgets, wrong reducer result, exclusive ownership before
and after handoff, durable exact retry/owner preservation, and refusal of an enlarged prefix.
Every observed read in a two-transaction handoff is faulted, including reads after provisional
reducer publication; no coordinator escapes. A post-open transaction-certificate mutation is
rejected. The unchanged 384-query oracle digest remains the integration acceptance check.
