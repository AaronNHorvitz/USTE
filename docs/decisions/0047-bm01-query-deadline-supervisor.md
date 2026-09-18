# Decision 0047 — BM-01 query-deadline supervisor

Date: 2026-09-17

Status: accepted as T-20 qualification-runner groundwork. T-20 and BM-01 remain open.

## Context

Decision 0046 post-checked the 30-second query budget but could not preempt a synchronous indexed
read that failed to return. Supervising each query in a fresh process would destroy the retained
USTE-cache half of the accepted pair. The deadline therefore needs an external parent while one
worker retains the opened database, authorization state and page cache for the entire campaign.

## Decision

The documented `linux-sample` command is now a parent supervisor. It starts the same release-built
executable in an internal worker mode with stdin reserved for a parent-lifetime pipe, stderr closed
and stdout reserved for a closed, content-free protocol. The worker flushes `query-start`
immediately before each production engine call and `query-finish` immediately after it returns,
before oracle verification. The parent waits
at most 30 seconds between those markers. Timeout, unexpected ordering, malformed output or a
worker failure causes the parent to kill and reap that exact child and return a fixed error code.
Protocol lines require UTF-8 plus newline termination and are capped at 1 MiB before allocation can
grow further; only fixed markers, fixed-format error codes and the content-free final JSON are
accepted. A final report must be followed by successful worker exit within five seconds.
The parent retains the only write end of a lifetime pipe. A worker watchdog exits immediately on
EOF, so parent interruption or process death cannot leave an unsupervised worker holding the
database lock. An owning parent guard attempts termination on every exit path; timeout is reported
as such only after kill/reap succeeds, otherwise cleanup failure is explicit.

The worker remains alive across warm-up and all measured rounds, so empty/retained USTE-cache
pairing is unchanged. Only the final content-free sampling report crosses the pipe. A report emitted
by the worker always reports enforcement false. The parent parses its JSON, verifies schema,
profile, warm-up count, sample count and timed-execution totals against the number of completed
marker pairs, then owns the sole false-to-true transition. The library's direct unsupervised
sampling entry point remains available for tests and explicitly reports false.

The existing monotonic post-return duration check remains defense in depth. Engine latency still
excludes marker I/O and oracle validation. The supervisor does not claim to control kernel,
filesystem, controller or device caches and does not remove the full-memory graph boundary.

## Evidence and limits

A focused test starts a disposable child, injects a query-start event without a finish event and
proves that the shortened test deadline kills and reaps that exact PID. Closed protocol parsing is
also tested. A release-built real-Btrfs 20/200 CLI smoke completed the supervised 96-query warm-up
and one 768-execution measured round in 1,758 ms; the parsed report set both deadline enforcement
and post-checking true and preserved 2,815 empty-cache versus zero retained-cache page reads.

This removes the query-deadline implementation gap, but it is not a qualifying BM-01 run. The exact
campaign still requires the accepted host reservation and environment evidence; graph recovery and
the live reducer remain full-memory. The contemporaneous host preflight remained below the 24-GiB
reservation.

## Verification

~~~text
cargo test --manifest-path experiments/t20-bench/Cargo.toml --locked --offline linux_runner --lib
# 16 passed; includes report/count validation, bounded protocol closure and real child timeout/kill/reap
cargo clippy --manifest-path experiments/t20-bench/Cargo.toml \
  --all-targets --locked --offline -- -D warnings
# passed
~~~
