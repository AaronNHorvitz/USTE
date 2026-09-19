# Decision 0093 — Native streamed suffix resume

Date: 2026-09-19

Status: verified partial T-20 implementation; qualifying campaigns remain open.

Connect Decision 0092 to the shared development disk-admission driver. Native resume no longer
requires the graph root to be at F or F-1: it may reconstruct a bounded missing graph suffix from
the selected authenticated base using private staged roots and terminal-only publication. The
profile's planned journal group count bounds the frontier and metadata overlay. The maximum graph
suffix is that count minus the policy bootstrap; one shared byte allowance is that suffix ceiling
times the maximum encrypted group plus its certificate. These are work ceilings, not reservations
or claims that such bytes are retained in memory. Per-step preparation/delta/merge bounds and the
64 MiB caller cache are unchanged. There is still no full-state fallback after policy bootstrap.

Open and query require graph and paired metadata roots at the frontier before invoking streaming
recovery; they cannot publish a missing graph root. Ready-root bounded authentication/resync is
allowed, with no slot rotation. Resume then uses the existing metadata rebase barriers before
authorized deterministic retries or new writes. Profile binding, cardinalities, retry identity,
retention, first-owner and collision semantics remain unchanged. Missing bases and corrupt selected
roots fail closed. Partial metadata roots at incompatible intermediate revisions still require
explicit rebuild; this increment does not erase them or bypass the pinned-pair barrier.

Reports preserve cold admission counters and add a separate terminal suffix-merge report. Neither
is complete authenticated-I/O accounting. Native 10,000-entity and memory-adapter 1,000-entity
development caps remain unchanged; qualifying-size admission is still refused. Storage's resident
certificate/blob maps, larger-than-memory evidence and exact BM-01/BM-06 remain open.

Native synthetic tests exercise two- and three-revision gaps from graph/metadata base one,
open refusal without terminal publication, resume/rebase, exact repeated resume, ready reopen
and all 384 queries against a separately built oracle. Existing missing-base, wrong binding,
cardinality, native SIGKILL-prefix and cache-pressure tests remain regressions. A native gap
close/reopen test is not a new SIGKILL-at-intermediate-stage or hardware power-loss qualification.
