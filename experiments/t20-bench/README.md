# T-20 benchmark fixture foundation

This standalone experiment pins `bm01-materialization-v1`. It prepares deterministic synthetic
fixture semantics and an independent adjacency-array BFS oracle. Its bounded `engine-check`
command also validates the mapping against production encrypted, authorized, durable graph/index
code after a simulated restart. The fixture checks do not provide BM-01 acceptance evidence;
the separate Linux samplers collect diagnostic latency while still withholding qualification.

The qualifying-size profile always uses the accepted BM-01 seed and exactly:

- 100,000 typed entity IDs and 1,000,000 typed relationship IDs;
- 800,000 uniformly shaped directed relationships with self-loops excluded;
- 100,000 relationships distributed across 100 hubs, alternating outward/inward direction and
  excluding self-loops;
- 100,000 clockwise relationships forming one complete entity ring;
- 32 measured and 8 disjoint warm-up roots for each topology class and each depth 1 through 4.

Typed IDs combine a seed-derived, type-separated prefix with the big-endian ordinal. They are
deterministic and collision-free within the admitted ordinal range. Endpoint shaping consumes the
byte-compatible `synthetic-v1` graph identity stream. The manifest records separate synthetic
entity/relationship stream, materialization, topology, measured-query, and warm-up-query digests.
It emits no individual record, endpoint, or query-root values.

## Commands

From this directory:

```text
cargo run --release --locked --offline -- manifest
cargo run --release --locked --offline -- manifest --entities 1000
cargo run --release --locked --offline -- oracle-summary --entities 20 > ORACLE
cargo run --release --locked --offline -- oracle-bundle --entities 20 > ORACLE_BUNDLE
cargo run --release --locked --offline -- engine-check --entities 20
cargo run --release --locked --offline -- disk-engine-check --entities 20
cargo run --release --locked --offline -- linux-disk-create --root ROOT \
  --password-file PASSWORD --entities 20
cargo run --release --locked --offline -- linux-disk-resume --root ROOT \
  --password-file PASSWORD --entities 20
cargo run --release --locked --offline -- linux-disk-open --root ROOT \
  --password-file PASSWORD --entities 20
cargo run --release --locked --offline -- linux-disk-query --root ROOT \
  --password-file PASSWORD --oracle-file ORACLE --entities 20
cargo run --release --locked --offline -- linux-disk-sample --root ROOT \
  --password-file PASSWORD --oracle-file ORACLE_BUNDLE --entities 20
cargo run --release --locked --offline -- linux-query --root ROOT \
  --password-file PASSWORD --oracle-file ORACLE --entities 20
cargo run --release --locked --offline -- linux-sample --root ROOT \
  --password-file PASSWORD --oracle-file ORACLE_BUNDLE --entities 20
cargo test --locked --offline
cargo clippy --all-targets --locked --offline -- -D warnings
```

The default is the exact qualifying fixture size. `--entities N` derives exactly `10*N`
relationships while preserving the 80/10/10 split and a full `N`-entity ring. Any value below
100,000 is labeled `nonqualifying-small-scale`; it is only for development and tests.

`engine-check` is capped at 1,000 entities and always emits `engine_benchmark: false`. It maps typed
fixture IDs to scoped graph IDs, adds one shared source Evidence record, creates relationships and
then accepts them in a separate durable revision. After encrypted index publication it restarts the
durable memory adapter, replays the journal, loads the persisted authorized root and compares all
384 measured query shapes with the independent oracle. The 20-entity/200-relationship golden output
digest is `46f1bdb3138d6325e4c0f56b5fd3bbf5ff092d816e8a0f6c23acd15687b910b5`.

`disk-engine-check` preserves that same cap, fixture, 384-query corpus and golden digest. Only a
policy-only bootstrap uses `GraphState`; all fixture writes and the final cold-admitted reads use
the disk-backed graph/coordinator capabilities. It selects the accepted 64 MiB USTE cache for
each writer batch and for the later reader (not simultaneous writer/reader caches), and
reports maintenance-authorized cache counters. It still uses the memory fault-model adapter and
development keys, holds the independent oracle in process and retains storage-level certificate/
blob metadata in memory. It is semantic evidence, not performance or bounded total-RSS evidence.
The existing Linux commands are not silently switched by this development command.

