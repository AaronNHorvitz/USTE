#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
check_tmp=$(mktemp -d /tmp/uste-check.XXXXXX)
trap 'rm -rf -- "$check_tmp"' EXIT

cd -- "$repo_root"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked

cargo fmt --manifest-path experiments/dependency-audit/Cargo.toml -- --check
cargo fmt --manifest-path experiments/fixture-generator/Cargo.toml -- --check
cargo fmt --manifest-path experiments/content-fixtures/Cargo.toml -- --check
cargo fmt --manifest-path experiments/t20-bench/Cargo.toml -- --check
cargo fmt --manifest-path fuzz/Cargo.toml -- --check
cargo metadata --manifest-path fuzz/Cargo.toml --locked --offline --format-version 1 \
  > /dev/null
rustfmt --edition 2024 --check experiments/storage-publication.rs tests/r0_vectors.rs

python3 scripts/check_docs.py
python3 scripts/check_task_graph.py

rustc --edition=2024 --test tests/r0_vectors.rs -o "$check_tmp/r0-vectors"
"$check_tmp/r0-vectors"
rustc --edition=2024 --test experiments/storage-publication.rs \
  -o "$check_tmp/storage-publication"
"$check_tmp/storage-publication"

CARGO_TARGET_DIR="$check_tmp/dependency-target" \
  cargo build --manifest-path experiments/dependency-audit/Cargo.toml --locked --offline
CARGO_TARGET_DIR="$check_tmp/fixture-target" \
  cargo test --manifest-path experiments/fixture-generator/Cargo.toml --locked --offline
CARGO_TARGET_DIR="$check_tmp/content-target" \
  cargo test --manifest-path experiments/content-fixtures/Cargo.toml --locked --offline
CARGO_TARGET_DIR="$check_tmp/t20-bench-target" \
  cargo test --manifest-path experiments/t20-bench/Cargo.toml --locked --offline
CARGO_TARGET_DIR="$check_tmp/t20-bench-target" \
  cargo clippy --manifest-path experiments/t20-bench/Cargo.toml \
    --all-targets --locked --offline -- -D warnings
