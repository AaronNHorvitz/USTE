# Decision 0042 — Linux/Btrfs BM-01 materialization runner

Date: 2026-09-17

Status: accepted as T-20 qualification-runner groundwork. T-20 and BM-01 remain open.

## Context

Decision 0041 bounded the exact fixture transaction schedule, but the executable path still used a
memory fault model, deterministic test entropy and a test key wrapper. A qualifying measurement
needs the production x86_64 Linux/Btrfs adapter, normal OS entropy, portable encrypted recovery and
restartable phases. It also must not expose operator paths, credentials or database internals in
reports.

## Decision

The isolated `uste-t20-bench` executable adds three Linux-only phases:

- `linux-create` creates without replacement, installs the durable benchmark policy, commits the
  deterministic bounded mapping and publishes its current encrypted graph index;
- `linux-resume` authenticates and replays the journal, retries policy and record transactions with
  stable identities, and publishes the current index only when no current root exists; and
- `linux-open` independently authenticates portable recovery and the complete journal, requires
  the expected frontier, constructs an authorized read view and admits exactly one current root.

All phases use `LinuxFileSystem`'s Btrfs profile, `OsEntropy`, `RecoveryEnvelope` and the fixed
Argon2id portable recovery adapter. The password is read from a no-follow, close-on-exec,
nonblocking descriptor and must be a current-effective-user-owned regular file with one link, no
group/other mode bits and 1–1024 exact bytes. Newlines are data; paths and credential metadata are
not reported. A real wall/monotonic clock capability supplies commit time.

Reports use the fixed `bm01-linux-run-v1` JSON schema and contain only aggregate profile,
frontier/recovery and elapsed-time fields. They set `engine_benchmark:false`, disclose that host
caches are uncontrolled and identify the full-memory graph boundary. Only the exact 100,000/
1,000,000 profile is labeled an environment-unverified qualification candidate; smaller profiles
are explicitly nonqualifying. The shared Evidence record stores a pinned profile-specific
`bm01-uste-graph-v1` digest, and open requires that authenticated marker before reporting the
caller-supplied counts. Resume rejects an authenticated frontier beyond the plan, requires every
retried transaction's actual revision and the final authorized read-view revision to match it.

## Consequences and limits

The deterministic batch identity is designed to make an interrupted materialization retryable
within the fixed 30-day retention interval without collecting the workload in memory. This
decision's original real-filesystem evidence covered a completed-frontier retry: a Btrfs 20/200
smoke demonstrated create, independent open, idempotent resume at the same revision and a second
open. Decision 0044 subsequently exercises real SIGKILL at every incomplete small-profile phase.
This remains development evidence, not BM-01 performance evidence.

Root publication/admission and authorized views still clone or validate complete graph state and
warm host caches. The runner therefore does not claim larger-than-memory behavior or a host-cold
query state. Decision 0043 subsequently adds separate oracle summaries and a one-pass correctness
query phase; repeated authorized cold/warm sampling is still required before an exact run can be a
BM-01 candidate. The accepted 24 GiB reservation is still required for that run.

## Verification

~~~text
cargo test --manifest-path experiments/t20-bench/Cargo.toml --locked --offline
cargo clippy --manifest-path experiments/t20-bench/Cargo.toml \
  --all-targets --locked --offline -- -D warnings
# 17 tests pass; strict clippy passes

cargo run --release --manifest-path experiments/t20-bench/Cargo.toml --locked --offline -- \
  linux-create --root ROOT --password-file PASSWORD --entities 20
uste-t20-bench linux-open --root ROOT --password-file PASSWORD --entities 20
uste-t20-bench linux-resume --root ROOT --password-file PASSWORD --entities 20
uste-t20-bench linux-open --root ROOT --password-file PASSWORD --entities 20
# each reports frontier=4 and current_roots=1; resume does not advance the frontier
~~~
