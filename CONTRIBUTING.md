# Contributing

Thanks for your interest in improving LGTM.

## Getting started

```sh
bun install
bun run tauri dev    # runs the desktop app (requires the Rust toolchain)
```

See the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for
platform system dependencies.

## Before opening a pull request

Run the same checks CI runs:

```sh
bun run check        # lint + format (Ultracite / Biome)
bun run compile      # typecheck
bun run test         # unit tests (Vitest)
bun run build        # production frontend build
```

And for Rust changes, in `src-tauri/`:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

`bun run fix` auto-fixes most lint/format issues. A pre-commit hook runs the
formatter on staged files automatically.

## Coding standards

Code style and conventions are enforced by [Ultracite](https://ultracite.ai)
(a Biome preset). See [`AGENTS.md`](./AGENTS.md) for the full guide. Formatting
is not a matter of opinion here — let the tooling handle it.

## Reporting issues

Use the issue templates for bugs and feature requests. For security issues, see
[`SECURITY.md`](./SECURITY.md).
