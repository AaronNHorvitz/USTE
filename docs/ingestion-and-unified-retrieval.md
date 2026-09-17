# Ingestion and unified retrieval

Draft contract · 2026-09-16 · Not implemented

Owns FR-33/34 and composes graph, spatial, temporal and content requirements. A query API
does not imply an external network call. The first interface is structured typed operations;
a custom textual query language, SQL/Cypher compatibility and natural-language query planner
are not required. Natural-language clients must translate into validated bounded operations.

## Data entry

Rust callers submit transactions directly to the owning engine. Approved local clients use
authenticated IPC/CLI operations. A separate ETL can read CSV/JSON, another database, sensor
output or an external service; the core does not fetch these sources automatically.
The importer receives only explicitly granted source and destination capabilities.

Item-to-asset relationships and price observations are ordinary caller-supplied data under
[application use cases](application-use-cases.md). The local batch importer does not become
a market-data client: no native feed/exchange/broker calls, provider logins or API keys are
part of this requirement. Preserve exact scaled amounts, quote units/currencies, asset IDs,
source timestamps and correction history using the admitted v1 value types.

Baseline R2 batch tooling reads JSON records and CSV with an explicit mapping manifest:
schema/version, identity keys, relationship endpoints, source-event IDs, timestamp roles and
units, timezone assumptions, coordinate frames/axis order, numeric units, sensitivity and
retention. Strings resembling dates or coordinates are not automatically trusted facts.
Source locators identify original files/rows and byte versions; retain authorized originals
or an explicit source-capture reference. Typed API imports use the same validation path.

1. Inspect bounded source metadata and validate mapping, permissions and admission limits.
2. Produce a no-write preview with row counts/errors and proposed transformations.
3. Stream accepted rows into bounded atomic batches with stable source keys/idempotency IDs.
4. Commit each batch's records, relationships and source provenance consistently.
5. Return durable revision ranges, checkpoint, accepted/rejected counts and bounded error report.

Each batch is atomic, not the entire multi-GiB job. The initial policy stops at an invalid
batch; earlier commits remain explicit. Skipping/quarantining rows requires an explicit mode
and encrypted bounded reports. Resume verifies source digest, mapping version and checkpoint;
changed input refuses blind continuation. Retry cannot duplicate effects. Cancellation and
lost replies use durable outcome lookup. Referential ordering/staging is explicit: no dangling
relationships while later batches catch up. A Python client is a later convenience, not a core
Rust dependency or a prerequisite for ETL.

## One bounded read view

A structured request contains principal/namespace, knowledge revision, optional world-time
instant/interval, optional branch, candidate filters, projections and work/result budgets.
Supported operators compose identity/property lookup, graph traversal, spatial predicates,
temporal predicates and content search. D-08/FR-33 define legal combinations and closed errors;
unsupported combinations fail rather than silently dropping a filter.

Execution authorizes before expansion/ranking, selects index candidates, applies final
predicates against one coherent view, then resolves requested records and authorized artifact
handles. The optimizer must preserve meaning when changing operator order. Index rebuilds,
late corrections or parser jobs cannot expose a half-updated view. Absence of parsed text
does not imply absence of a matching stored file; report coverage limitations.

Responses contain actual entity IDs/versions and requested properties; observed/estimated/
simulated state; geometry/frame/time provenance; matched relationships; evidence locators;
authorized original/range-read handles; source/derivation versions; coverage, ranking reasons
and truncation. Large bytes are streamed separately after current authorization checks.
A stale/revoked handle cannot become a permanent bearer capability. No LLM is needed to read
an item or original file. Unsupported binaries remain retrievable as opaque bytes.

## Product acceptance fixture

Create a synthetic world with two organizations, vehicles, a road graph, a geographic region,
local depot frame, location observations and linked text/JSON inspection records plus an
unknown binary. R3 adds a PDF fixture and polygon/crossing checks. Include an object without
location and a document without a physical owner.

Demonstrate identity lookup and byte round-trip; scoped graph path; nearby objects at a
specified time; inspection text plus relationship plus location filter; observed versus
interpolated state; late correction at a later knowledge revision; branch prediction;
UTC/local display; and an explicitly unreachable route. Return source-backed objects, not
only prose. Hide a second namespace's closer object and shorter route without leaking either.
Restart, cancel/resume an import, rebuild indexes, purge a source and test stale handles.

Extend the fixture with local CSV/JSON price observations linked to tracked items and a
headless game-world consumer. Query an item's location, linked price and source at one
knowledge view; expose missing/stale prices and reject incompatible quote/unit comparisons.
Later corrections must not enter earlier views. Run without outbound network or provider
credentials, while retaining local database keys and authorization. No live-data or trading
performance claim is part of this example.

VT-22 compares composed queries with a simple authorized reference scan; VT-23 proves privacy,
import idempotency and lifecycle behavior. BM-13 measures p50/p95/p99, peak memory, ingest rate
and backpressure under simultaneous content extraction, movement updates and physics.
