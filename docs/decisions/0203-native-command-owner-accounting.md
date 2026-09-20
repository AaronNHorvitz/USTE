# Decision 0203: Native BM-01 command owner accounting

Date: 2026-09-20

Status: Accepted implementation; full standalone regression suite and strict lint passed.

Use Decision 0202's trusted owner diagnostics to account for each distinct vault lifetime in
successful native packed BM-01 create/open/resume/rebuild commands. Fixed slots identify initial
bootstrap, bootstrap-resume, construction, rebuild and terminal admission. Record immediately
before dropping an owner, or before returning the terminal session. A consuming recovery-to-live
handoff keeps the same vault and is not a second slot. Bootstrap resume must retain work from
its initial open and bounded ordinary handoff before it explicitly reopens.

Reject duplicate slots and checked-counter overflow atomically. Publish the exact per-owner
reports and their sum in a new `owner_vault_work` field; retain the existing last-owner report.
Do not change authentication, authorization, durability, nonces, admission limits, query timing,
on-disk formats, M1 handoff or benchmark targets. BM-06 accounting is separate follow-up work.

This sums completed decrypt calls only. It excludes key unwrap, encryption-byte work, failed
pre-vault decoding, device I/O and unsuccessful commands which return no report. It does not
make `complete_authenticated_io` true or qualify BM-01. Query/sample reports retain their
separate setup and measured-query scopes, with command accounting nested in their setup report.

Require exact checked sums, duplicate/overflow refusal, all completed command paths (including
empty and policy-only process-loss resume), unchanged terminal digests/source bytes and normal
wrong-key/corruption/authorization regression checks.

All 119 active standalone tests passed (five opt-in ignored), including native create/open/
rebuild/resume owner-set conservation and separate-process empty/policy/data-prefix recovery.
Strict standalone Clippy passed. Exact commands and per-suite results are in PROGRESS.
This is local implementation verification, not a qualifying benchmark measurement.
