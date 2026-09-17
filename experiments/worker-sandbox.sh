#!/usr/bin/env bash
set -euo pipefail

# R0 feasibility probe only. Production supervision is implemented and tested at T-23.
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
source_fixture="$repo_root/acceptance/r0/content-fixtures.tsv"

command -v bwrap >/dev/null
command -v prlimit >/dev/null
test -r "$source_fixture"

# RLIMIT_NPROC is per host UID and would interfere with unrelated user processes; descendant
# count needs a delegated cgroup in the production supervisor. This probe bounds address space,
# CPU time and descriptors without changing host cgroup/security configuration.
prlimit --as=268435456 --cpu=5 --nofile=64 -- \
  bwrap \
    --unshare-user \
    --unshare-pid \
    --unshare-ipc \
    --unshare-uts \
    --unshare-cgroup-try \
    --unshare-net \
    --new-session \
    --die-with-parent \
    --ro-bind /usr /usr \
    --symlink usr/bin /bin \
    --symlink usr/lib /lib \
    --symlink usr/lib64 /lib64 \
    --proc /proc \
    --dev /dev \
    --dir /input \
    --ro-bind "$source_fixture" /input/source \
    --tmpfs /scratch \
    --dir /work \
    --chdir /work \
    --clearenv \
    --setenv PATH /usr/bin \
    /usr/bin/bash -c '
      set -euo pipefail
      test -r /input/source
      test ! -e /etc/passwd
      test ! -e /var/home
      test ! -e /run/user
      test "$(ulimit -v)" = 262144
      test "$(ulimit -t)" = 5
      test "$(ulimit -n)" = 64
      if touch /usr/uste-sandbox-escape 2>/dev/null; then
        exit 20
      fi
      printf "worker-output\n" > /scratch/result
      test "$(cat /scratch/result)" = worker-output
      if /usr/bin/bash -c "exec 9<>/dev/tcp/198.51.100.1/9" 2>/dev/null; then
        exit 21
      fi
      printf "sandbox_probe=ok input_bytes=%s\n" "$(wc -c < /input/source)"
    '
