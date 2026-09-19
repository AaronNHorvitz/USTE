# Decision 0128 — Canonical ordered content commitments

Date: 2026-09-19

Status: accepted and locally verified T-20 logical primitive; disk integration remains open.

The frozen index-v1 carrier rewrites each nonempty family, and graph-state-v1 hashes a complete
canonical state stream. Neither cost is removed by bounded recovery metadata or certificate
windows. Introduce a separately named logical commitment primitive for a future versioned
copy-on-write index. Do not reinterpret any v1 digest, manifest, page, reducer, journal outcome or
M1 handoff. This decision does not yet specify or enable a new persistent carrier or graph profile.

The logical structure is a canonical binary Patricia tree over nonempty byte keys in lexicographic
order. Encode each byte conceptually as `1 || eight most-significant-first bits`, followed by a
zero terminator. A branch uses the first differing bit of its descendants; unary nodes are
collapsed. This shape depends on key contents, not insertion order, balancing, page boundaries,
object IDs, encryption randomness or file placement. Values contribute their length and a SHA-256
content digest. SHA-256 is provided by the already-approved RustCrypto dependency, not implemented
by USTE. These commitments do not replace authenticated encryption or authorize a consumer.

All hash input integers are unsigned big endian. The node prefix is the literal
`USTE-ORDERED-COMMITMENT-V1\0`, database ID (16), namespace ID (16), caller's distinct index profile
(32), nonzero family (1), then kind (1): zero for empty, one for leaf, two for branch. Empty has no
remaining bytes. A leaf appends key length (u32), key, value length (u64), value digest (32).
The value digest is SHA-256 of `USTE-ORDERED-COMMITMENT-VALUE-V1\0`, value length (u64), value.
A branch appends its bit position (u32), then left and right summaries, each entry count (u64),
logical key-plus-value bytes (u64), digest (32). Counts and byte sums are checked, not inferred
from untrusted allocations. Empty children are prohibited in branches.

The primitive admits keys through 4 KiB, values through 16 MiB, and at most one billion entries.
Proof paths are borrowed and explicitly bounded by branch count and aggregate input bytes;
there is no proof-count-sized allocation in verification or transition. Context-separated roots
are logical commitments, not independently admitted canonical state: their trusted expected value
must ultimately come from separately specified domain/journal admission. A self-consistent tree
or caller-supplied hash is never commit authority.

An exact lookup proof has one leaf (or an empty root) and root-to-leaf sibling summaries with
strictly increasing branch bits. Verify routing, leaf/key bounds, root counts/bytes/digest and
context before releasing membership or applying a compare-and-swap delta. Exact before-values
are compared by bounded length/content hash. Inserts split at the first differing key bit;
replacements preserve shape; deletes collapse the now-unary parent. Rebuild only the supplied
path commitment. A canonical admitted base remains canonical under these operations.

Required evidence includes an independent sorted-key reconstruction and independently calculated
hash goldens, exhaustive small insertion/deletion orders, mixed-prefix and zero/0xff keys,
generated replacement histories, proof mutation/context substitution, exact/minus-one work
bounds, value/key boundaries and atomic conflict/error behavior. No disk scalability, cold
admission, durability, production cryptographic review or BM-01/BM-06 qualification follows from
passing these primitive tests. Encrypted copy-on-write pages, publication/recovery, scoped bounded
queries, versioned graph/coordinator integration and complete measurement remain subsequent work.

Reproduce the independent standard-library hash vectors with
`python3 scripts/check_ordered_commitment_vectors.py`; the Rust primitive pins the same four
results without calling or importing the Python implementation.