`linux-disk-create/resume/open` use a distinct native Btrfs database name and portable recovery
credentials, with a separate 10,000-entity native development ceiling (Decision 0087). Both
memory-adapter engine checks remain capped at 1,000. Native over-limit profiles are rejected
before filesystem/child-process access, and reports include `development_entity_limit`.
They use profile-derived
admission/preparation/merge work limits shared with the memory-adapter verifier.
The limits validate through the exact-size 212-revision plan, including 30,001 preparation proofs,
but constructor validity does not qualify execution or remove the native development ceiling.
Create streams the fixture through
authorized disk writes; resume admits paired metadata roots and a graph base, streams its bounded
authenticated suffix privately, publishes only the terminal graph root, then repairs metadata and
retries the unchanged deterministic plan. The suffix count is bounded by the fixture's planned
revisions minus its policy bootstrap; one shared certificate/group byte allowance covers metadata
replay, with explicit per-revision proof/merge limits. Only an empty
or policy-only bootstrap may use bounded full replay. Missing roots on a larger prefix fail closed.
Open requires completed, repaired roots and validates the fixture Evidence binding and exact
current/history/adjacency/provenance/reverse/policy cardinalities. Reports retain the initial
`cold_admission` graph/metadata revisions, graph scan and semantic lookup work, and admitted
counts separately from `final_state_counts` after repair/materialization. Count order is current,
history, outgoing, incoming, provenance, reverse, policy history, current policy. These privileged
setup measurements exclude coordinator journal passes, suffix preparation, publication and query
work; they are not complete authenticated-I/O counters. The separate `suffix_recovery` object
reports private graph merge work/counts and declared recovery ceilings; it likewise excludes
proof reads, coordinator passes and terminal publication I/O. An already-current root is
reauthenticated and resynchronized without rotating slots, even on open. Construction
and open do not themselves run query verification or qualify BM-01/BM-06; storage metadata remains resident.
Native reports distinguish filesystem-adapter observation from authenticated cached-index work.
Decision 0085 adds cached primitive operation/error, page, hit, fragment and result-byte totals, including
work before errors, with separate warm-up and paired sample deltas. These maintenance-only
counters exclude uncached cursor/recovery/publication work and are not complete I/O or device
traffic measurements; `authenticated_io_accounting` is `partial-cached-primitives`.

The adapter observation retains call/failure counts and requested/returned read/write bytes.
Setup, query, warm-up and paired sample populations are
separate. These exclude credential/oracle-file I/O, internal syscalls and handle drops, and are
neither physical-device traffic nor complete authenticated-index statistics. Observation never
changes storage results; overflow invalidates measurements. No path or payload is retained.
The native tests require the experiment's `target` directory to reside on Btrfs:
`CARGO_BUILD_JOBS=1 cargo test --release --locked --offline native_disk -- --test-threads=1`.

`linux-disk-query` consumes the same separately generated bounded oracle summary as `linux-query`,
but uses native disk state and authorized reads throughout. It checks all exact outcomes, visits,
result sizes and digests, with a 64 MiB USTE cache cleared before each query. The query process
does not build oracle adjacency arrays. Its single-pass latency/RSS diagnostics are not sampling
or qualification: host caches remain uncontrolled, storage metadata remains resident, and this
command does not enforce the sampler's preemptive deadline. Reports state these limits explicitly.

`linux-disk-sample` instead uses a persistent, parent-supervised worker with the fixed 30-second
query deadline, separate warm-up and paired empty/retained-cache measurements. It shares the
unchanged sample plan and exact outcome/digest checks. Parent validation binds the engine schema,
sample windows, round/execution counts and enforcement claim. Native commands still retain their
development ceiling; this is not a qualifying-size campaign. Reports disclose resident storage
metadata and unmeasured complete authenticated I/O, omitting unavailable counters. As with the
legacy sampler, `engine_benchmark:true` denotes sampling while `budget_evaluation:not-performed`
and the nonqualifying label prevent it being mistaken for acceptance.

