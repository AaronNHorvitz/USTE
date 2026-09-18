#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
if [[ $# -gt 1 ]]; then
  echo "usage: $0 [EMPTY_BTRFS_DIRECTORY]" >&2
  exit 2
fi

if [[ $# -eq 1 ]]; then
  demo_root=$1
  if [[ ! -d "$demo_root" ]] || [[ -n "$(find "$demo_root" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
    echo "demo root must be an existing empty directory" >&2
    exit 2
  fi
else
  mkdir -p "$repo_root/target/memory-pilot-demo-runs"
  demo_root=$(mktemp -d "$repo_root/target/memory-pilot-demo-runs/run.XXXXXX")
fi

echo "M1_DEMO_ROOT=$demo_root"
env CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 \
  cargo run --manifest-path "$repo_root/Cargo.toml" \
  -p uste-memory-adapter --bin uste-memory-demo --locked --offline -- "$demo_root"
