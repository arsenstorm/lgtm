# LGTM

LGTM is a local-first desktop code-review tool for Git repositories. It opens
a repository on disk, diffs the working tree or a branch against a base, and
lets you leave inline comments in a persisted review session — all without a
server, an account, or a network connection. Its distinctive feature is
deterministic reviewer memory: when you save a substantive inline comment,
LGTM stores a normalised fingerprint of the commented code, and when
materially similar code shows up in a later diff it proposes your previous
comment as a ghost suggestion you can accept, edit, dismiss, or permanently
disable. There is no LLM, no embeddings, no network call, and nothing is ever
published automatically.

## Features

- Open a local Git repository (path validated and canonicalised in Rust)
- Working-tree vs `HEAD` and branch vs merge-base diffs
- Split/unified diff rendering with syntax highlighting (`@pierre/diffs`)
- Light/dark theme
- Per-file viewed tracking
- Persisted review sessions with inline draft comments — single-line and
  multi-line, on either side of the diff
- Conservative re-anchoring of comments when the underlying diff changes,
  with outdated comments marked rather than silently moved or dropped
- Markdown export of a review
- Deterministic reviewer-memory suggestions (accept / edit / dismiss /
  disable), with feedback that adjusts future confidence
- Keyboard shortcuts and a command palette
- GitHub pull-request review: open a PR by URL, comment inline, and submit
  one grouped review (Comment / Approve / Request changes) — token stored in
  the OS keychain, all requests made from Rust
- Import of your past GitHub review comments into reviewer memory
  (explicit, scoped to one repository, cancellable, deduplicated)

## Requirements

- [Bun](https://bun.sh)
- A Rust toolchain (stable, via `rustup`)
- `git` on `PATH`
- macOS, Linux, or Windows, per [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/)

## Setup & run

```sh
bun install
bun run tauri dev
```

Build a release bundle:

```sh
bun run tauri build
```

## Development

```sh
bun run test           # vitest
cd src-tauri && cargo test
bun run check           # Ultracite/Biome lint check
bun run fix              # Ultracite/Biome autofix
bunx tsc --noEmit
```

## Architecture

```
src/
  app/            app shell, header/status bars, top-level state
  features/
    repositories/ repo picker, recent repos
    changes/      diff loading, review session, file-review (viewed) state
    diff/         diff viewer component
    reviews/      comments, composer, summary, Markdown export, shortcuts,
                  command palette
    memory/       suggestion cards, suggestion/memory hooks
  lib/
    tauri/        typed invoke wrappers around the two Tauri commands
    db/           SQLite access — repositories, sessions, comments, memory
                  examples, suggestions, file state, settings
    diff/         patch-line mapping, DiffAnchor construction, re-anchoring
    memory/       lexical normalisation, fingerprinting, similarity scoring,
                  candidate extraction/filtering
    errors/       structured app error type
  types/          shared Git/review types
src-tauri/src/
  commands/       thin #[tauri::command] wrappers (repository.rs, git.rs)
  git/            exec.rs (safe process runner), repository.rs, diff.rs
  error.rs        structured AppError serialized as {code, message, details}
```

Rust owns path validation and safe, read-only git execution (no shell,
per-process timeouts, output caps) and returns diff/metadata to the frontend.
React owns everything else — diff rendering, selection, review/comment state,
the memory engine, and all persistence, which goes through the `lib/db`
layer. SQLite (via `tauri-plugin-sql`) stores repository metadata, review
sessions, comments, memory examples, and suggestion feedback; the repository
on disk remains the source of truth for code.

## How reviewer memory works

When a comment is saved, it only becomes a memory example if it passes
quality gates: the code selection has enough tokens to fingerprint, the
comment body is long enough and not generic boilerplate ("nit", "lgtm",
"+1", ...), and the file isn't a lockfile, build output, or otherwise
excluded path. Surviving examples are lexically normalised (rename- and
literal-insensitive) and fingerprinted as trigrams, structural "shape",
identifiers, and surrounding context.

Candidates in later diffs are scored against stored fingerprints with a
weighted combination — trigram similarity (0.45), shape (0.2), context
(0.15), identifiers (0.1), and repository scope (0.1) — and only surfaced as
suggestions above a conservative threshold of 0.72. Suggestions are capped
at 3 per file and 10 per review. Accepting or dismissing a suggestion
records feedback that nudges that example's future confidence up or down;
memories marked "never suggest again" are permanently excluded.

## Keyboard shortcuts

| Key | Action |
| --- | --- |
| `j` / `k` | Next / previous file |
| `n` / `p` | Next / previous comment |
| `c` | Comment on selection |
| `v` | Toggle file viewed |
| `r` | Refresh diff |
| `s` | Open review summary |
| `Cmd/Ctrl+K` | Command palette |
| `Cmd/Ctrl+Enter` | Save comment (in composer) |
| `Esc` | Cancel comment (in composer) |

Shortcuts are ignored while typing in an editable field.

## Limitations

- Untracked files are listed in the file list but not diffed (avoids
  touching the index)
- Single repository open at a time, one window
- GitHub review submission re-checks the PR head SHA but does not rebase
  your drafts onto a changed head automatically (refresh re-anchors them)
- No Markdown preview — comment bodies render as plain text
- The lexical normaliser is language-agnostic; there's no Tree-sitter-based
  parsing yet
- No hunk-level keyboard navigation (only file- and comment-level)

## Security

LGTM never runs git through a shell, validates and re-validates repository
paths in Rust, and only ever calls read-only git subcommands with hardened
environment, timeouts, and output caps; nothing is sent over the network.
See [docs/security.md](docs/security.md) for the full trust model and
boundaries.

## Docs

- [docs/implementation-plan.md](docs/implementation-plan.md) — architecture
  decisions, milestone breakdown, and progress log
- [docs/security.md](docs/security.md) — trust model and security boundaries
