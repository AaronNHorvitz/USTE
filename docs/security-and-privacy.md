# Security and privacy

Draft threat model and contract · 2026-09-16 · Not an audit or certification

Owns FR-09, FR-10, FR-11, NFR-01, NFR-04 and shared security controls.

## Assets and adversaries

Protect source files, assertions, provenance, credentials/keys, indexes, journal history,
backups, namespace isolation, authorization state, and availability.
Treat client requests, stored documents, database imports, parser/model output, filenames,
archives, and recovery files as potentially hostile.

Threats include injection, privilege escalation through references, malicious parsing,
resource exhaustion, path traversal, file substitution, unauthorized historical retrieval,
deleted-data resurrection, artifact tampering, dependency compromise, and stale-backup rollback.

Boundary limits: encryption at rest does not protect plaintext in an unlocked authorized
process from a compromised kernel/administrator. Offline software cannot prove freshness
against an attacker replacing every file and local key-state copy with an old valid backup.
Stronger rollback resistance needs a specified external/hardware trust anchor.
No “government approved,” “CISA certified,” “unhackable,” or all-Rust-stack claim is authorized.

## Language and dependency policy

- Safe Rust for first-party storage, reducers, schemas, and query logic by default.
- Enable an unsafe-code prohibition in core crates unless a reviewed exception is necessary.
- Inventory dependency features, native build/link steps, unsafe boundaries, licenses,
  maintainers, advisories, and update policy for each supported target.
- Do not embed C/C++ database engines or disguise them behind Rust wrappers.
- The strict engine profile also excludes C/C++ spatial/GIS and physics implementations,
  including transitive algorithmic dependencies. Audit the resolved build, not package labels.
- Standard-library/OS interfaces and cryptographic primitives require documented boundaries;
  pure-Rust source is not proof that every instruction below it is Rust.
- Parser/model workers have separate inventories. Native dependencies require an explicitly
  approved optional profile, never silent inclusion in the strict profile.
- No crypto implementation written from scratch. Decision 0005 selects the exact v1
  library/suite and key-adapter boundary; Decision 0006 selects parser candidates and
  strict-profile feasibility.

## Authorization

Default deny. Trusted adapters bind a principal to a namespace and bounded operations.
Identity fields supplied by an LLM or document are not authentication.
Enforce permissions before graph expansion, content access, search ranking, aggregation,
export, and branch evaluation. Do not filter unauthorized results only after returning counts,
snippets, graph topology, or rankings that reveal their existence.

Historical content access uses current permissions, not the permissions at its original
revision. Revocation invalidates caches, parser leases, query handles, and subscriptions
within a documented bound. Worker results arriving after revocation are rejected.
Promotions, schema changes, imports, retention holds, key changes, and executable capabilities
have distinct permissions. Stored procedures are inert unless separately authorized elsewhere.

An embedded API protects against untrusted inputs, not an already compromised host process.
Use process separation when the consumer itself must not hold unrestricted database handles.

## Encryption and key lifecycle

Require authenticated encryption for sensitive blobs, journal payloads, index keys/values,
snapshots, worker outputs, backups, and temporary spill. Bind database/namespace identity,
format, object role, epoch and record identifiers as appropriate authenticated context.
Public framing and size/timing leakage must be documented; encryption does not hide everything.

Decision 0005 specifies random XChaCha nonces, distinct derivation contexts and restore/clone/
rotation rules across process restarts, partial writes, backups and object rewriting. Tests must
exercise those rules rather than substitute a resettable counter under a reused key.
Define unlock/lock, OS-keystore integration, recovery credentials, key loss, export keys,
zeroization limits, swap/core-dump behavior, and operator-controlled rotation.
Never log raw keys, credentials, extracted sensitive text, or raw low-entropy content hashes.
Authenticated content detects modifications, not necessarily replacement of all state by an
older authentic version.

## Local execution and content safety

Core is offline by default. File imports do not follow symlinks or fetch linked URLs by
default. Open/validate stable handles rather than trusting a path that can change after check.
Parser workers receive only authorized immutable versions, scratch space, output limits,
and a lease; no database-wide keys, home-directory access, sockets, credentials, or execution
tools. Enforce OS isolation, filesystem restrictions, process-tree limits, and no egress.

Use resource caps for CPU, elapsed time, RSS, output bytes, decompression ratio, nesting,
page/frame counts, tokens, dimensions and recursion. Terminate descendants on cancellation.
Format sniffing is an untrusted parser too. Malware detection, when available, is advisory;
it never makes a file “safe to execute.”

See [content requirements](content-ingestion-and-parsing.md) for macros, archives, HTML,
media, external models, and unsupported formats.

## Retention and deletion

Distinguish:

1. Retraction: retained history says a claim is no longer accepted.
2. Expiry: policy makes a record ineligible for normal retrieval.
3. Purge: remove retained content and all reachable sensitive derivations under the defined
   lifecycle, including worker staging, search entries, summaries, and embeddings.

Maintain minimal content-free audit/tombstone metadata only when policy permits. A digest,
filename, source locator, or relationship can itself be identifying data; do not assume it
is harmless. Holds conflicting with purge must produce an explicit decision, not false success.

Deletion tracks branches, readers, shared blobs, backups, caches, key copies, and derived
outputs. Deleting one deduplicated artifact may not erase bytes still lawfully owned by
another reference; communicate the distinction between reference removal and byte erasure.
Key destruction only provides cryptographic erasure if no usable keys/copies remain, and
shared keys cannot selectively erase one record. No promise of physical SSD sanitization.

Supported restore enforces the current deletion epoch before exposing data. External copies
and old exported keys cannot be recalled. Completion receipts distinguish immediate access
revocation, pending physical reclamation, and operator-controlled backup expiration.

## Security release evidence

Spatial histories, geometry bounds, navigation paths and simulated outcomes inherit source
sensitivity. Enforce authorization before spatial ranking/path expansion; invalidate dependent
indexes, trajectories, route caches and physics branches on source revocation/deletion.
Bound transform depth, geometric complexity, candidate expansion, collision pairs and steps.
Hostile geometry or a physics job cannot monopolize the service or bypass worker isolation.
See [spatial privacy](spatial-world-model.md) and [physics budgets](physics-and-motion.md).

Required: threat-model review, exact dependency inventory, unsafe review, negative
authorization tests, parser escape/exfiltration tests, corrupt-input fuzzing, deletion/restore
tests, reproducible build evidence and independent assessment before production.
No private reporting address or response SLA exists yet; [SECURITY.md](../SECURITY.md) makes
that distribution-readiness gap explicit. Decision 0011 permits local implementation while
T-62 remains open, but no executable may be externally distributed before verification.
