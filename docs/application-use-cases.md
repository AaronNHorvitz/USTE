# Native applications, future games and item/price tracking

Owner-requested scope clarification · 2026-09-17 · Planned, not implemented

USTE is a natively built Rust database with a physics/simulation kernel. Agent memory is one
consumer, not its exclusive purpose. Future games and item/asset tracking use the same generic
storage, graph, spatial, temporal and content contracts. This document clarifies FR-15/27/28/
31/33/34; it adds no external data service, trading system, new wire codec or universal physics.

## Future game development

A game can store world/item identities, ownership/containment relationships, geometry, local
frames, movement, attachments and event history. Its application layer supplies rules, inputs,
rendering and player interaction. USTE supplies bounded retrieval, durable state/history and
the declared kinematic/contact simulation profiles. A headless test is sufficient initially;
no renderer, editor, multiplayer server or game-engine integration is a release prerequisite.

Simulation facts are scoped to their virtual world/branch. They are not real-world observations.
Unanchored game ticks need no UTC date; an explicit versioned mapping is required when one is
requested. Inspecting state does not advance physics. The existing 2D/3D kinematic and constrained
2D contact baseline remains unchanged; general 3D game physics is still future scope.

## Tracking items linked to asset prices

Use ordinary application-defined entity/assertion schemas, not new privileged record kinds:

| Concept | Data supplied by the caller |
|---|---|
| Tracked item | Stable identity, optional quantity/unit, location history and source documents |
| Asset/instrument | Explicit identifier and relevant venue/contract/grade/network context; a ticker alone need not be unique |
| Item-to-asset relationship | Declared relationship and validity/evidence; physical stock is not silently equated to a financial contract |
| Price observation | Asset reference, exact scaled-integer amount/scale, quote currency or asset, price unit, price kind and source |
| Time/provenance | Source observation time, original timestamp interpretation, ingest/recorded revision, corrections and source-file/row locator |

For example, import a synthetic copper-shipment record, its location observations and a CSV
of copper-price observations quoted in USD per metric ton. Link them explicitly. A query can
return the shipment, its known location and the linked price record at a specified time and
knowledge revision, along with the source rows. Equivalent patterns can hold user-supplied
equity or crypto observations without understanding or contacting any exchange.

Use the admitted v1 integer/fixed-point types; do not silently convert exact prices through
binary floating point. Declare units and quote currencies; mixed units/quotes cannot be
aggregated or compared as interchangeable. Missing/stale/conflicting prices remain visible.
Selecting last-known price requires an explicit rule and reports observation age. Corrections
learned later cannot leak into earlier knowledge views. Local import is not a claim of live
data, market accuracy, executable quotes or supported trading performance. Automatic FX,
valuation, portfolio accounting and order execution are not added by this example.

## Ingestion and credential boundary

Inputs are local CSV/JSON, other authorized files, synthetic fixtures, or typed transactions
from the caller. A separate ETL may collect data from an external service; its connections,
provider secrets, entitlement handling and retry logic stay outside USTE. Storage access does
not authorize obtaining data from elsewhere. Mapping manifests and price records contain
source identity/provenance, never provider passwords or API tokens.

USTE does not ship native exchange/broker/feed clients, acquire accounts, automatically fetch
prices or require external-provider credentials to open a database, import local data, query
items or run physics. Its local API is an interface into the database. Local encryption keys
and principal authentication are separate requirements and are not removed. Initial tooling
or dependency acquisition may use development network access; runtime offline acceptance
uses prepared local inputs and the pinned build, not external services.

## Acceptance and work ownership

- T-28: generic consumers include a headless world client and tracking-domain mapping.
- T-54: preview/import/retry local price CSV/JSON with explicit units, timestamps and identities.
- T-56: demonstrate both consumer examples and linked item/location/price/source retrieval.
- VT-15/20/22/23: check consumer behavior, unchanged physics semantics, coherent historical
  queries, exact amounts, duplicate imports, malformed data, source corrections and isolation.

Run the new examples with outbound networking disabled and provider credential variables
unset, retaining local database key/authorization controls. No network mocks should be
needed: the fixtures enter through local import/transaction interfaces. Check missing prices,
unit/quote mismatch, future corrections and a foreign namespace's hidden prices. These are
acceptance requirements, not claims that executable examples already exist. No completed R0
task is reopened or marked differently by this application-level clarification.
