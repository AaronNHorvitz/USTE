# M1 local memory pilot demo

This is a synthetic, embedded Rust demonstration of `memory-pilot-v1`. It requires x86_64 Linux,
Rust 1.95.0, a Btrfs directory, this exact branch, and dependencies already fetched into Cargo's
local cache. It does not need network access, a model, a hosted account, provider credentials or a
consumer runtime.

## Run

From the repository root:

~~~bash
git switch codex/uste-implementation
git rev-parse --short HEAD
CARGO_NET_OFFLINE=true scripts/run_memory_pilot_demo.sh
~~~

The script creates a retained run directory under `target/memory-pilot-demo-runs/` so its encrypted
artifacts can be inspected. To use another admitted empty Btrfs directory, pass it as the only
argument. It never overwrites a nonempty directory.

Expected semantic output (the run-directory suffix is intentionally variable):

~~~text
M1_DEMO_ROOT=.../target/memory-pilot-demo-runs/run.XXXXXX
M1_DEMO phase=created revision=4 fact=20 citation=exact
M1_DEMO phase=lock-conflict result=USTE_MEMORY_LOCKED
M1_DEMO phase=reopened current_facts=2 historical_fact=20
M1_DEMO phase=revoked result=USTE_MEMORY_NOT_FOUND
M1_DEMO_OK schema=memory-pilot-v1 generation=2 revision=11 source_authority=consumer
~~~

Success means the source-backed fact and exact bytes survived encrypted reopen; a second writer was
refused; the current correction and independent contradiction were visible while the predecessor
remained historically queryable; revocation hid the citation; and a new generation rebuilt from the
unchanged consumer source. It does not qualify production durability or all M1 fault/measurement
cases by itself.

## Layout and authority

The demo root contains:

~~~text
consumer-authority/
  consumer-source-v1.txt
  consumer-checkpoint-v1.bin
uste-derived-index/
  memory-demo/                 # encrypted USTE journal/blob objects and writer lock
~~~

The consumer directory is the source authority. USTE receives a capability rooted only at
`uste-derived-index/`. `consumer-checkpoint-v1.bin` carries schema version, scope, authority
generation and at most eight pending uploads. A pending entry is durable before staging begins and
contains the token, source version, encoding, event time and stable retry identities.

## Rollback and cleanup

For an integration rollback, stop/drop the adapter, disable shadow queries, preserve the consumer
source/checkpoint, and move the complete derived-index directory aside for investigation or remove
that exact directory under the consumer's normal data-handling policy. Create a fresh empty admitted
index root, advance the consumer generation once and reimport approved sources. Never reset the
consumer source store merely to repair this derived index.

Removing a demo/index directory is logical disposal, not proof of physical erasure from journal
history, backups or media. Do not place personal data in this pilot. A stale checkpoint temporary
file or any integrity/key error is a visible stop condition; do not silently reset or downgrade it.

## Supported operations and limits

The adapter supports exact text/opaque source ingestion, facts and explicit links, corrections,
durable source revocation, generation rebuild, identity/lexical/one-hop reads and exact citation
bytes under the frozen limits in Decision 0055. The consumer supplies stable operation IDs, policy,
scope and current generation.

It does not support arbitrary parsers, model execution, interval/spatial predicates, ranking,
embeddings, IPC, concurrent writers, authoritative migration, physical purge, backup/restore or
larger-than-memory state. Those remain on the preserved roadmap.
