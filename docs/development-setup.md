# Fedora Kinoite development setup

This setup runs the Rust workspace foundation and current R0 experiments; it does **not** create
a database executable or claim a supported release. Commands are pinned to Fedora 44, Rust
1.95.0 and cargo-deny 0.20.2, matching the first reference runner. Run development tools inside
a Toolbox rather than layering packages onto the immutable host.

## Create the Toolbox

~~~bash
toolbox create --container uste-dev --image registry.fedoraproject.org/fedora-toolbox:44
toolbox enter uste-dev
sudo dnf install -y gcc git curl pkgconf-pkg-config
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.95.0
source "$HOME/.cargo/env"
rustup component add rustfmt
git clone git@github.com:AaronNHorvitz/USTE.git
cd USTE
git switch codex/uste-implementation
~~~

Review the rustup installer in environments where piping a network response to a shell is not
acceptable. USTE does not require or perform this installation itself. Core operation is
planned to remain offline; dependency acquisition is a development/build step.

Install the pinned policy checker into a repository-local ignored directory:

~~~bash
cargo install cargo-deny --version 0.20.2 --locked --root .tools
~~~

## Reproduce current evidence

~~~bash
bash scripts/check.sh
CARGO_DENY_BIN=.tools/bin/cargo-deny bash scripts/check_supply_chain.sh

# Individual R0 commands, when diagnosing a failure:
rustc --edition=2024 --test tests/r0_vectors.rs -o /tmp/uste-r0-vectors
/tmp/uste-r0-vectors
rustc --edition=2024 --test experiments/storage-publication.rs -o /tmp/uste-storage-publication
/tmp/uste-storage-publication
cargo build --manifest-path experiments/dependency-audit/Cargo.toml --locked
cargo test --manifest-path experiments/fixture-generator/Cargo.toml --locked
cargo test --manifest-path experiments/content-fixtures/Cargo.toml --locked
.tools/bin/cargo-deny --manifest-path experiments/dependency-audit/Cargo.toml \
  --config deny.toml --locked check all --show-stats
.tools/bin/cargo-deny --manifest-path experiments/fixture-generator/Cargo.toml \
  --config deny.toml --locked check all --show-stats
.tools/bin/cargo-deny --manifest-path experiments/content-fixtures/Cargo.toml \
  --config deny.toml --locked check all --show-stats
cargo fmt --manifest-path experiments/dependency-audit/Cargo.toml -- --check
cargo fmt --manifest-path experiments/fixture-generator/Cargo.toml -- --check
rustfmt --edition 2024 --check experiments/storage-publication.rs tests/r0_vectors.rs
bash experiments/worker-sandbox.sh
~~~

The candidate dependency graph currently emits one expected duplicate-version warning for
`miniz_oxide`; every advisory, license and source check must have zero errors.
The sandbox probe requires unprivileged user/network namespaces and bubblewrap 0.11.0; it
fails rather than falling back to an unsandboxed worker.

## Runnable synthetic example

The generator streams without retaining the corpus in memory. This three-record example has
a pinned logical digest:

~~~bash
SEED=8f41d0a52b40f13f4a77bc3beae2026a8bc42ad48d12ce53d92e29f612111001
cargo run --quiet --locked --offline \
  --manifest-path experiments/fixture-generator/Cargo.toml -- \
  digest graph 3 "$SEED"
# 03e40364c7454f76790af26062ab4094483bb5df6dfc0d0feac4f0b4c2c1f499

cargo run --quiet --locked --offline \
  --manifest-path experiments/fixture-generator/Cargo.toml -- \
  emit graph 3 "$SEED" > /tmp/uste-synthetic-graph.bin
wc -c /tmp/uste-synthetic-graph.bin
# 176 /tmp/uste-synthetic-graph.bin
~~~

The emitted stream is a benchmark input, not the production journal format. Do not commit
generated corpora, benchmark output, databases, keys or private source files.

List the byte-exact generated parser fixtures, or stream one to an isolated temporary path:

~~~bash
cargo run --quiet --locked --offline \
  --manifest-path experiments/content-fixtures/Cargo.toml -- list
cargo run --quiet --locked --offline \
  --manifest-path experiments/content-fixtures/Cargo.toml -- emit pdf_text \
  > /tmp/uste-synthetic.pdf
~~~

The list output must match `acceptance/r0/content-generated.tsv`. These are hostile/parser
fixtures and must never be opened with an active desktop preview or executed.
