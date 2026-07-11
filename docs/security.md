# LGTM security notes

## Trust model

LGTM is a local-first desktop application. Everything it stores — repository
metadata, review sessions, comments, memory examples, suggestion feedback —
stays on the device in a SQLite database in the app data directory. No
network requests are made in the current milestones.

### Trusted

- The user and their actions in the UI.
- The installed `git` executable found on `PATH`.

### Untrusted input

Treated as untrusted everywhere:

- Repository contents, file names and paths appearing in diffs.
- Git command output (diffs, branch names, remote URLs).
- Anything typed into comment bodies (rendered as plain text, never as HTML).

## Boundary: webview → Rust

The frontend can only call two narrowly scoped Tauri commands:

- `open_repository(path)` — canonicalises the path in Rust, requires it to be
  inside a Git work tree, and returns repository metadata.
- `get_diff(repo_path, source)` — re-validates that `repo_path` is exactly a
  canonical repository root before running any diff.

There is no generic shell or filesystem command exposed to the webview.
Tauri capabilities are limited to `core:default`, `opener:default`,
`dialog:default` and the SQL plugin's load/select/execute for the app's own
database.

## Boundary: Rust → git

All git invocations go through one runner (`src-tauri/src/git/exec.rs`):

- Arguments are passed as an argv array — never through a shell, never
  concatenated into a command string.
- User-supplied ref names are validated (`validate_ref_name`: no leading `-`,
  no whitespace/control characters, no `..` or rev-syntax metacharacters) and
  then resolved with `rev-parse --verify --end-of-options` before use.
- Environment hardening: `GIT_TERMINAL_PROMPT=0` (no interactive prompts),
  `GIT_PAGER=cat`, `GIT_OPTIONAL_LOCKS=0`, and `GIT_EXTERNAL_DIFF`/`GIT_DIR`/
  `GIT_WORK_TREE`/`GIT_INDEX_FILE` removed. Diffs additionally pass
  `--no-ext-diff`.
- Every process has a 30-second timeout (killed on expiry) and a 10 MiB
  stdout cap (`diff-too-large` error rather than unbounded memory use);
  stderr is capped at 64 KiB.
- The app is read-only towards repositories: only `rev-parse`,
  `symbolic-ref`, `remote get-url`, `for-each-ref`, `ls-files`, `merge-base`
  and `diff` are ever invoked. Nothing stages, commits, or mutates config.

## Rendering

- Diffs are rendered by `@pierre/diffs` (Shiki-based tokenisation); file
  contents are never injected as raw HTML by our code.
- Comment bodies and remembered suggestions are rendered as plain text
  (`white-space: pre-wrap`), not as Markdown-derived HTML.

## Errors and logging

- Errors crossing the Rust boundary are structured
  (`{ code, message, details? }`); `details` carries trimmed git stderr and
  paths, never environment contents.
- No telemetry, no analytics, no network logging.

## Boundary: Rust → GitHub (Milestone 4)

- The personal access token lives only in the OS keychain (`keyring`:
  Keychain / Credential Manager / Secret Service) under
  `com.arsenstorm.lgtm` / `github-token`. It is never written to SQLite,
  local storage, logs, or error payloads; in Rust it is wrapped in a newtype
  whose `Debug` prints `[redacted]`.
- All GitHub requests happen in Rust over HTTPS (`api.github.com` only),
  with a 30-second timeout and a 10 MiB response cap. Owner/repository names
  are validated (`[A-Za-z0-9_.-]`, no `.`/`..`) and pull numbers are numeric
  before any URL is built; PR URLs must be `https://github.com/...`.
- Review submission re-fetches the pull request and compares the head SHA to
  the one the review was written against; a mismatch aborts with
  `pull-request-revision-changed` instead of posting against unseen code.
  Submission happens exactly once per confirmation — no automatic retry.
- Errors map to structured codes (`authentication-failed`,
  `github-rate-limited`, `github-permission-denied`,
  `pull-request-not-found`, `pull-request-revision-changed`,
  `network-failed`); GitHub response bodies included in details are trimmed
  and never contain the token.
- Imported review comments are stored locally (`imported_github_comments`)
  and pass the same quality gates as local comments before becoming memory
  examples.

## Future milestones (not yet implemented)

- Model-assisted suggestions (M5) must be opt-in, show exactly what code
  would be sent, and send the smallest necessary window.
