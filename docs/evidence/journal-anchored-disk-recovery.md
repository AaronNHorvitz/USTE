# Journal-anchored warm disk recovery evidence

Decision 0053 recovers `GraphDiskLiveState` ready at the authenticated frontier or pending across
exactly one journal revision without constructing complete graph maps.

## Verified behavior

- Complete journal authentication retains only the final bounded decoded group in an opaque,
  redacted token; the token cannot be publicly manufactured.
- Recovery proof loading decodes the exact token request and uses the semantically admitted
  predecessor root plus existing bounded current/history/reverse and delta preparation.
- Final open independently authenticates the journal, checks the base scope/certificate and exact
  captured principal/key/transaction/outcome/request/inventory/digest identity, and reruns reducer
  external-prepared validation before publishing pending state.
- Retry, transaction and blob-owner maps are rebuilt from authoritative groups rather than trusted
  from the discovery pass.
- Wrong prepared output and an intervening append fail without returning a coordinator.
- A recovered pending graph retains exact idempotent retry, rejects distinct progress, survives a
  bounded terminal-publication failure and installs the exact next base.
- A second restart admits the new frontier root and opens ready with no suffix, then prepares the
  next revision without complete `GraphState` replay.

~~~text
cargo test -p uste-txn --all-targets --locked
# 28 passed; 0 failed
cargo test -p uste-graph --all-targets --locked
# 43 passed; 0 failed
cargo clippy -p uste-txn -p uste-graph --all-targets --locked -- -D warnings
# passed
~~~

## Deliberate boundary

Only zero or one suffix is accepted. Coordinator maps remain complete in memory and journal
metadata is replayed from the origin. Decision 0059 subsequently makes root-manifest discovery
fixed-size and applies caller-selected cursor bounds before run pages are read; it does not change
the metadata boundary or serialize a partial semantic scan. If neither frontier nor predecessor
root admits, callers must use the explicit complete-state fallback. No BM-01/BM-06 or larger-than-
memory end-to-end result is claimed.
