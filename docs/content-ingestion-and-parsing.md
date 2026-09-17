# Unstructured content, ingestion, and parsing

Draft contract · 2026-09-16 · No parser or file support is implemented yet

Owns FR-17 through FR-23 and the content portions of FR-13/FR-24.

## Store any format; declare what can be understood

The blob store accepts arbitrary byte sequences within admission, quota, and size limits.
No recognized extension or successful parsing is required to preserve authorized original
bytes. This includes unknown binaries, multimedia, archives, office files, executables, and
proprietary formats. Admission may reject prohibited content or refuse unbounded workloads.
“Any format” is not unlimited size, automatic safe rendering, or universal semantic support.

Separate contracts:

| Capability | Meaning |
|---|---|
| Store | Preserve and retrieve exact bytes with version, integrity, and policy metadata |
| Inspect | Report supplied/detected media type, size, encoding hints, and structural metadata |
| Parse | Produce explicitly supported structural/text output without executing content |
| Enrich | Optional OCR, transcription, embeddings, or model interpretation with provenance |
| Retrieve | Return permitted originals/ranges or derived chunks within explicit budgets |
| Cite | Resolve a returned passage/value to its exact retained source version and locator |

## Format matrix and release obligations

Every row is planned, not an availability claim. Decision 0006 selects exact v1 adapter
candidates, dependencies, supported variants and fixtures. R0 selection does not make an
adapter available; its implementation and gate-specific isolation/fixture evidence remain due.

| Family | Required baseline behavior | Earliest parsing gate |
|---|---|---|
| Unknown/proprietary binaries, executables, disk images | Opaque storage and metadata; no execution or implied interpretation | R1 storage only |
| Plain text, Markdown, source code, logs | Bounded decoding, exact byte/line mapping; code stays inert | R2 |
| JSON, CSV/TSV, XML, HTML | Bounded structural/text extraction, preserve types/raw tokens; no XML external entities, DTD fetches, scripts, or remote resources | R2 |
| PDF including scanned pages | Page-aware text; OCR only through an approved worker; report partial extraction, encryption, layout loss, or unsupported features | R3 |
| OOXML documents, spreadsheets, presentations | Text/table/sheet/cell/slide extraction; do not execute macros, formulas, external links, or embedded objects | R3 |
| Images (initially PNG/JPEG) | Bounded decode, dimensions/metadata, optional local OCR with region locators | R3 |
| Audio/video | Decision 0006 selects WAV PCM and Y4M baselines; bounded local metadata, optional transcription and sampled-frame extraction with time locators | R3 |
| Archives (initially ZIP/TAR) | List entries first; opt-in bounded expansion, per-entry provenance and independent admission | R3 |
| Legacy office files, other image/media codecs, CAD/scientific/proprietary formats | Opaque storage and explicit unsupported parsing until an adapter passes its gate | Later adapters |

R3 must demonstrate the named baseline families and at least one exact audio and video
format/model profile chosen at R0. Native-code restrictions may limit available adapters;
if no acceptable implementation exists, record a product-scope/security-profile decision
rather than silently wrapping an unreviewed native library. OCR/transcription are optional
per request and must have at least one tested local configuration for claimed support.
Advanced vision reasoning and universal multimodal interpretation are not release promises.

## Ingest protocol

1. Authorize namespace, source, sensitivity, size and storage quota; reserve bounded capacity.
2. Stream into encrypted private staging; count actual bytes rather than trusting a header.
3. Capture supplied name/type separately from detected type; record mismatches without
   trusting the extension. Validate stable source handles and reject unsafe path traversal.
4. Verify content identity/integrity; finalize immutable blob objects durably.
5. Commit the new artifact version, evidence and object references atomically.
6. Only then acknowledge availability; enqueue an authorized parsing request separately.

Upload/session IDs enable resumption without exposing other clients' staging. Resumption,
interrupted cleanup, corruption checks, and expiration require tests. Unknown-length streams
stop at quota. Zero-byte content is valid unless a declared schema prohibits it.
Equal bytes do not merge artifact identities or cross namespace policy boundaries.

Timestamp metadata follows [time and ordering](time-and-ordering.md). Preserve source
created/modified/published claims separately from host receipt and committed availability.
Missing zones, ambiguous dates and unsupported time scales do not prevent opaque byte storage.
Derived timestamp interpretations retain exact field provenance and normalization versions.
Neither an old modification date nor a later extraction makes content available at an earlier
knowledge revision. Media offsets remain relative unless an absolute origin is supplied.

