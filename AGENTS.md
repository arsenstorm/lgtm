# Working in this repository

LGTM is a Rust workspace. There is no JavaScript toolchain.

## Layout

- `crates/protocol` — wire types shared by every binary
- `crates/orchestrator` — task state, worker WebSocket, HTTP API, policy
- `crates/agent` — the worker: git worktrees, agent runs, checks, review
- `crates/client` — HTTP/WebSocket client for the orchestrator API
- `crates/github`, `crates/linear` — the two issue sources
- `crates/cli` — `lgtm`, the developer command
- `apps/desktop` — the GPUI desktop app

## Checks

Run all three before you call a change done:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Conventions

- One change per commit. Message: `type(scope): message`, one line, no body.
- No new dependency unless the standard library and what is already in
  `Cargo.toml` genuinely cannot do it.
- Comments explain a why — a constraint, a trade-off, a reason. Delete
  comments that narrate what the code does.
- Tests live next to the code they cover, in a `#[cfg(test)] mod tests` in
  the same file or a sibling `*_tests.rs`.
- Prefer deleting code to adding it. Keep functions small and flat.
