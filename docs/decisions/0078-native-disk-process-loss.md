# Decision 0078 — Real process-loss recovery of the native disk fixture

Date: 2026-09-18

Status: T-20 partial implementation; supervised disk sampling and qualification remain open.

Add `linux-disk-create-crash-probe`, retaining the development ceiling and accepting only nonzero,
nonfinal prefix revisions. Reuse the existing bounded, content-free crash-probe readiness marker.
Revision one pauses after policy certification and before any bootstrap roots are published.
Subsequent pauses occur after a data transaction's graph publication and coordinator rebase.
The normal create/resume/open/query commands use a no-op observer; their semantics are unchanged.
The explicit probe parks after flushing its marker and is not an ordinary creator.

A real CLI integration test generates the oracle summary in a separate process, creates an
owner-only synthetic recovery-password fixture, then exercises prefixes one, two and three of
the unchanged 20/200, four-revision plan. The parent validates the bounded readiness marker,
SIGKILLs and reaps only its owned child, and verifies the signal exit status. Fresh CLI processes
must recover the exact prefix, reach revision four, pass completed-root open and all 384 oracle
queries, and repeat resume without adding a revision. Invalid pause points fail before filesystem
access. Test guards kill/reap owned children on failure; CLI output is drained concurrently with
explicit byte and time bounds rather than risking a full-pipe deadlock.

This adds process termination evidence, not controller power-loss evidence, physical erasure,
larger-than-memory qualification or production readiness. It does not claim interruption coverage
at every storage operation. Pending graph and partial metadata publication continue to have the
separate native closed-prefix and deterministic fault-matrix coverage. These commands/tests require
the accepted native Btrfs environment and unchanged memory safeguards. No M1 interface, benchmark
threshold or roadmap requirement is changed.