## Worker boundary

Parsing, preview generation, archive expansion, decoding, OCR, transcription, and embeddings
run outside the privileged storage process. A job binds source version/digest, namespace,
current permission lease, parser/model/config version, resource budget and output schema.
Workers get narrow read-only handles and isolated scratch space, not arbitrary host paths.
They cannot write committed records or inherit consumer credentials.

The supervisor enforces no egress, no arbitrary process launch, resource/time caps,
cancellation of descendants, scratch cleanup and permission revalidation before commit.
Output passes size, schema, provenance and content-policy validation. A worker crash cannot
crash or corrupt the database; repeated failure is quarantined with a bounded retry policy.
No automatic plugin installation, model download, remote parsing, or cloud fallback.

Archive paths reject absolute paths, parent traversal, symlinks/hardlinks and device entries
unless a future explicitly safe policy exists. Bound depth, entry count, expanded bytes and
compression ratio; do not trust archive-declared lengths. Office containers follow the same
rules. Do not evaluate spreadsheet formulas; report cached values separately from formulas.
Passwords are supplied through ephemeral capability-scoped input, not stored in evidence.

## Processing states

Storage and parsing have separate state machines. Stored does not mean parsed.
Processing outcomes include NotRequested, Queued, Running, Complete, Partial, Unsupported,
PasswordRequired, Malformed, LimitExceeded, Cancelled, Quarantined and Failed.
Complete is relative to the advertised extraction contract, not proof of full semantic
understanding. Partial outputs must list omitted pages/entries/regions or unknown coverage.
Transient errors and permanent unsupported states are distinct; retries are idempotent.

## Derived representations and locators

Retain original bytes as immutable authority when policy permits. Extracted text, tables,
metadata, previews, OCR, transcripts, summaries, and embeddings are versioned derivations.
Never overwrite an original with normalized text.

A derivation identifies every source version, adapter/build/config, model/weights identity
when used, captured inputs, output schema, coverage, errors and confidence semantics.
Deterministic re-extraction is not assumed: preserve accepted output needed for replay.

Locator types include original byte ranges; decoded line/character mappings; PDF page and
bounding box; spreadsheet sheet/cell; slide/shape; image region; archive member/version;
audio/video time interval; and structural paths into parsed JSON/XML.
Coordinates declare units and origin. A chunk may have multiple source spans.
If exact mapping is unavailable, identify the artifact/page and explicitly mark the locator
as approximate. Never invent precise citations for a paraphrase or model-generated claim.

## Retrieval and agent assistance

Provide discover, inspect, bounded original/range read, parse-request/status, chunk search,
table extraction, citation resolution and derivation inspection operations.
Results carry format/status, source version, scope, sensitivity, coverage and provenance.
Return handles plus requested ranges rather than loading whole files into model context.
Binary payloads are not silently inserted into prompts as text.

Index lexical chunks, graph relationships and optional embeddings under the same access
boundary. Embedding model/dimension/version changes require a new index generation.
Fresh parsing or model output produces proposals, not approved knowledge. Source instructions
remain untrusted quoted data; the consumer must preserve that boundary in its prompt/tool
handling too. The database cannot guarantee an external model will ignore prompt injection.

## Lifecycle, previews, and verification

A new file version invalidates “current” derivations according to policy while retaining
authorized history. Revocation/deletion propagates to queued jobs, active leases, chunks,
previews, summaries, embeddings, branches, scratch, and backed-up copies within the supported
retention contract. Reprocessing cannot restore deleted source content.

No auto-open or active HTML preview. Download/preview consumers must use safe disposition,
isolated origins where applicable, and never execute embedded scripts or macros.
Store diagnostic codes and bounded redacted errors, not raw sensitive parser dumps.

Required evidence: unknown binary round-trip; mixed encodings; corrupted/truncated formats;
encrypted documents; large images; recursive/archive bombs; traversal and symlink attempts;
external-resource requests; macros/formulas; hostile document instructions; source replacement;
worker cancellation/crash; revoked lease; stale derivations; citation resolution; and
deletion/recovery across original and derived copies. Exact limits live in the versioned
acceptance manifest, not hardcoded undocumented defaults.
