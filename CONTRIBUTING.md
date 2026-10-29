# Contributing

Issues and pull requests are welcome.

Keep the native Rust CLI compact and compatible with the minimum Rust version in `Cargo.toml`. Commands should emit compact JSON by default, avoid exposing credentials, reject cross-origin redirects, and preserve the rule that analysis and inference happen in the connected agent.

Before opening a pull request, run:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
```

Changes to the distribution manifest or update protocol must remain compatible with previously published launchers. Security-sensitive changes should include focused tests.
