# Parser-worker sandbox feasibility

Date: 2026-09-16 · Fedora 44 reference runner · bubblewrap 0.11.0

This R0 probe demonstrates that the selected strict worker boundary is available without
`sudo` or changing host security settings. It is not the T-23 supervisor, a sandbox escape
audit or proof that every kernel/filesystem combination behaves identically.

## Probe

`experiments/worker-sandbox.sh` applies address-space, CPU-time and descriptor rlimits before
starting bubblewrap. Bubblewrap creates user, PID, IPC, UTS, cgroup-if-available and network
namespaces, a new session, and a kill-on-parent-death relationship. The worker sees read-only
`/usr`, one explicit read-only source file, `/proc`, minimal `/dev`, an empty working directory
and private tmpfs scratch. Environment is cleared except a fixed `PATH`.

Inside the worker the probe verifies:

- the granted source is readable and private scratch is writable;
- `/etc/passwd`, `/var/home` and `/run/user` are absent;
- `/usr` cannot be modified;
- an IPv4 TCP connection fails in the unshared network namespace;
- address space is 256 MiB, CPU time 5 seconds and open descriptors 64.

~~~text
$ bash experiments/worker-sandbox.sh
sandbox_probe=ok input_bytes=2725
~~~

The first sandboxed attempt failed because `RLIMIT_NPROC=32` counts every process owned by the
host UID, not just the worker tree. It was removed rather than risking unrelated user processes.
Production descendant count/RSS enforcement therefore needs a delegated cgroup or an owning
supervisor that observes the PID namespace and terminates the complete tree. The current test
environment has no accessible user systemd bus, so cgroup delegation is not claimed.

## Remaining controls

T-23 must implement lease revalidation, wall timeout, stdout/schema/output caps, cancellation,
descendant cleanup, stable inherited descriptors and a maintained syscall filter. It must test
fork attempts, signal races and a worker that deliberately holds descendants open. The Linux
profile refuses parsing when required isolation cannot be established; it does not silently run
the parser in-process. This probe closes feasibility for mount/network/user/PID isolation and
basic rlimits only.
