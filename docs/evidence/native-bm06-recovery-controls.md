# Native BM-06 development recovery controls

Date: 2026-09-19. Tested tree: pushed `134f116` plus this increment.

The Decision 0116 native two-record fixture now exercises additional required recovery controls.
These are small correctness cases, not a qualifying recovery campaign or latency evidence.

- Capture the bounded encrypted root manifests at revision 100, then finish/recover revision 101.
  Corrupt only terminal-changed manifests, or move them into the test's private saved-root
  directory. A plain open refuses the stale graph base; explicit recovery admits the retained
  revision-100 graph/metadata pair, rebuilds only the certified suffix and verifies all 200
  historical versions. The complete certificate file remains byte-for-byte unchanged.
- Corrupt every optional root manifest. Open and recover refuse rather than reconstruct a hidden
  full-memory graph or silently roll history back. Restore the captured derived manifests and
  verify all versions again. Storage's independently rebuildable empty-blob catalog may rebuild
  before graph-base refusal; this is not claimed as complete graph index-loss rebuild support.
- Append nine bytes of incomplete certificate tail and nine bytes beyond the committed journal
  end. Recovery reports both exact amounts, preserves revision 101 and restores each complete
  file byte-for-byte to the saved committed prefix. The following open reports zero repair bytes.

Existing native cases remain active: owned-child SIGKILL after a flushed certified-tail marker,
live-owner exclusion, current credential checks, committed-certificate corruption refusal,
wrong-profile and phase-frontier guards, exact retry and second-process history verification.
Only synthetic files in newly created test-owned directories are changed. Missing manifests are
moved rather than deleted; fixtures are removed by their own test cleanup.

Command, serially under a user scope with MemoryHigh=3G, MemoryMax=4G and MemorySwapMax=512M:

```text
CARGO_BUILD_JOBS=1 cargo test --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline --test recovery_process -- --test-threads=1 --nocapture
CARGO_BUILD_JOBS=1 cargo clippy --manifest-path experiments/t20-bench/Cargo.toml --all-targets --locked --offline -- -D warnings
```

Scope `run-p412685-i21380063.scope` exited 0: six tests pass in 30.25s; strict Clippy passes in
1.00s. Sampled peak 381,009,920 bytes and zero swap are not a final whole-run maximum. Preflight
had 35 GiB available RAM and 3.9 GiB free swap. Host is the existing Fedora 44/kernel 7.1.10,
i9-13900KF/Btrfs zstd:1 development machine, without the qualifying CPU/RAM reservation.

No benchmark threshold changes. Complete graph cache-loss rebuild, arbitrary-prefix native
materialization resume, retained authoritative baseline controls, scalable construction and the
30 exact-size reserved-host recovery trials remain open. T-20/T-19 are not complete.
