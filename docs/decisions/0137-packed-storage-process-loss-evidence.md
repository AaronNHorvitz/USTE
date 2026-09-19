# Decision 0137 — Packed storage process-loss verification

Date: 2026-09-19

Status: accepted and locally verified on Linux/Btrfs; other-platform and release qualification open.

Exercise Decisions 0129–0136 against the supported Linux adapter in isolated synthetic test
directories. Use a fresh writer incarnation for each writing process, restore a test-only wrapped
key, and kill only the child process created by the test. Synchronize readiness over a pipe with
a finite parent timeout and an RAII child guard; no unbounded polling or unrelated-process control.

Cover SIGKILL after the first output-pack write but before its sync, after complete pack sync but
before any new manifest, after a partial test manifest, and after complete manifest/file/directory
sync. Reopen from a new Linux adapter/vault. The old immutable tree must remain exactly readable
at every boundary. No missing/partial manifest may be accepted as a new root; a completely synced
test manifest must recover the exact new tree while sharing unchanged old-pack links.

Manifest names, certificate/state claims and key wrapping in this test are synthetic harness
inputs, not the future production discovery, authorization or journal-admission protocol. The
test checks encrypted pack/manifest durability and fail-closed reads, not power loss, physical
erasure, authoritative migration or a qualifying benchmark. Record the actual filesystem/profile
tested; do not infer ext4 or other-platform qualification from a Btrfs result.
