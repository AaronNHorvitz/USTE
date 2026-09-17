# Decision 0012 — Canonical `uste-v1` wire profile

Date: 2026-09-17

Status: accepted for `uste-v1` format 1.0. Completes the byte-level choices left implicit by
[Decision 0003](0003-data-time-and-compatibility.md). A change to any assigned byte requires a
new format/compatibility decision and golden-vector migration evidence.

## Frame and version dispatch

A frame is `USTE || record-kind:u8 || major:u8 || minor:u8 || payload-length:ULEB128 || payload`.
Lengths and unsigned integers use minimal unsigned LEB128. A decoder rejects an unterminated,
overflowing or non-minimal integer, checks the record-kind cap and remaining input before any
payload allocation, and requires the frame to consume the entire input. Allocation failures are
reported as structured errors rather than panics.

Format 1.0 assigns record kind `0x01` to a canonical generic value. Other kinds, another major,
or a minor newer than the known schema are `UnsupportedVersion`; they are not guessed or decoded
as generic values. The generic-value payload cap is 16 MiB. Future record kinds each require a
fixed non-caller-raiseable cap.

Schema record fields, when introduced, encode `field-count`, followed by strictly increasing
`field-id`, `field-length`, and exact field bytes. All numeric components are minimal ULEB128.
Unknown or missing required fields fail. A field may be skipped only when the known enclosing
schema/version predeclares that exact field ID as optional; an on-wire optional flag cannot grant
that permission. No field-bearing record kind is assigned by this decision.

## Generic value tags

| Tag | Value | Payload after tag |
|---:|---|---|
| `00` | null | none |
| `01` | false | none |
| `02` | true | none |
| `03` | unsigned integer | minimal ULEB128 `u128` |
| `04` | signed integer | sign `00` nonnegative or `01` negative, then minimal magnitude ULEB128 |
| `05` | bytes | byte length, then exact bytes |
| `06` | UTF-8 string | byte length, then exact UTF-8 bytes |
| `07` | list | element count, then encoded values |
| `08` | map | entry count, then key-byte-length, UTF-8 key bytes, encoded value |
| `09` | POSIX UTC instant | signed seconds sign/magnitude, then nanoseconds ULEB128 |
| `0a` | scoped record reference | database ID, namespace ID, record ID |

Other tags are invalid in format 1.0. Signed negative zero, invalid sign bytes, magnitude above
the `i128` endpoint, floats, NaN and infinity are not encodable. Bytes and strings are each at
most 1 MiB. A list or map has at most 65,536 entries. Map keys are unique and strictly increasing
by unsigned UTF-8 bytes; Unicode is preserved without normalization. Root container depth is one,
scalar leaves do not add depth, and depth above 32 is rejected.

One generic value tree has at most 262,144 nodes, counting every scalar and container. This fixed,
non-caller-raiseable aggregate limit prevents small nested collection syntax from amplifying into
an unbounded owned object graph. Both construction/encoding and decoding enforce it. Decoding also
uses fallible allocation and rejects a container when its declared direct children cannot fit the
remaining aggregate node budget.

The UTC instant is the pair `(floor POSIX epoch seconds, nanoseconds 0..999_999_999)` within
Gregorian years 0001 through 9999: seconds `-62_135_596_800..=253_402_300_799`. T-09 owns only
this invariant-bearing pair and its ordering/encoding. T-45 owns RFC 3339 parsing, named zones,
fold/gap policy, source envelopes, display and replay integration.

## Identity bytes and external text

Every bare identity is a kind byte plus exactly 16 opaque bytes in binary contexts:

| Tag | Rust type | External prefix |
|---:|---|---|
| `01` | DatabaseId | `db_` |
| `02` | NamespaceId | `ns_` |
| `03` | RecordId | `rec_` |
| `04` | TransactionId | `txn_` |
| `05` | SourceEventId | `src_` |
| `06` | IdempotencyKey | `idem_` |

External text is the exact prefix plus 32 lowercase hexadecimal digits. Uppercase, whitespace,
hyphens, wrong prefixes and wrong lengths fail. All 128-bit patterns, including zero, are valid;
generation belongs to the later trusted randomness adapter. Bare IDs never imply ambient scope.
Durable record, transaction and source-event references include database and namespace IDs.
Idempotency scope additionally includes the authenticated principal at the transaction layer.
Committed revision zero denotes the pre-commit state and is not a commit identity; committed
revisions are `1..=u64::MAX`, and successor exhaustion is an error rather than wraparound.

## API and safety consequences

`uste-types` is safe Rust with `std` only: no Serde-defined canonical bytes, filesystem, clock,
randomness, crypto, parser, network or timezone dependency. Constructors enforce per-value
limits and private collection storage prevents unchecked mutation. Encoding computes and checks
the complete payload length and aggregate node budget before output allocation, stopping as soon
as either fixed cap is proven exceeded. Decoding borrows the input, checks every length
against both limits and remaining bytes, and never reserves from an unchecked count.

`acceptance/r1/canonical-v1.tsv` owns literal format-1.0 bytes. T-09 tests exact encodings,
endpoints, every truncation, version/tag/length failures, ordering, depth and deterministic
mutation-fuzz inputs. Accepted bytes must decode and re-encode identically; round-trip alone is
not evidence of canonicality.
