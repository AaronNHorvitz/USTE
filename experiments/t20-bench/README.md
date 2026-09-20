# T-20 benchmark fixture foundation

## Packed engine development equivalence

Decision 0166 adds an opt-in packed verifier alongside the unchanged v1 engine and native commands:

```text
cargo run --release --locked --offline -- packed-engine-check --entities 20
cargo run --release --locked --offline -- packed-engine-check --entities 1000
cargo test --release --locked --offline engine::packed -- --test-threads=1
```

It bootstraps only policy in the ordinary reducer, then uses packed graph/primary/quota roots,
authorized writes, one-outcome live overlays and metadata rebase after every existing fixture
batch. Cold open independently admits the terminal triple. Explicit origin rebuild streams every
transaction with zero recovery overlays; both paths preserve exact retries and all 384 independent
oracle queries. A final cold admission compares complete v1 logical-state digests. Storage uses
disk certificate/blob metadata recovery and queries use a bounded 64 MiB packed cache.

The filesystem, entropy/key wrapper and in-process oracle are development models. Requests above
1,000 entities fail before filesystem/key allocation. The 20/200 frozen oracle and the complete
1,000/10,000 development check pass. The latter's whole command (including compilation) took
250.81 s, peaked at 431,496 KiB RSS and recorded no query cache evictions; see the
[recorded observation](../../docs/evidence/packed-engine-1000-development.json).
These are not native durability, query-latency, cache-pressure or larger-than-memory qualification.
No existing fixture database is migrated or replaced by this command.

Decision 0167 adds native Linux/Btrfs terminal phases in the separate
`bm01-linux-packed-engine` database. With an existing private credential file and writable Btrfs
root (the same credential/root requirements as the native v1 runner):

```sh
cargo run --release --locked --offline -- oracle-summary --entities 20 > /tmp/uste-packed-oracle.tsv
cargo run --release --locked --offline -- linux-packed-create --root "$USTE_BENCH_ROOT" --password-file "$USTE_BENCH_PASSWORD" --entities 20
cargo run --release --locked --offline -- linux-packed-open --root "$USTE_BENCH_ROOT" --password-file "$USTE_BENCH_PASSWORD" --entities 20
cargo run --release --locked --offline -- linux-packed-resume --root "$USTE_BENCH_ROOT" --password-file "$USTE_BENCH_PASSWORD" --entities 20
cargo run --release --locked --offline -- linux-packed-query --root "$USTE_BENCH_ROOT" --password-file "$USTE_BENCH_PASSWORD" --entities 20 --oracle-file /tmp/uste-packed-oracle.tsv
cargo run --release --locked --offline -- linux-packed-rebuild --root "$USTE_BENCH_ROOT" --password-file "$USTE_BENCH_PASSWORD" --entities 20
```

Create refuses replacement; open fails closed on absent/corrupt terminal derived roots. Explicit
rebuild authenticates the fixture binding before reconstructing derived roots from the journal.
Decision 0168 resume supports authenticated data-bearing prefixes: choose the newest complete
same-revision graph/primary/quota triple, stream the certified suffix, then retry/continue the
unchanged batches. Complete cache loss is not implicitly rebuilt. Legacy policy-only prefixes are
not profile-bound and are refused. Decision 0169 newly binds policy retry/transaction identities to
the frozen fixture version and entity count. Resume can initialize an empty store or recover that
new policy-only prefix with at most one outcome, zero blob owners and 1 MiB replay. Larger prefixes
never use the ordinary reducer. Data-bearing legacy fixtures remain supported.
Decision 0173 explicit rebuild restores any authenticated bound prefix without appending batches;
reports include its actual frontier and `complete_fixture`. After partial-prefix cache loss, rebuild
explicitly before resume. Empty stores and legacy unbound policy-only rebuilds refuse. Open/query
still require a complete terminal fixture. Native tests verify 20 entities/200
relationships, real close/open, selected-prefix suffix recovery, partial publication, all batch retries,
384 oracle queries and authority preservation through rebuild. The unchanged 20,000-entity native
admission ceiling is not measured packed qualification. The explicit test-only
`linux-packed-create-crash-probe ... --pause-after-revision N` parks after durable creation (0),
policy acknowledgement (1), or graph publication before metadata rebase (data revisions). Use it
only with an owning supervisor that terminates and reaps its child. The process regression exercises
all five 20/200 boundaries, exact resume and all oracle queries; it does not emulate power loss.
Complete authenticated I/O and qualifying BM-01/BM-06 campaigns remain required. Native packed
BM-06 development history and recovery are described below.

