# R0 unsafe and platform boundary review

Date: 2026-09-17 · target: x86_64-unknown-linux-gnu · status: product/candidate review, not audit

This review covers direct dependencies selected by Decisions 0005/0006 at the pinned versions
in `experiments/dependency-audit/Cargo.lock`. The Decision 0005 subset is now also admitted to the
root product `Cargo.lock` through `uste-crypto` with those exact features; the parser candidates
remain confined to the experiment graph. This review distinguishes source that contains `unsafe`
from code reachable under selected features. Transitive blocks still require ongoing review,
fuzzing and advisory monitoring; this document is not an independent security assessment.

## Engine/key dependencies

| Crate | Selected features | Direct unsafe boundary | Disposition |
|---|---|---|---|
| `chacha20poly1305 0.11.0` | alloc, getrandom, zeroize | no `unsafe` token in crate `src`; CPU/entropy transitives use platform/optimized code | cryptographic adapter only; wrong-key/context/tag vectors required |
| `argon2 0.6.0` | alloc, getrandom, password-hash, zeroize | raw allocated block array, lane/slice pointer views, `Send`/`Sync`, optional AVX2 dispatch | key-recovery adapter only; fixed memory/lanes, upstream vectors, Miri where supported |
| `getrandom 0.4.3` | std | OS syscall/platform implementations | sole entropy boundary; fail closed, deterministic injection only in tests |
| `libc 0.2.189` | transitive/platform | OS ABI calls | Linux adapter only; never exposed as arbitrary syscall capability |

No first-party core crate contains unsafe code. `uste-crypto` has `#![forbid(unsafe_code)]` and its
root product graph has no native `links` package. Dependency unsafe and the OS syscall boundary are
not hidden by that lint and remain inventory.

## Parser/fixture candidates

| Candidate | Selected-feature observation | Boundary and control |
|---|---|---|
| `serde_json 1.0.151` | unchecked UTF-8/pointer offsets and representation casts internally | isolated structured worker; pre-scan depth/bytes; fuzz raw-token and malformed UTF-8 cases |
| `csv 1.4.0` | unchecked UTF-8 after its validation path | worker uses byte records first and validates chosen encoding before strings |
| `quick-xml 0.42.0` | `unsafe` occurrences in selected source are comments suggesting unused optimizations | DTD/entity rejection still occurs before output acceptance |
| `html5ever 0.40.1` | architecture SIMD tokenizer fast paths contain unsafe | separate worker; no DOM execution/fetch; malformed-token fuzzing required |
| `lopdf 0.45.0` | no `unsafe` token in crate `src`; parser/crypto/decompression transitives remain | separate PDF worker; page/object/output/decompression limits; scanned/encrypted states explicit |
| `png 0.18.1` | no direct `unsafe`; compression/checksum transitives use optimized code | dimension/pixel/output caps precede decode allocation |
| `zune-jpeg 0.5.15` | AVX2/NEON unsafe exists in source, but `x86`/`neon` defaults are disabled; selected feature is only `std` | keep SIMD features disabled; process isolation and malformed-image fuzzing |
| `zip 8.3.0` | POD layout casts plus unsafe writer/metadata APIs; default crypto/compressors disabled | worker never calls unsafe metadata constructors; list-first path/type/ratio checks |
| `tar 0.4.46` | header layout casts and Unix sparse-file syscalls | inherited byte stream only; no extraction helpers; reject links/devices/sparse extensions |
| `hound 3.5.1` | unchecked sample writer/buffer paths; reader is the production path | strict worker parses bounded PCM; writer is fixture-only |
| `y4m 0.8.0` | unchecked vector length initialization in frame reader | header-derived plane sizes checked against pixel/frame caps before frame reads |

`encoding_rs`, `memchr`, `simdutf8`, compression crates, hash containers, synchronization and
CPU-detection crates add further unsafe/platform surface. Cargo metadata reports no native
`links` package in the selected Linux graph, but Rust SIMD and OS calls remain real unsafe
boundaries. “No C/C++ engine” is therefore maintained without claiming an all-safe stack.

## Review outcome

- No dependency silently introduces a native database, GIS or physics engine.
- Parser unsafe is never placed in the privileged storage process by this profile.
- The JPEG SIMD source is not reachable under selected features and must stay disabled.
- Argon2 memory/pointer code and platform entropy are the principal privileged dependency
  exceptions. T-11 covers real fixed-profile execution and injected boundary failures; upstream
  vectors and supported sanitizer/Miri campaigns remain T-39 hardening and T-41 review evidence.
- ZIP/TAR write APIs are fixture-only. Production workers expose bounded readers and validated
  outputs, not archive filesystem extraction.

This closes T-04's R0 identification/classification portion of the unsafe inventory. Actual
worker binaries must still prove resolved feature graphs, sandbox controls and hostile fixtures
under T-23/T-24 and later adapter tasks; NFR-01 remains continuous through every release.
