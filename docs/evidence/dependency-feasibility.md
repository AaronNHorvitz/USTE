# Strict-profile dependency feasibility snapshot

Date: 2026-09-16 · Rust/Cargo 1.95.0 · target: x86_64-unknown-linux-gnu

This is an R0 candidate audit, not final dependency admission, security review or SBOM.
`experiments/dependency-audit/Cargo.toml` pins every Decision 0005/0006 candidate and its
`Cargo.lock` records the resolved graph (116 packages for all target conditions).

## Results

~~~text
cargo build --manifest-path experiments/dependency-audit/Cargo.toml
# Finished dev profile successfully

cargo tree --manifest-path experiments/dependency-audit/Cargo.toml --locked |
  rg '(-sys|cc v|cmake|bindgen|clang|openssl|zstd-sys|bzip2-sys|libz-sys)'
# no matches
~~~

Cargo metadata reports no package with a nonempty native `links` field. The selected ZIP
deflate backend is `zlib-rs`, not system zlib; TAR still uses `libc`/`filetime` for platform
filesystem behavior. The strict worker will parse TAR from an inherited stream and must not
expose archive filesystem helpers.

Every resolved license expression offers one or more of MIT, Apache-2.0, BSD-3-Clause, Zlib,
Unlicense, 0BSD, BSL-1.0 or CC0-compatible terms. Target-only `r-efi` offers MIT/Apache in
addition to LGPL. A release still needs license-file collection and policy-tool confirmation.

A source-token scan found `unsafe` in many transitive crates, including platform entropy,
SIMD/CPU detection, containers and decoders. Therefore “pure Rust” is not “no unsafe.” Notable
direct candidate surfaces with unsafe tokens include the RustCrypto stack, `html5ever`,
`lopdf` transitives, `png` transitives, `quick-xml`, `tar`, `zip` and `zune-jpeg`. Exact block
review and fuzz evidence remain required before admission. The experiment proves resolution,
licensing feasibility and absence of native links, not memory safety.

## Compatibility observations

- `lopdf 0.45.0` resolves AES/MD5, Brotli, DEFLATE, text-encoding and SIMD-related transitives
  even with default features disabled. Its worker therefore has a materially larger audit
  surface than the R2 structured-text adapters.
- `zip 8.3.0` is pinned rather than the 9.0 prerelease and has every default compressor and
  encryption feature disabled; only Rust DEFLATE is selected.
- `zune-jpeg 0.5.15` disables x86/NEON defaults. `png 0.18.1` resolves Rust DEFLATE paths.
- Core storage will not depend on parser crates. Worker crates will split by format so an
  operator does not load the entire candidate set for one parse.

## Policy-check result

`cargo-deny 0.20.2` was installed under `/tmp` and run against all three experiment lockfiles with
`deny.toml`, the cached RustSec database and `--frozen`. Advisories, licenses and sources pass
with zero errors. The dependency candidate graph has one deliberate warning: `miniz_oxide`
0.8.9 through `png` and 0.9.1 through current `flate2`; this duplicate is recorded rather than
silently allowed. The fixture-generator graph passes all four checks without warnings. The
content materializer initially failed policy on an unnecessary IJG-licensed JPEG encoder; the
encoder was removed and the byte fixture pinned directly. Its final graph passes with only the
same recorded `miniz_oxide` duplication.

## Remaining T-04 evidence

Materialize and hash every generated fixture recipe, run every adapter under the supervisor
limits, and review the exact unsafe blocks reachable on the supported target. The policy tool
must be rerun against final split worker lockfiles. Until then T-04 remains unchecked.
