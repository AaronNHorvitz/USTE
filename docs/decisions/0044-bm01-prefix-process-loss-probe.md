# Decision 0044 — BM-01 durable-prefix process-loss probe

Date: 2026-09-17

Status: accepted as T-20 recovery-runner groundwork. T-20 and BM-01/BM-06 remain open.

## Context

The Linux materializer's stable transaction identities made a durable prefix theoretically
resumable, but Decision 0042 evidenced only a retry after the final frontier. Existing journal
fault matrices prove individual publication boundaries; the benchmark owner still needed a real
process-loss protocol that leaves an incomplete multi-transaction fixture and resumes it through
portable recovery in a fresh process.

## Decision

The Linux-only `linux-create-crash-probe` command accepts a revision strictly between zero and the
profile's planned final revision. It creates the database normally and observes each materializer
commit only after the coordinator returns its exact durable revision. At the selected frontier it
writes and explicitly flushes one content-free `bm01-linux-crash-probe-v1` marker, then parks
indefinitely while retaining the exclusive database owner. An external harness must SIGKILL that
exact child and require its wait status to show signal loss before invoking `linux-resume` in a new
process. The probe never reports benchmark success and cannot select the final frontier.

The ordinary materializer and resume path share the same postcommit observer seam. Production
policy, entity/evidence, relationship-create and relationship-accept transactions retain their
existing identities and payloads. Resume replays the certified prefix, retries those identities,
commits only the missing suffix, publishes exactly one current root and performs the existing
authenticated frontier/read-view/profile checks.

## Consequences and limits

On the reference Btrfs filesystem, release-built 20-entity/200-relationship children were killed
after revisions 1, 2 and 3—the policy, entity/evidence and relationship-create frontiers. Each
fresh resume reached revision 4 with one root and no repaired certificate tail or ignored journal
bytes; an independent open then admitted revision 4. This is real SIGKILL/prefix-recovery evidence,
not a simulated clean return.

The probe tests loss after an acknowledged complete transaction, not every byte within journal or
certificate publication; the existing deterministic and Btrfs journal process-loss suites own
those boundaries. The small profile does not prove exact-scale duration, bounded-memory recovery,
BM-01 latency or BM-06's 10-million-event recovery budget. The command is test/qualification
machinery, not a consumer database API.

## Verification

~~~text
cargo test --manifest-path experiments/t20-bench/Cargo.toml --locked --offline
# 21 passed, 1 exact-profile release test ignored in the debug-profile suite

# For each REVISION in 1, 2, 3 with a fresh Btrfs ROOT:
uste-t20-bench linux-create-crash-probe --root ROOT --password-file PASSWORD \
  --pause-after-revision REVISION --entities 20
# wait for bm01-linux-crash-probe-v1, SIGKILL the exact child, require signal-loss status
uste-t20-bench linux-resume --root ROOT --password-file PASSWORD --entities 20
uste-t20-bench linux-open --root ROOT --password-file PASSWORD --entities 20
# each resume/open reports frontier=4 and current_roots=1
~~~