`oracle-summary` is intended to run separately from `linux-query`, so the independent oracle's
adjacency arrays do not enter the query process. The bounded 256 KiB summary pins profile/query
digests and exact output or limit outcomes without record identifiers. At exact scale it contains
299 successful outputs and 85 expected result-limit refusals. `linux-query` reopens the production
Btrfs database, clears USTE's page cache before each authorized traversal and checks exact outcome
equivalence. Its single-pass timings/RSS/counters are correctness diagnostics and the JSON still
sets `engine_benchmark:false`.

`oracle-bundle` uses one independent oracle construction to emit both the 96 disjoint warm-up
expectations and the measured section. Exact byte lengths, per-section digests and a combined
digest make substitution or truncation fail closed. At qualifying scale the warm-up section has 74
successful outputs and 22 expected result-limit refusals; it is groundwork for repeated sampling,
not benchmark evidence.

`linux-sample` validates the warm-up once, then executes each measured query as an empty-USTE-cache
and immediately retained-cache pair. It keeps success and typed-refusal latency populations
separate by depth and topology, plus all-topology groups for budget evaluation. Exact scale has no
lowering flags and always selects five complete samples of at least 60 seconds; scaled profiles run
one complete development round. The 30-second limit is rejected after a query returns, but the
documented CLI also runs a parent supervisor that kills and reaps its worker when an engine call
does not return within that limit. One worker remains alive across the campaign, preserving retained
cache state. The report still withholds budget evaluation and discloses uncontrolled host caches
and full-memory graph state. Only the engine call is timed; marker I/O and oracle validation are
outside that interval, and successful-work/index counters are reported separately for empty and
retained USTE cache states.

`linux-create-crash-probe --pause-after-revision REVISION` is an explicit process-loss harness.
It admits only a nonzero, nonfinal frontier, flushes a content-free readiness marker after that
transaction is durable, and parks until an external parent SIGKILLs the exact process. A subsequent
`linux-resume` must recover the prefix and finish the deterministic suffix. Do not use the probe as
an ordinary database creator.

`linux-disk-create-crash-probe --pause-after-revision REVISION` applies the same explicit harness
to the development disk path. Revision one pauses after policy certification but before bootstrap
roots; later revisions pause after graph publication and metadata rebase. Resume with
`linux-disk-resume`, then verify with `linux-disk-open` and `linux-disk-query`. The owned-child
SIGKILL/reopen/oracle CLI matrix is reproducible on Btrfs with:
`CARGO_BUILD_JOBS=1 cargo test --release --locked --offline --test disk_process_loss -- --test-threads=1`.
This tests process loss at selected durable prefixes, not hardware power loss or every I/O boundary.

## Oracle semantics

The oracle constructs independent outgoing and incoming adjacency arrays. For each depth level it
expands a stable entity-ordinal frontier and scans stable relationship-ordinal candidates.
`visits` counts every adjacency candidate examined, including a relationship revisited from its
other endpoint. Results are unique relationship ordinals; reachable entities are reported
separately and exclude the root. The default limits are global across the entire query: 1,000,000
visits and 100,000 unique relationship results. Exceeding either fails the query rather than
returning a truncated answer.

## Deliberate limitations

- The development verifier uses the durable memory fault model, deterministic development entropy
  and a test key wrapper, not the Linux adapter or portable recovery profile.
- The Linux sampler collects candidate latency, RSS, authenticated I/O and result-byte evidence,
  but the memory-adapter verifier does not.
- Multi-hop traversal is stable client-side composition of production one-hop authorized reads;
  there is no native multi-hop engine request yet.
- `qualification: qualifying-fixture-size` describes only exact fixture dimensions. It is not a
  performance or release claim.
- BM-01 still needs the exact sampler campaign under the accepted host reservation and removal of
  the full-memory graph boundary. BM-06 and streaming
  larger-than-memory recovery are outside this fixture increment.
