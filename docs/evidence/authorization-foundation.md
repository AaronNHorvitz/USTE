# T-16 authorization and quota foundation evidence

Date: 2026-09-17 · scope: local policy/transaction/blob qualification, not production security
certification

## Implemented boundary

- `uste-policy` has no storage, crypto, filesystem, clock or network dependency. It provides
  private-construction, issuing-kernel-bound authenticated principals, default-deny namespace/record authorization,
  independent action bits, checked policy versions, narrowing record rules and bounded quotas.
- `uste-txn::AuthorizedCoordinator` derives persisted transaction identity from the authenticated
  principal. Consumer requests cannot provide a different principal. Raw coordinator methods are
  privileged internal capabilities and are not the consumer boundary.
- Current policy is checked before revision views, outcome indexes, upload/resume/write/finish/
  abort, blob scope/existence and commits. Transaction-ID lookup filters by its recovered owner;
  another principal and an unknown ID both return no outcome after authorization.
- Revision views and upload handles retain a versioned lease. Policy replacement invalidates their
  next access. Generic views expose no reducer snapshot, and upload internals cannot be dereferenced
  or extracted by the caller.
- Staging uses exact accepted plaintext bytes. A write reserves the complete offered slice, then
  reconciles the handle's actual accepted-byte delta even when storage reports an error. Finalize
  retains its reservation; accepted abort releases it; commit moves it to unique committed usage.
- Recovery rebuilds the unique committed-blob/first-principal map from authenticated group order and
  verified inventories. Reopening reconstructs identical namespace and principal committed bytes.

## Focused verification

~~~text
cargo test -p uste-policy --all-targets --locked
# 4 passed; 0 failed
cargo test -p uste-txn --test authorized_policy --locked
# 12 passed; 0 failed
cargo test -p uste-txn --all-targets --locked
# 24 passed; 0 failed (2 unit, 12 policy integration, 10 coordinator integration)
~~~

The policy tests prove empty-policy denial, independent permissions, record-level narrowing,
redacted principal/lease debug output, hard-cap rejection and stale-version invalidation. The
authorized coordinator tests prove:

- denied read/upload/outcome paths return `Unauthorized` without reaching the raw operation;
- a reducer-declared record denial returns before its stateful `prepare` method is called;
- a reducer-declared record target outside the coordinator namespace is denied before `prepare`,
  even when that principal also has permission in the foreign namespace;
- an authenticated principal minted by an identically configured foreign kernel is rejected;
- views and uploads presented to a different coordinator instance are rejected before raw access;
- an uncommitted finalized blob and the same committed blob are indistinguishable to a denied
  principal;
- five exact bytes succeed and byte six is rejected before storage mutation;
- a successful commit changes staged `5`/committed `0` to staged `0`/committed `5` and restart
  reconstructs the same principal and namespace charge;
- another authorized principal cannot discover a transaction outcome by transaction ID;
- two live handles meet the configured cap, the third is rejected, dropping a handle returns its
  live slot without releasing staged bytes, and accepted durable abort releases exact staging;
- a crash-after encrypted chunk sync returns an error while the full 1 MiB remains charged from
  the handle's observed accepted bytes; and
- unresolved zero-byte reservations stop at 32 and use a blob-ID index for inventory lookup;
- a resume-only grant cannot turn a fabricated same-scope token into an upload: every unknown token
  requires authenticated durable evidence, before and after restart;
- after restart, new starts fail closed, while an evidenced token recovers its authenticated 1 MiB
  charge and can be durably aborted;
- a recovered zero-byte final marker is accepted as evidence and can finish/commit; and
- replacing policy invalidates both a pinned view and an upload before new storage access.

The full repository check and final counts are recorded in `PROGRESS.md` after this increment's
review. The test adapters use deterministic keys and in-memory/fault storage; T-15 separately owns
production Linux adapter and large encrypted-object evidence.

## Bounded limitations

The policy is supplied by a trusted local adapter after restart. Because format 1.0 cannot enumerate
all abandoned reservations, recovered coordinators refuse new starts until T-35 adds complete
reconciliation; known tokens remain resumable. T-17 owns durable native policy
records and reducer-declared record requirements. It must rerun VT-06 against actual graph
adjacency, history and corrections; later query/content/simulation/lifecycle tasks own their real
search, count, branch, export, worker and subscription paths. This evidence establishes their
default-deny/versioned lease primitive, not those unimplemented features or an independent review.
