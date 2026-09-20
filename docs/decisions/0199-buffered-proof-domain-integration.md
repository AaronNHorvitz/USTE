# Decision 0199: Optional writer and suffix proof buffers

Date: 2026-09-20

Status: Accepted and locally verified domain integration.

Expose `proof_cache_bytes: Option<usize>` separately in trusted packed graph writer preparation
and suffix recovery limits. `None` preserves existing behavior; all existing/default constructors
select it. A configured request/revision uses Decision 0198's fresh bounded proof cache, dropped
before delta staging. Canonical/semantic admission, staging and proof retention keep their own
budgets. Do not conflate these sequential cache lifetimes or reuse warmed pages across revisions.

Writer authorization, retry/collision ordering, content-free errors, certification and derived
repair remain unchanged. Suffix recovery retains exact authenticated source traversal and atomic
terminal triple installation. Aggregate successful preparation cache counters with checked
arithmetic separately from unchanged logical proof-work counters. No-op suffix/retry paths need
no preparation cache. Cache-size refusal occurs at preparation, after existing authorized/source
binding preflight; it is not an all-recovery pre-I/O admission guarantee.

Prototype Rust struct-literal callers must supply the new option. No memory-pilot adapter or
on-disk format change. Verify both default and buffered authorization/retry/revocation/fault
paths, suffix exact/corrupt/collision/fault recovery and counter conservation before native
selection or a performance claim. Keep all old fault counts and benchmark requirements.

Twelve new integration tests passed/37.47 s, followed by all 53 matching graph regressions/130.00 s
including original uncached variants. Strict workspace Clippy, warning-denying workspace docs
and strict standalone compatibility checks passed. Initial redundant test-helper qualifiers and
an incorrect new error-variant expectation were corrected as recorded in PROGRESS. The existing
writer distinguishes `Preparation(Storage(ResourceLimit))` from authorization failure; no
runtime behavior or assertion strength was weakened. These tests supplement, not impersonate,
the prior 710-test full workspace baseline.
