# Contributing to Rhyven

Build with Rust 1.90 or later and a C compiler. Run commands from the repository
root. Changes to an app contract should include behavior cases; runtime changes
should exercise the relevant state, permission and transport boundaries.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --locked --bin rhyven
python3 qa/universal_market_check.py target/release/rhyven
python3 qa/shared_runtime_check.py target/release/rhyven
```

Python checks use temporary state. Docker integration checks require a compatible
Docker engine and can execute fixture code; read the script before running it.
Website checks use Playwright and axe; see [website/README.md](website/README.md).

Keep behavior generic: a conforming new app should not need a runtime source
change. MCP and REST must use the same validation and execution rules. Preserve
collection identity, permission review, revision guards and retry semantics.

Submit a focused pull request describing the problem, resulting behavior and
checks run. Explain compatibility or migration changes. Use synthetic fixtures;
never commit app databases, credentials, private keys or customer repositories.
Report security issues through [SECURITY.md](SECURITY.md), not a public issue.

Contributions are submitted under Apache-2.0. Preserve third-party notices.
Release/image publication is a maintainer action; pull-request workflows must not
receive publishing credentials or run with write permissions.
