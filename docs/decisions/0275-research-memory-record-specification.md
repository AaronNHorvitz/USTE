# Decision 0275: Research, documentation and repository memory record specification

Date: 2026-09-24

Status: Accepted as the DB-R02 specification; no implementation, capability or release claim.

DB-R02 (CAP-20, CAP-22, CAP-27, CAP-29) asks USTE to specify versioned source, artifact, claim
and edge records for research, documentation and repository memory with scope, provenance,
revisions, freshness and budgets, without fetching the web or executing tools inside storage.
The completed memory pilot (T-63–T-68) already stores source versions, facts with exact
locators, corrections, retraction, source revocation and generation rebuild, but it has no
retrieval provenance, fetch outcome, freshness, support kind, typed edges or research-scale
budgets.

Adopt [research memory records](../research-memory-records.md) as the `research-memory-v1`
contract. It reuses the pilot's source-version identity and locator shapes and the data model's
assertion lifecycle, adds `Source` retrieval provenance and fetch outcome, `Artifact`
derivations, `Claim` support kinds with mandatory citations (or an explicit `unsupported`
label), excerpt-digest `Citation`s and typed `Edge`s, and freezes per-namespace budgets before
implementation. USTE performs no network, DNS, search, tool or model activity; stale or
inaccessible sources are stored states, never refetch triggers; a derived index is neither a
source of truth nor a permission; and relations never grant cross-repository access.

The alternative of extending `memory-pilot-v1` in place was rejected: its frozen M1 profile and
limits are acceptance evidence and must stay unchanged. The new profile is a separate record kind
with its own format version, and pilot records map to it read-only through a separately qualified
step. Purge remains owned by T-34; until then the profile supports revocation and rebuild only.

The package is split into DB-R02.1 (this specification), DB-R02.2 (codec and golden vectors),
DB-R02.3 (reducer, budgets and reference model), DB-R02.4 (bounded authorized queries with
citation, freshness and truncation reporting) and DB-R02.5 (pilot mapping fixtures). Only
DB-R02.1 is delivered here; DB-R02 stays open. Verification: documentation and task-graph checks.
