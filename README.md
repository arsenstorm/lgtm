# LGTM

LGTM is an orchestrator for AI coding agents. You give it a prompt and a
repository; it runs an agent on a worker in a git worktree, streams the
output back, runs the repository's checks, reviews the diff, and then lets
you approve it, open a pull request, and merge. Workers can be this machine,
another machine, or a container that exits when it is done. There is a CLI
and a native desktop app.

## Install

```sh
curl -fsSL https://lgtm.arsenstorm.com/install | bash        # macOS, Linux, Raspberry Pi
```

```powershell
powershell -c "irm lgtm.arsenstorm.com/install.ps1 | iex"
```

Both put `lgtm` in `~/.lgtm/bin`. Run `lgtm upgrade` to update it. From
source: `cargo install --path crates/cli`.

## Quick start

```sh
lgtm serve                          # orchestrator plus a local worker
lgtm run "add a HEALTH.md file"     # from any repo with an origin remote
```

`lgtm serve` generates a token, stores it at `~/.lgtm/token`, and prints a
join line. Paste that `lgtm worker ws://… --token …` line on another machine
to add it to the fleet.

## What you can do

- `lgtm run` a prompt, a GitHub issue (`--issue`), or a Linear issue
  (`--linear`); `--model` picks the harness's model, `lgtm plan` proposes
  dependent steps instead of a diff.
- `--agent codex` is a first-class executor alongside Claude: planning,
  follow-ups that resume the same thread, and the review pass all work.
- `lgtm backlog github` and `lgtm backlog linear` import a whole labelled
  backlog as one batch of tasks.
- `lgtm tell <id> "…"` sends a follow-up to a task that is awaiting review.
- `lgtm memory add "…"` records a fact every agent run in the repository is
  told.
- `lgtm todo add "…"` keeps a note; `lgtm todo promote <id>` turns it into a
  task.
- `lgtm pad <id>` shows the notes an agent kept in `.lgtm/scratchpad.md`; they
  follow the task to a retry.
- Repository checks run after the agent finishes; a second agent pass
  reviews the diff and reports findings.
- `lgtm approve`, `reject`, `cancel`, `retry`, and `merge` drive a task to a
  pull request and into `main`.
- Approving rebases the branch onto its base before pushing; a conflict shows
  as `conflicted` and `lgtm tell <id> "…"` has the agent resolve it. The
  repository's checks are not run again after a clean rebase.
- `lgtm tasks`, `show`, `logs`, `diff`, and `workers` show what is going on.
- Policy in the repository can retry, fix failing checks, and auto-approve
  or auto-merge.
- The desktop app (`apps/desktop`) lists tasks and shows activity, a
  coloured diff, checks, and plans in a review pane.

## Orchestration

```sh
lgtm serve --orchestrate claude   # or codex; off when the flag is absent
```

Each time a task under a goal ends, a model reads the goal, its tasks, and
what that task did, and decides one next step: approve it, retry it, send it
a follow-up, create a task the goal still needs, or wait for a person. LGTM
checks the decision before acting — it approves only what the checks and the
review already cleared, and it refuses a new task that depends on work
outside the goal. Every decision, refusal and failure is on the task's event
log (`lgtm logs <id>`). Tasks that belong to no goal are never touched.

## Agent tools

Every agent run gets LGTM's own context over MCP, so the harness reaches it
with tools instead of file conventions:

| Tool | What it does |
| --- | --- |
| `memories_list` | The facts recorded for this repository. |
| `memory_propose` | Propose a fact for the next run to be told. |
| `todos_list` | The open todos for this repository. |
| `todo_create` | Note work the run spotted but did not do. |
| `scratchpad_read` | The working notes kept for this task. |
| `scratchpad_write` | Replace those notes. |

The worker registers the server (`lgtm mcp`, this same binary, over stdio)
with claude and codex for every run; there is nothing to configure.

An agent cannot write a memory directly: `memory_propose` files a todo
titled `Proposed memory: …`. You read it, and `lgtm memory add "…"` makes it
a memory every later run is told.

## Repository config

A repository can declare its checks and its policy in `.lgtm/config.toml`.
Workers read it from the worktree they just built.