Decision 0174 adds supervised packed sampling over a completed fixture. Generate the two-section
oracle bundle (not the single-section summary used by `linux-packed-query`):

```sh
cargo run --release --locked --offline -- oracle-bundle --entities 20 > /tmp/uste-packed-bundle.tsv
cargo run --release --locked --offline -- linux-packed-sample --root "$USTE_BENCH_ROOT" --password-file "$USTE_BENCH_PASSWORD" --entities 20 --oracle-file /tmp/uste-packed-bundle.tsv
```

The parent enforces the fixed 30-second query deadline and validates the worker's closed protocol.
One worker cold-admits the complete triple, executes 96 warm-ups and one development round of 384
empty/retained identical-query pairs with a 64 MiB packed cache. Reports separate successful and
typed-limit latency populations, cache work, adapter I/O and per-owner vault decrypt work. They do
not claim complete authenticated I/O, controlled host caches, performance qualification or budget
evaluation. The unchanged native
20,000-entity cap rejects qualifying dimensions before filesystem access; it is not measured capacity.

Decision 0176 separates last-cold-open-vault setup totals, warm-up deltas and empty/retained query
deltas. Successful envelope calls count header/padded ciphertext/tag bytes and returned unpadded
plaintext bytes; failed calls are separate. Retained cache hits do not imply new decryption.
Earlier discarded owners, key unwrap and pre-vault decode refusals are not included. Terminal
phase and single-pass query reports carry the same explicitly partial measurement boundary.

## BM-06 materialization

Decision 0170 adds `bm06-packed-check --records 2` (also accepts 1). It constructs all 100 versions
through packed authorized writes, verifies certified-tail recovery from the checkpoint, checks every
historical payload, then explicitly rebuilds from origin with zero overlays and checks all data-batch
retries and the terminal v1 digest. It uses memory-model storage/entropy/credentials and refuses more
than two records before allocation. This does not run native recovery trials or qualify BM-06.

```text
cargo run --release --locked --offline -- bm06-packed-check --records 2
```

Decision 0171 adds separate native packed BM-06 terminal commands. Reuse the existing private
credential/Btrfs-root setup; no existing v1 or BM-01 database is replaced:

```sh
cargo run --release --locked --offline -- bm06-packed-linux-create --root "$USTE_BENCH_ROOT" --password-file "$USTE_BENCH_PASSWORD" --records 2
cargo run --release --locked --offline -- bm06-packed-linux-open --root "$USTE_BENCH_ROOT" --password-file "$USTE_BENCH_PASSWORD" --records 2
cargo run --release --locked --offline -- bm06-packed-linux-resume --root "$USTE_BENCH_ROOT" --password-file "$USTE_BENCH_PASSWORD" --records 2
cargo run --release --locked --offline -- bm06-packed-linux-tail --root "$USTE_BENCH_ROOT" --password-file "$USTE_BENCH_PASSWORD" --records 2
cargo run --release --locked --offline -- bm06-packed-linux-recover --root "$USTE_BENCH_ROOT" --password-file "$USTE_BENCH_PASSWORD" --records 2
cargo run --release --locked --offline -- bm06-packed-linux-rebuild --root "$USTE_BENCH_ROOT" --password-file "$USTE_BENCH_PASSWORD" --records 2
```

Create ends at checkpoint 100 with 198 verified versions; tail certifies frontier 101 but leaves
derived publication pending, reports verification only through checkpoint 100 and claims no terminal
digest. Recover streams the suffix, verifies all 200 versions and exact-retries the tail. Decision
0172 resume validates existing history and finishes incomplete construction at checkpoint 100;
already certified frontier 101 stays 101. Only zero/one-revision prefixes use bounded ordinary
bootstrap (one outcome/zero owners/1 MiB replay); certified policy identities must match exactly.
Rebuild explicitly reconstructs the actual authenticated prefix, including incomplete construction,
without appending events. After all-cache loss, run rebuild before resume. Ordinary open still
requires a complete checkpoint/terminal triple; resume never silently reconstructs missing bases.

`bm06-packed-linux-create-crash-probe ... --pause-after-revision N` and
`bm06-packed-linux-tail-crash-probe ...` are test-only parked-child controls. Run them only under an
owning supervisor that terminates/reaps its child. Regression tests use real SIGKILL at bootstrap,
incomplete graph/metadata publication and final certified-tail boundaries, not simulated power loss.
The two-record cap is unchanged. None of these commands qualifies BM-06 or measures complete I/O.

