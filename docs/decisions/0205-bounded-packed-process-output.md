# Decision 0205: Bounded packed BM-01 process output

Date: 2026-09-20

Status: Accepted test-harness implementation; all seven packed process tests and strict lint passed.

Before extending native BM-01 development scale, remove the process harness's wait-before-drain
ordering. Drain stdout and stderr concurrently, each with a 256 KiB plus one-byte sentinel
bound, while retaining the existing owned-child deadline, kill and reap behavior. Refuse either
overflow; preserve nonzero exit status and diagnostic bytes. The normal deadline remains 90
seconds. An internal explicit-deadline helper supports controlled tests, not a runtime or
benchmark threshold change. Follow the already verified packed-history harness pattern.

Verify empty output, both streams larger than pipe capacity, exact bounds, each overflow stream,
nonzero exit with both outputs, deadline cleanup and all existing packed native process cases.
Use only synthetic shell builtins for control children. This changes no database production code,
format, proof budget, admission scale, security policy, M1 result or qualifying benchmark target.
No scale qualification follows merely from fixing a test supervisor.

Verified the two new output/deadline tests alongside all five original native process tests in
35.22 seconds, followed by strict standalone Clippy. The same invocation verified corrected
Decision 0204 history tests; exact command, baseline distinction and limits are in PROGRESS.