```toml
[validation]
fmt = "cargo fmt --all --check"
clippy = "cargo clippy --workspace --all-targets -- -D warnings"
test = "cargo test --workspace"

[policy]
retry = 1          # extra agent runs after a crash
fix_checks = 2     # follow-up runs that try to fix failing checks
review = true      # review the finished diff with a second agent run
review_executor = "auto"  # auto picks the other harness when the worker has both
timeout_secs = 3600  # kill an agent run after this long
auto_approve = false
auto_merge = false
max_diff_lines = 300   # no auto-approve for a diff bigger than this
protected_files = ["migrations/*", "Cargo.lock"]  # never auto-approved
budget_per_task_usd = 2.0  # no auto-approve for a run that cost more

[sandbox]
profile = "standard"   # standard: stripped env, writes only to the worktree, secrets unreadable
```

`standard` runs the agent with only the variables it needs — a token in the
worker's shell never reaches it — and confines its writes to the worktree, the
repository mirror, the tool caches, and the temporary directories. macOS uses
`sandbox-exec`, Linux uses `bubblewrap`; where `bwrap` is missing, and on other
systems, the environment allowlist is all that applies and the worker says so.
`HOME` stays the real home, so claude and codex keep their own config and
login; the host's secrets (`.ssh`, `.aws`, `.gnupg`, `.config/gh`, `.netrc`,
`.docker/config.json`) are unreadable instead. The macOS login keychain stays
readable, because that is where claude keeps its own token and denying it only
logs the agent out. `strict` runs as `standard` until the container boundary
lands.

A malformed file changes nothing: unknown or ill-typed keys are logged and
the defaults stay.

## Workspace

- `crates/protocol` — wire types shared by every binary
- `crates/orchestrator` — task state, worker WebSocket, HTTP API, policy
- `crates/agent` — the worker: git worktrees, agent runs, checks, review
- `crates/client` — HTTP/WebSocket client for the orchestrator API
- `crates/diff` — patch parsing, unified/split layouts, and the file tree for the review pane
- `crates/github` — pull requests, issues, CI status
- `crates/linear` — Linear issues
- `crates/cli` — `lgtm`, the developer command
- `apps/desktop` — the GPUI desktop app

Checks: `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`.

## Remote and ephemeral workers

Workers connect out, so they can run anywhere: another machine, a
container, a spot instance. `docker/agent.Dockerfile` builds a worker image
(`lgtm-agent` plus the Claude Code CLI). See
[docs/remote-workers.md](docs/remote-workers.md) for TLS with a self-signed
certificate, `--ephemeral`/`--max-tasks` workers that clean themselves up,
having the orchestrator provision workers on demand, and running over
Tailscale with no TLS at all.

## Notifications

You should not have to watch LGTM. The desktop app raises an OS notification
when a task needs a person — ready for review, failed, timed out, runner
lost, merged. Settings → Notifications turns it off; it is on by default.

`lgtm serve --webhook URL` (or `LGTM_WEBHOOK`) POSTs the same events, so
Slack, email, or anything else can hang off one URL:

```json
{
  "task_id": "0123abcd",
  "status": "awaiting_review",
  "repository": "https://github.com/you/repo.git",
  "line": "add a /health endpoint: ready for review"
}
```

Delivery is best effort: a webhook nobody answers is logged and dropped.

## Security

Every orchestrator API call and worker connection carries a shared token.
`lgtm serve --tls-cert/--tls-key` serves over TLS; see
[docs/remote-workers.md](docs/remote-workers.md). With a `GITHUB_TOKEN` set
on the orchestrator, workers push with a per-push token handed over for
that one push and need no GitHub credentials of their own — the agent
process never receives it — though the mirror clone at the start of a run
still uses the worker's own credentials.

## Docs

- [docs/remote-workers.md](docs/remote-workers.md) — TLS, ephemeral
  workers, provisioning, Tailscale
- [docs/release.md](docs/release.md) — cutting a release, installing,
  upgrading

LGTM used to be a Tauri desktop review app. That code is preserved at the
git tag `v0.1.0-tauri`.