`bm06-manifest [--records N]` emits the Decision 0114 versioned-event fixture manifest, not
a recovery measurement. Default 100,000 records each retain 100 versions (10 million events),
with 4096 payload bytes per version. The public `recovery_materialization::Bm06Profile::batch`
constructs at most 512 distinct-record creates/replacements at a requested durable revision,
so a future native materializer can resume without retaining earlier requests. Revision one
is reserved for the durable policy; the exact checkpoint/root boundary is 19,405 and the final
frontier is 19,601. History payload alone is 40,960,000,000 bytes; that is logical fixture size,
not measured memory/disk usage or proof of larger-than-memory recovery.

```text
cargo run --release --locked --offline -- bm06-manifest
cargo run --release --locked --offline -- bm06-manifest --records 2
cargo run --release --locked --offline -- bm06-disk-check --records 2
cargo test --release --locked --offline bm06 -- --test-threads=1
```

The synthetic stream digest matches the independent fixture generator's `events` output at
the unchanged BM-06 seed. The manifest does not hash 40.96 GB of canonical payload materialization;
its `canonical_request_stream_digest` is null and `database_materialized` is false. The
[pinned manifest](../../acceptance/r1/bm06-materialization-v1.tsv) also includes a small canonical
request-stream golden (two records, database/namespace IDs each sixteen `0x06` bytes; revision-one
policy excluded, each subsequent request prefixed with little-endian revision and byte length).
Small reducer tests inspect all historical versions and compare sequential reduction to decoded
checkpoint plus suffix. Bounded native development coverage follows below; scalable construction
and 30 reserved-host qualifying trials remain implementation work. No BM-06 pass is claimed.

Decision 0115 adds `bm06-disk-check`, explicitly capped at two records (200 real historical
versions). It uses authorized encrypted disk-state writes, deliberately refuses only the last
certified transaction's derived publication, cold-recovers that suffix from the selected root,
checks exact retry, reopens again and verifies every historical payload through authorized reads.
Its filesystem and credentials remain development models; JSON discloses that boundary and zero
qualifying trials. Exact-scale admission math is tested without running that database. All larger
profiles are rejected before filesystem/key allocation. This is not a native recovery campaign.

Decision 0116 adds native development phases on Btrfs with the same two-record ceiling, OS entropy
and the existing owner-only password-file rules. Supply an existing private root directory and
synthetic password file; these commands create only the distinct `bm06-linux-disk-engine` database:

```text
cargo run --release --locked --offline -- bm06-linux-create --root ROOT --password-file PASSWORD --records 2
cargo run --release --locked --offline -- bm06-linux-tail --root ROOT --password-file PASSWORD --records 2
cargo run --release --locked --offline -- bm06-linux-recover --root ROOT --password-file PASSWORD --records 2
cargo run --release --locked --offline -- bm06-linux-open --root ROOT --password-file PASSWORD --records 2
```

Create stops at revision 100; tail certifies 101 with intentionally uncompleted derived publication;
recover repairs it and checks exact retry; open requires the repaired frontier. Create never
overwrites. A failed partial directory is retained. Decision 0117 adds `bm06-linux-resume` with
the same root/password/record arguments: it verifies a bound prefix before completing revision
100, or repairs/verifies an already certified 101 without appending. New fixtures bind the record
count in the authenticated bootstrap retry identity; legacy unbound fixtures remain readable by
open/recover but cannot resume. Expired bootstrap retry evidence also refuses resume.
The supervised `bm06-linux-create-crash-probe` additionally requires `--pause-after-revision N`
(1–100). Tests exercise policy-only and populated prefixes with owned-child SIGKILL and resume.
Tests run `tail-crash-probe` as an owned child and SIGKILL it only after its flushed
durable-tail marker, then recover in fresh processes. Do not launch the probe without a supervising
parent: it waits indefinitely by design. Reports separate verified history from recovery work and
disclose partial adapter accounting, uncontrolled host caches and zero qualifying trials.
Native tests also corrupt/remove terminal cache manifests while retaining an earlier valid pair,
refuse complete graph-base loss, and repair exact incomplete certificate/journal tails. Reports
include `repaired_certificate_tail_bytes` and `ignored_uncommitted_journal_bytes`; committed
certificate corruption remains fatal. See the [control evidence](../../docs/evidence/native-bm06-recovery-controls.md).

