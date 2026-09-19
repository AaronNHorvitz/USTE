# Decision 0140 — Scoped packed-index maintenance

Date: 2026-09-19

Status: accepted and locally verified T-20 maintenance contract; domain integration remains open.

Expose the Decision 0139 journal bridge through an exclusive coordinator/recovery maintenance
handle pinned to a namespace and authenticated certificate target. Ordinary coordinator maintenance
authenticates its current frontier with caller-specified certificate-read limits. Recovery accepts
only an opaque authenticated transaction: reuse its live-owner proof when available, otherwise
perform explicitly bounded authentication of its exact revision/digest. Never silently retry a
rejected retained proof through a different trust path.

Admission, point reads, cursor creation and private staging reject another namespace or a tree
newer than the pinned target before tree I/O. Storage still checks owner/key/canonical capability
and exact staging identity. Cursors permanently fail after any scope, revision or storage error,
including an attempt to use a future cursor through an older recovery target. Successful later
maintenance may read an older tree, subject to the caller's separate domain/policy admission.

The handle cannot append transactions, alter retry metadata or publish roots. Private historical
stages do not become visible intermediate commits. Root publication/discovery, versioned domain
semantics and consumer authorization are separate subsequent integration work. This preserves the
existing v1, native development and M1 profiles; no benchmark or task completion follows from this
bridge alone.
