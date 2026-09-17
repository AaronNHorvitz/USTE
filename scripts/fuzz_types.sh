#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
fuzz_seconds=${USTE_FUZZ_SECONDS:-60}
fuzz_seed=${USTE_FUZZ_SEED:-1592639215}

cd -- "$repo_root"

for target in decode_v1 structured_v1
do
  cargo +nightly-2026-08-01 fuzz run "$target" -- \
    "-max_total_time=$fuzz_seconds" \
    "-seed=$fuzz_seed" \
    -max_len=4096 \
    -rss_limit_mb=1024 \
    -print_final_stats=1
done
