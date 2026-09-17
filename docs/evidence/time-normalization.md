# T-45 pinned time normalization evidence

Date: 2026-09-17 · local correctness evidence, not independent security certification

## Implemented result

- Added `uste-time`, leaving `uste-types` std-only. The verified normalizer has no clock,
  filesystem, environment or network capability and resolves names only from embedded TZDB 2026c.
- Added strict RFC 3339, explicit-unit numeric and local civil normalization; Euclidean negative
  epoch handling; explicit fold/gap/conflict results; unsupported leap/non-POSIX retention; and
  full-range canonical UTC plus explicit-zone local presentation.
- Added a bounded provenance envelope with source artifact/version/locator, exact token,
  interpretation, status/reason, accepted pair, precision, uncertainty, assumptions and profiles.
  Its strict canonical `Value` schema rejects inconsistent and unknown required state.
- Added replay integration by storing the complete envelope as a graph property. Cold replay uses
  canonical transaction bytes, restores the exact accepted pair and performs no timestamp parse or
  zone resolution. The standalone codec test also replaces original text with an invalid token and
  proves decode still returns the authenticated recorded pair rather than reinterpreting it.
- Added a transaction/restart test proving equal wall samples and rollback yield revisions 1, 2
  and 3 without merging or reordering.

## Pinned profiles and boundaries

| Item | Value |
|---|---|
| Normalization | `posix-utc-v1` |
| Parser | `strict-rfc3339-v1` |
| Envelope | `uste-time-envelope-v1` over canonical generic `Value` |
| Local presentation | `uste-local-presentation-v1` |
| Rule engine | `jiff 0.2.37`, defaults off, `std` only |
| Embedded rules | `jiff-tzdb 0.1.8`, IANA `2026c` |
| TZDB SHA-256 | `8e18c4fd3aad2c58d45340e356a02229ce6811c83427c5b086ad4468b7582209` |
| Envelope golden | 849 bytes; `52920090900e58df57da9fb816a032b76c3a9d6c227f94a9c9605790923f5cc9` |

The direct feature tree contains `jiff-core`, `jiff-tzdb`, `sha2` and `uste-types`; it enables no
Jiff system timezone, filesystem zoneinfo, network, Serde or platform bundle feature. Jiff is
pure Rust but contains an internal compact tagged timezone representation with reviewed unsafe
pointer/`Arc` operations; first-party `uste-time` forbids unsafe. No C/C++ database, GIS or physics
engine is introduced.

## Focused verification

~~~text
cargo test -p uste-time --all-targets --locked
# 15 passed; 0 failed
cargo test -p uste-graph --test time_vector_intervals --locked
# 1 passed; 0 failed
cargo test -p uste-graph --test time_replay --locked
# 1 passed; 0 failed
cargo test -p uste-txn --test transaction_coordinator \
  equal_and_rolling_back_wall_samples_never_order_or_merge_commits --locked
# 1 passed; 0 failed
cargo clippy -p uste-time --all-targets --all-features --locked -- -D warnings
# passed
RUSTDOCFLAGS='-D warnings' cargo doc -p uste-time --all-features --no-deps --locked
# passed
bash scripts/check.sh
# passed: format, strict Clippy, 193 workspace tests, docs, task graph and experiment suites
/tmp/uste-t09-tools/bin/cargo-deny --locked check advisories licenses sources bans
# advisories ok, bans ok, licenses ok, sources ok
~~~

The time crate runs ten production point-normalization vectors from `acceptance/r0/time.tsv`; the
graph interval test runs the remaining two rows. The calendar test exhaustively round-trips every
day in years 0001–9999. Codec coverage checks every truncation, trailing bytes, unknown/missing
fields, profile/digest corruption, inconsistent status, field caps and both resolved/unresolved
round trips. Named-zone cases include the 2026 Chicago fold and gap, matching/conflicting offsets,
unknown zones, canonical-case presentation and UTC range endpoints.

An agent review found that the initial public unit struct could bypass profile verification.
Construction is now opaque and the only constructor verifies the exact version/digest before any
normalization or presentation; clones can exist only after that check. Follow-up review of the
corrected tree found no remaining high- or medium-severity findings. This review is local engineering
evidence, not independent certification.

## Limits and next owners

T-21 owns temporal indexes, valid/recorded-time query composition and correction knowledge views.
T-24 owns timestamp extraction from content-worker outputs. Precision does not imply accuracy;
normalization does not correct drift or prove source truth. No leap-second, TAI, GPS or smear
conversion table is admitted. T-62 remains unverified and blocks executable distribution only.
