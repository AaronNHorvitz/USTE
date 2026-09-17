# T-09 canonical types and format evidence

Date: 2026-09-17 · Product toolchain: Rust/Cargo 1.95.0 · Target: x86_64-unknown-linux-gnu

This record closes T-09's bounded type, identity and canonical serialization scope. It does not
claim transaction, storage, encryption, parser, database-executable or release readiness.
[Decision 0012](../decisions/0012-canonical-wire-profile.md) owns the accepted bytes and limits.

## Implemented contract

- `uste-types` is safe Rust with `std` only and has no product dependency, build script, native
  link or ambient filesystem/clock/random/network behavior.
- Opaque 128-bit database, namespace, record, transaction, source-event and idempotency types
  have distinct binary tags and exact lowercase external text. Durable record, transaction and
  source-event references carry database and namespace scope.
- `CommitRevision` reserves zero and rejects successor wraparound. `UtcInstant` admits only the
  selected years-0001-through-9999 POSIX range and nanoseconds below one billion.
- Format 1.0 frames use strict version dispatch and minimal ULEB128. Signed values preserve their
  domain, reject negative zero and cover both `i128` endpoints. Maps sort exact UTF-8 bytes and
  reject duplicates without Unicode normalization.
- Fixed payload, inline-byte, collection, depth and aggregate 262,144-node limits are enforced.
  Decoder allocations are fallible and collection counts are checked against remaining bytes and
  the aggregate node budget before reservation. The amplification regression encodes five legal
  maximum-size child lists inside a roughly 320 KiB payload and rejects it at the node budget.
- `acceptance/r1/canonical-v1.tsv` contains independent literal bytes for every assigned value
  tag, integer endpoints, UTC endpoints and scoped record-reference identity layout. Every byte
  prefix truncation and appended trailing byte is rejected.

## Verification

~~~text
cargo test -p uste-types --all-targets
# 7 unit + 12 integration tests passed

cargo clippy -p uste-types --all-targets -- -D warnings
# passed

toolbox: Fedora 44 with gcc-c++ 16.2.1-2.fc44
cargo-fuzz 0.13.2; libfuzzer-sys 0.4.13; nightly-2026-08-01

cargo +nightly-2026-08-01 fuzz run decode_v1 -- \
  -max_total_time=60 -seed=1592639215 -max_len=4096 \
  -rss_limit_mb=1024 -print_final_stats=1
# 14,518,800 executions in 61 seconds; peak RSS 513 MiB; no crash or artifact

cargo +nightly-2026-08-01 fuzz run structured_v1 -- \
  -max_total_time=60 -seed=1592639215 -max_len=4096 \
  -rss_limit_mb=1024 -print_final_stats=1
# 1,610,094 executions in 61 seconds; peak RSS 554 MiB; no crash or artifact
~~~

The malformed-input target passes arbitrary bytes directly to the decoder and also derives short
well-framed payload candidates; every accepted frame must re-encode byte-identically. The
structured target derives all value families, nested lists/maps and scoped references from fuzz
bytes, then checks exact length, encode and decode equality. The ordinary integration suite also
runs a fixed 50,000 malformed-input campaign, golden mutations and 10,000 generated values so the
core invariant remains covered without nightly or native test tooling.

`fuzz/Cargo.lock` pins the isolated test graph. The NCSA-licensed C++ libFuzzer runtime is admitted
only for fuzz instrumentation and is checked by the repository supply-chain policy; it is excluded
from the root workspace and is not a product/runtime dependency. No crash regression was produced
by these campaigns. Future crashes belong under a committed minimal regression fixture, while
coverage corpora and build artifacts remain ignored.
