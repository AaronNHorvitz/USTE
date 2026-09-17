# Decision 0006 — Strict content adapter and sandbox profile

Date: 2026-09-16

Status: accepted candidate registry for R1–R3; each adapter remains unavailable until its
fixture, isolation and resolved-dependency audit passes.

Closes D-05 and is the decision artifact for T-04. “Candidate” here selects an exact
implementation path; it is not a support claim.

## Capability boundary

Opaque streaming storage is always in the privileged engine and has no parser dependency.
Every inspect/parse/enrich adapter is a separate unprivileged process speaking a bounded,
versioned stdin/stdout protocol over inherited descriptors. The supervisor launches a fixed
binary by registry ID—never a document-supplied path—with a private empty working directory,
read-only source descriptor, output descriptor, no ambient environment, no sockets, no home,
and Linux namespaces/seccomp/cgroup limits. Failure to establish every selected isolation
control refuses the job. The engine revalidates lease, output schema, provenance and quota.

Strict workers are offline Rust binaries. No macros, scripts, formulas, external entities,
links, fonts, embedded objects, archive links or document actions execute. Sniffing is subject
to the same byte/time limits. Default per-job caps are in Decision 0007.

## Versioned registry

| Gate/family | Registry ID and exact implementation candidate | Strict behavior |
|---|---|---|
| R2 text/Markdown/source/log | `text-v1`, first-party bounded UTF-8/UTF-16 decoder | exact byte/line map; other encodings partial/unsupported |
| R2 JSON | `json-v1`, `serde_json 1.0.151`, default features | max 32 nesting enforced before materialization; retain raw number token |
| R2 CSV/TSV | `csv-v1`, `csv 1.4.0`, default features | explicit dialect/encoding/mapping; byte and row locators |
| R2 XML | `xml-v1`, `quick-xml 0.42.0`, no features | streaming events; reject DTD/entity declarations |
| R2 HTML | `html-v1`, `html5ever 0.40.1`, no features | text/structure only; no fetch, script, style or active preview |
| R3 PDF | `pdf-v1`, `lopdf 0.45.0`, `default-features=false` | unencrypted text objects/pages; scanned/encrypted/unsupported filters are partial/password/unsupported |
| R3 OOXML | `ooxml-v1`, `zip 8.3.0` with only Rust `deflate-flate2-zlib-rs`, plus `quick-xml` | DOCX/XLSX/PPTX text/table locator subset; reject macros/links; no formula evaluation |
| R3 ZIP | `zip-v1`, same constrained `zip` build | list first; explicit expansion; safe regular entries only |
| R3 TAR | `tar-v1`, `tar 0.4.46`, `default-features=false` | uncompressed regular entries only; links/devices rejected |
| R3 PNG | `png-v1`, `png 0.18.1`, no `zlib-rs` feature | dimensions/metadata and bounded RGBA decode |
| R3 JPEG | `jpeg-v1`, `zune-jpeg 0.5.15`, `default-features=false, features=[std]` | dimensions/metadata and bounded decode; no SIMD/unsafe profile assumed |
| R3 audio | `wav-pcm-v1`, `hound 3.5.1` | RIFF/WAVE PCM 8/16/24/32-bit metadata and bounded sample/time ranges |
| R3 video | `y4m-v1`, `y4m 0.8.0` | YUV4MPEG2 4:2:0/4:2:2/4:4:4 headers and bounded sampled raw frames |

OCR and speech transcription return `UnsupportedFormat` in the strict v1 profile. They are
optional enrichments in the product contract; no model/weights are claimed. A future local
model profile requires exact redistributable weights, hashes, license, resource envelope and
worker audit. The selected WAV/Y4M baseline deliberately avoids FFmpeg and native codecs.
Legacy office, compressed TAR, encrypted ZIP/PDF, JavaScript rendering and proprietary media
remain opaque-storage-only.

## Dependency, license and unsafe review

Crates.io metadata on 2026-09-16 reports the versions/licenses above as MIT, Apache-2.0,
MIT OR Apache-2.0, or (for `zune-jpeg`) MIT OR Apache-2.0 OR Zlib; `csv` also offers
Unlicense/MIT. This is not the resolved audit. The lockfile, build scripts, native links,
features, source checksums, licenses and `unsafe` use must be inventoried after each worker is
added. Default `lopdf` and `zip` features are explicitly disabled because they add unnecessary
clock, parallel, image and native/compression surface. Any resolved C/C++ link fails the
strict profile pending a new approved optional-profile decision.

## Coverage and fixtures

Each registry entry gets tiny valid, truncated, oversized, deeply nested and hostile synthetic
fixtures plus precise coverage expectations. XML external entity/DTD, HTML scripts/URLs,
OOXML macros/formulas/external links, archive traversal/symlink/bomb, PDF password/scanned
pages, malformed image dimensions, WAV length mismatch and Y4M frame truncation are required
negative cases. The fixture manifest stores bytes, SHA-256, redistribution origin and expected
status/locator; unknown binaries must round-trip without invoking a worker.

`acceptance/r0/content-fixtures.tsv` is the normative fixture registry. `hex:` and `utf8:`
recipes are literal; `generated:` recipes are stable names reserved by the R0 materializer and
are materialized by `experiments/content-fixtures`. Exact byte counts and SHA-256 values are
pinned in `acceptance/r0/content-generated.tsv`. The manifest contains positive and
negative/inert cases for every baseline family, including unknown/zero bytes, depth, active
content, encryption, traversal, expansion, dimensions and truncation. The deterministic PDF
password fixture uses legacy PDF V1 encryption only to exercise `PasswordRequired`; it is not
the database encryption suite and is never accepted as a security profile.

The absence of a credible strict OCR/transcription dependency is visible, not a gate waiver:
the optional enrichments are not claimed. Exact R3 baseline parsing support remains gated on
implementation and VT-09/10 evidence.
