# Decision 0100 — Native disk-driver storage recovery

Date: 2026-09-19

Status: implemented and locally verified. Not T-20 completion or benchmark qualification.

Expose Decision 0099's opt-in cold-open mode through `AuthenticatedIndexRecovery`, retaining only
the exact final canonical transaction/inventory. Graph/domain and coordinator retry/transaction/
first-owner admission remain separate requirements before writer handoff. Add privileged storage
residency and historical cold-open work accessors to the disk coordinator, not consumer facades.
A nonempty-inventory transaction cursor fixture verifies exact captured/streamed outcomes and
inventory plus the charged certificate-proof range work.

Both disk development drivers now use this mode. The closed BM-01 graph fixture prohibits blob
inventories; admit zero storage blob/namespace/inventory/reference-binding counts and zero payload
verification bytes. Its derived catalog contains exactly the 20-byte metadata key and 48-byte
value on one page. Rebuild writes 68 logical bytes. Permit two point-read page visits: binary
search and entry read. Actual counters distinguish one authenticated page read from one cache hit.
The initial one-visit setting correctly refused native open; correcting this new derived bound
does not change a benchmark target, existing format cap or query limit. An initial test expecting
two physical page reads was also corrected to require the distinct read/hit counts.

The certificate/group allowance retains its profile-derived triangular proof term. Tail-descriptor
admission is bounded by the planned group count. All constructors validate through 100,000 entities,
but native execution still refuses more than 10,000 and the memory-model verifier remains capped
at 1,000. Zero-blob fixture success is not arbitrary-blob append or larger-than-memory evidence.

Derive `storage_metadata_memory_resident` from the journal's actual selected certificate/blob
residency modes, not from an empty dataset or an unconditional report literal. Native reports add
`storage_recovery`: final owner's cold validation/replay, discovery certificate work, catalog
admission/rebuild and resident-entry counts. A bootstrap may acquire more than one owner, so this
is explicitly `last-storage-owner-cold-open-only`, not total setup I/O. Existing setup adapter
counters span the aggregate observed operations; cached-index statistics still exclude uncached
recovery/publication. Catalog lookup reports now include cache hits rather than disguising them
as physical page reads. These counters remain privileged and content-free.

The sampler's parent requires the disk storage mode, zero retained history entries, the exact
fixture group count in both cold passes, zero blob verification/bindings and the unchanged partial
I/O declaration. Reject missing/substituted mode/count fields. Keep all deadline, sample-window,
round-count, outcome, result-size and digest checks. The legacy sampler is unchanged.

Missing/stale optional storage catalogs may rebuild during driver open. This is distinct from
repairing graph/coordinator state: only resume may perform a missing graph suffix, and a larger
prefix without the required graph/coordinator roots still fails closed. Query-phase adapter work
remains separate from setup work, including any catalog reconstruction. No authoritative data
migration, AgentMage change, M1 interface change, executable distribution or threshold reduction.

The process-loss assertion originally compared all manifests after refused graph open. With the
new optional catalog rebuild that assertion failed. It now requires every saved graph/coordinator
manifest to remain byte-identical, exactly one new catalog manifest, and a second refused open
to leave all manifests unchanged. All three owned-process tests and 51 active native unit tests
pass; two pre-existing exact-scale oracle tests remain separately gated. See PROGRESS for commands.

Remaining T-20 work includes disk-aware nonempty inventory append, domain-authorized charge
transfer, native larger-than-memory and exact BM-01/BM-06 qualification, and remaining complete
I/O accounting. The 24 GiB/16-logical-CPU/200 GiB qualifying reservation is not replaced by the
3G/4G/512M test scope. T-19 and all accepted later spatial/content/lifecycle/physics work remain.
