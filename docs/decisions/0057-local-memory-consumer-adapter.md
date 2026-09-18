# Decision 0057 — Restricted local memory consumer adapter

Date: 2026-09-18

Status: accepted T-67 implementation boundary at `a07bdec`. The adapter is an embedded Rust/Linux
pilot API, not stable public API, local IPC, a production service, or consumer-runtime integration.

## Decision

Add the safe-Rust `uste-memory-adapter` crate around the T-64–T-66 core. One
`LocalMemoryAdapter` owns one Linux filesystem capability, encrypted coordinator, policy-bound
principal and writer lock. It returns owned bounded values and exposes no raw coordinator,
filesystem, reducer snapshot, upload handle or namespace-scoped blob capability. Reads are
synchronous through `&mut self`, below the frozen eight-reader ceiling. The initial admitted host is
x86_64 Linux on the already qualified Btrfs profile; unsupported filesystems fail explicitly.

The consumer supplies a current `PolicyKernel` and principal from that same kernel at every open.
It also implements `ConsumerAuthority`, which remains authoritative for:

- the exact namespace and nonzero approval/synchronization generation;
- immutable source-version bytes;
- the version-1 bounded pending-upload checkpoint; and
- durable insertion/removal of pending upload records.

The adapter starts an upload, requires `persist_pending` to return durably before writing source
bytes, then streams/finalizes the blob and commits the source using caller-stable idempotency and
transaction IDs. After restart it resumes the exact token, obtains the current authoritative source
bytes, completes the same request, verifies the finalized length and SHA-256 for text and opaque
sources, and only then clears the checkpoint. Changed bytes fail `SourceChanged`. If clearing fails,
the same committed request is retried safely on the next open.

`ConsumerCheckpoint::validate` rejects unknown schema versions, zero generations, wrong scope,
foreign or duplicate tokens, invalid source versions and more than eight pending uploads. A
generation can advance only by one through `begin_rebuild`; it becomes the adapter's serving
generation only after the durable begin commit succeeds. The source store is neither moved nor
modified by rebuild.

Citation resolution is one restricted call: it first performs record/source/blob authorization,
then reads at most the admitted one-source bound and returns the citation plus exact original bytes.
No raw reference-to-read operation is public. Cancellation and typed content-free error categories
remain visible. `USTE_MEMORY_LOCKED`, `USTE_MEMORY_UNSUPPORTED_VERSION`, wrong-generation,
revocation and recovery errors are explicit.

## Demonstration boundary

`scripts/run_memory_pilot_demo.sh` runs `uste-memory-demo` offline with one Cargo job and one test
thread. The synthetic demo uses a fixed non-secret password and a synthetic policy only. It creates
separate `consumer-authority/` and `uste-derived-index/` directories, then demonstrates exact ingest
and citation, competing-owner refusal, reopen, correction and historical lookup, a visible
contradiction, revocation, and generation-2 rebuild from the unchanged source.

The demo checkpoint uses a small strict `UMCP` version-1 codec and owner-only files as a concrete
sample of the trait contract. It is not a general consumer database, credential manager or stable
wire protocol. A real consumer must map its own durable transaction/outbox mechanism and must never
copy the demo password.

## Consequences

The embedded adapter avoids inventing an IPC security boundary for M1. A future service must still
add authenticated peers, framing, ownership, key custody and bounded request handling. AgentMage is
not modified, stopped or migrated. T-68 must still execute process termination, disk/full/corrupt/
wrong-key fault cases and measurements, then publish the exact consumer handoff. T-62 remains a
distribution-only prerequisite.