Decision 0119 adds explicit `bm06-linux-rebuild --root ROOT --password-file PASSWORD --records 2`
for the terminal fixture, including complete loss of optional graph/coordinator roots. It privately
reconstructs only the authenticated first transaction, independently admits its staged indexes,
streams 100 later graph revisions, publishes/rebases terminal roots, checks exact retry and verifies
all 200 history versions. Ordinary open/recover retain their complete graph-base-loss refusal.
Decision 0120 now stages retry/transaction metadata to private disk roots after each revision,
with zero outcome-overlay capacity and zero admitted suffix outcomes. Reports expose the 300
metadata merges and their logical output work; this inventory-free path still rewrites immutable
families and does not qualify larger-than-memory recovery or general blob-owner origin staging.
Tests preserve committed journal/certificate bytes under complete missing/corrupt cache loss.

## BM-01 materialization

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
development keys and holds the independent oracle in process. It now opens with disk-backed
certificate/blob metadata (Decision 0100), with actual storage residency in its report. It is
semantic evidence, not performance or bounded total-RSS evidence.
The existing Linux commands are not silently switched by this development command.

`linux-disk-create/resume/open` use a distinct native Btrfs database name and portable recovery
credentials, with a separate 20,000-entity native development ceiling (Decision 0108, extending
Decision 0087 after the 10,000-entity observation had no cache evictions). Both
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
replay, including all certificate-proof re-reads, with explicit per-revision proof/merge limits.
Disk recovery no longer retains the complete certificate-anchor map; actual residency/counts
appear in `suffix_recovery.certificate_anchor_residency`. Suffix recovery now uses bounded
authenticated certificate windows with selected-byte rechecks (Decision 0127); conservative
admission bounds are not constant-time recovery or complete I/O measurements. Storage blob/inventory/namespace metadata now uses
Decision 0099's disk catalog. This closed graph fixture admits zero blob bindings and a one-entry,
68-logical-byte catalog; its exact point lookup allows two visits (one read plus one cache hit).
Only an empty or policy-only bootstrap may use bounded full replay. Missing graph/coordinator
roots on a larger prefix still fail closed. Optional storage catalogs can rebuild before that
graph admission, including on open; this does not authorize intermediate graph-root publication.
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
and open do not themselves run query verification or qualify BM-01/BM-06. The separate
`storage_recovery` object reports the last owner's cold-open validation/replay, catalog proofs,
admission/rebuild and actual resident-entry counts. A bootstrap can open more than one owner;
these work counters do not cover all owners or all filesystem I/O. Setup adapter counters remain
the separate aggregate adapter observation. Decision 0101 adds bounded nonempty inventory append
at the trusted storage boundary; its coordinator/authorized write integration is separate work.
Arbitrary-blob acceptance is tested at the storage boundary, not inferred from this
zero-blob fixture. Larger-than-memory and exact-scale performance qualification remain open.
Native reports distinguish filesystem-adapter observation from authenticated cached-index work.
Decision 0085 adds cached primitive operation/error, page, hit, fragment and result-byte totals, including
work before errors, with separate warm-up and paired sample deltas. These maintenance-only
counters exclude uncached cursor/recovery/publication work and are not complete I/O or device
traffic measurements; `authenticated_io_accounting` is `partial-cached-primitives`.
The historical `enumerated_fragments` field includes Decision 0106's sparse key probes as well
as enumerated fragments; `fragment_work_semantics` discloses this explicitly. Full-page parser
validation is still excluded. This partial counter is neither a comparison count nor CPU time.

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
or qualification: host caches remain uncontrolled, complete recovery I/O accounting is absent, and this
command does not enforce the sampler's preemptive deadline. Reports state these limits explicitly.

`linux-disk-sample` instead uses a persistent, parent-supervised worker with the fixed 30-second
query deadline, separate warm-up and paired empty/retained-cache measurements. It shares the
unchanged sample plan and exact outcome/digest checks. Parent validation binds the engine schema,
sample windows, round/execution counts and enforcement claim. Native commands still retain their
development ceiling; this is not a qualifying-size campaign. Reports expose disk storage residency
and unmeasured complete authenticated I/O, omitting unavailable counters. The parent validates
the disk storage mode, zero history-map entries and exact cold-pass fixture cardinalities. As with the
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
- BM-01 still needs the exact sampler campaign under the accepted host reservation. The legacy
  sampler retains its full-memory graph boundary; the native disk driver now uses disk-backed
  graph/coordinator bases, bounded suffix recovery, disk certificate proofs and bounded storage
  blob/inventory catalogs. Native packed development integration and BM-06 prefix/recovery tests
  exist, but complete authenticated I/O, scale and larger-than-memory qualification remain open.
