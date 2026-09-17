#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd -- "$repo_root"

cargo fetch --locked
for manifest in \
  fuzz/Cargo.toml \
  experiments/dependency-audit/Cargo.toml \
  experiments/fixture-generator/Cargo.toml \
  experiments/content-fixtures/Cargo.toml
do
  cargo fetch --manifest-path "$manifest" --locked
done
