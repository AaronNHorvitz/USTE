# Decision 0141 — Scoped packed-root publication and recovery discovery

Date: 2026-09-19

Status: accepted and locally verified T-20 maintenance contract; domain integration remains open.

Connect Decision 0138's bounded append-only root slots to coordinator/recovery maintenance without
exposing the journal's key vault or directory. Ordinary discovery authenticates the exact current
frontier. Recovery discovery requires an explicit revision and separate certificate-chain and
manifest work limits; no implicit backward search or complete certificate history is constructed.
Names are always derived under the maintenance owner's namespace, not caller-supplied scope.

Publication remains a separate privileged operation on ordinary or recovery maintenance, not on
the private historical staging handle. Storage must require the exact current frontier revision
and digest before any creation. An older fully authenticated staged tree cannot publish an
intermediate recovery root. Current-revision descriptors may reference unchanged historical packs.

Discovered handles certify the manifest/certificate binding only. Decision 0140's bounded family
admission is still necessary for canonical content, and domain/reducer/state-digest admission and
consumer policy checks remain separate. A cold owner must discover and admit again; old live-owner
receipts are invalid. The journal remains authoritative and failed publication leaves optional
rebuildable artifacts, never a new commit. Attempt exhaustion and T-35 reclamation are unchanged.

No v1, M1, benchmark threshold, release gate or consumer handoff changes follow from this bridge.
