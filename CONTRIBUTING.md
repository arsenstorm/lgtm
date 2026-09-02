# Contributing

Thanks for your interest in improving LGTM.

## Getting started

LGTM is a Rust workspace. You need a stable Rust toolchain (via `rustup`)
and `git` on `PATH`.

```sh
cargo run -p lgtm-cli --bin lgtm -- serve    # orchestrator plus a local runner
```

## Before opening a pull request

Run the same checks CI runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

`cargo fmt --all` fixes formatting.

## Coding standards

See [`AGENTS.md`](./AGENTS.md). In short: one change per commit, a
`type(scope): message` subject, no new dependency without a real need, and
comments that explain a why.

## Reporting issues

Use the issue templates for bugs and feature requests. For security issues,
see [`SECURITY.md`](./SECURITY.md).
