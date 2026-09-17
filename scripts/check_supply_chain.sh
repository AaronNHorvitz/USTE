#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
deny_bin=${CARGO_DENY_BIN:-cargo-deny}

cd -- "$repo_root"

for manifest in \
  Cargo.toml \
  experiments/dependency-audit/Cargo.toml \
  experiments/fixture-generator/Cargo.toml \
  experiments/content-fixtures/Cargo.toml
do
  "$deny_bin" --manifest-path "$manifest" --config deny.toml --locked \
    check all --show-stats
done

