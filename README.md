# LGTM

LGTM is an orchestrator for AI coding agents. You give it a prompt and a
repository; it runs an agent on a runner in a git worktree, streams the
output back, runs the repository's checks, reviews the diff, and then lets
you approve it, open a pull request, and merge. Runners can be this machine,
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
lgtm serve                          # orchestrator plus a local runner
lgtm run "add a HEALTH.md file"     # from any repo with an origin remote
```

`lgtm serve` generates a token, stores it at `~/.lgtm/token`, and prints a
join line. Paste that `lgtm runner ws://… --token …` line on another machine
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
  told; `lgtm memory approve <id>` accepts one an agent proposed.
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
- `lgtm terminal <id>` opens a shell in the task's worktree on its runner; it
  stays open when you detach. It is a shell of its own, not the agent's
  session, and it does not resize.
- `lgtm tasks`, `show`, `logs`, `diff`, and `runners` show what is going on.
- Policy in the repository can retry, fix failing checks, and auto-approve
  or auto-merge.
- The desktop app (`apps/desktop`) lists tasks and shows activity, a
  coloured diff, checks, and plans in a review pane.

## Orchestration

```sh
lgtm serve --orchestrate auto     # claude if this machine has it, else codex
lgtm serve --orchestrate claude   # or codex; off when the flag is absent
```

Each time a task under a goal ends, a model is given the LGTM tools over MCP
and takes a few steps toward the goal: it inspects the goal and the task that
ended, then creates dependent work, sends a follow-up, retries, approves, or
calls `wait` because a person is needed. It finishes with one paragraph for
the developer, which lands on the ended task's event log (`lgtm logs <id>`)
along with every step it took. Tasks that belong to no goal are never
touched.

| Tool | What it does |
| --- | --- |
| `goal_inspect` | The objective, the status, and one line per task. |
| `task_inspect` | One task's attempts, checks, findings and activity. |
| `task_create` | Add work the goal needs, optionally behind others. |
| `task_message` | Tell a task's agent to fix something itself. |
| `task_retry` | Requeue a task that crashed or timed out. |
| `task_approve` | Approve and push a task. |
| `runner_list` | The connected runners. |
| `wait` | Stop and leave the goal to a person. |

Every one of them goes through the same HTTP endpoint a person uses, so LGTM
validates each the same way — `task_approve` is refused unless the checks
passed and no blocking review finding is left, which a person's own approve
may waive. `wait` marks the goal for attention, which shows as blocked until
the next task or message under it.

A model can be pinned per kind of task, for the goals a plan should think
harder about than the work:

```sh
lgtm serve --model-for plan=opus --model-for run=sonnet
# or LGTM_MODELS="plan=opus,run=sonnet"
```

A task created without a model of its own gets the one for its kind. The
review model is a separate setting the runner does not honour yet.

## Agent tools

Every agent run gets LGTM's own context over MCP, so the harness reaches it
with tools instead of file conventions:

| Tool | What it does |
| --- | --- |
| `memories_list` | The facts recorded for this repository. |
| `memory_propose` | Propose a fact; it waits for a person's approval. |
| `todos_list` | The open todos for this repository. |
| `todo_create` | Note work the run spotted but did not do. |
| `scratchpad_read` | The working notes kept for this task. |
| `scratchpad_write` | Replace those notes. |

The runner registers the server (`lgtm mcp`, this same binary, over stdio)
with claude and codex for every run; there is nothing to configure.

An agent cannot write a memory directly: `memory_propose` files a memory
that waits, unapproved, until you run `lgtm memory approve <id>`; only then
does a later run get told it.

## Repository config

A repository can declare its checks and its policy in `.lgtm/config.toml`.
Runners read it from the worktree they just built.

```toml
[validation]
fmt = "cargo fmt --all --check"
clippy = "cargo clippy --workspace --all-targets -- -D warnings"
test = "cargo test --workspace"

[policy]
retry = 1          # extra agent runs after a crash
fix_checks = 2     # follow-up runs that try to fix failing checks
review = true      # review the finished diff with a second agent run
review_executor = "auto"  # auto picks the other harness when the runner has both
timeout_secs = 3600  # kill an agent run after this long
auto_approve = false
auto_merge = false
max_diff_lines = 300   # no auto-approve for a diff bigger than this
protected_files = ["migrations/*", "Cargo.lock"]  # never auto-approved
budget_per_task_usd = 2.0  # no auto-approve for a run that cost more
reassign = 1  # move a lost or failed task to another runner this many times
budget_daily_usd = 50.0

[sandbox]
profile = "standard"   # standard: stripped env, writes only to the worktree, secrets unreadable
network = "unrestricted"  # unrestricted, none, or allowlist
allowed_hosts = ["github.com", "api.anthropic.com", "crates.io"]  # what allowlist may reach
memory_mb = 4096       # address space one run may map; unset means no limit
processes = 256        # processes and threads the run's user may have; unset means no limit
cpu_seconds = 3600     # CPU time before the run is killed; unset means no limit
```

`standard` runs the agent with only the variables it needs — a token in the
runner's shell never reaches it — and confines its writes to the worktree, the
repository mirror, the tool caches, and the temporary directories. macOS uses
`sandbox-exec`, Linux uses `bubblewrap`; where `bwrap` is missing, and on other
systems, the environment allowlist is all that applies and the runner says so.
`HOME` stays the real home, so claude and codex keep their own config and
login; the host's secrets (`.ssh`, `.aws`, `.gnupg`, `.config/gh`, `.netrc`,
`.docker/config.json`) are unreadable instead. The macOS login keychain stays
readable, because that is where claude keeps its own token and denying it only
logs the agent out. `strict` runs as `standard` until the container boundary
lands.

`memory_mb`, `processes` and `cpu_seconds` cap what one run may spend; each is
unset by default, and an unset one is no limit. On macOS and Linux the run
starts behind a shell that sets the matching `ulimit` inside the sandbox, so
the limits bind the agent and everything it spawns. A limit the kernel refuses
is skipped rather than fatal: Darwin does not enforce an address-space limit at
all, so `memory_mb` is a Linux limit in practice while `processes` and
`cpu_seconds` hold on both. On Linux, when this user has a delegated cgroup v2
tree, the run also gets a `lgtm-<pid>` cgroup with `memory.max` and `pids.max`,
removed when the run ends; without delegation the `ulimit`s are all there is.
On Windows nothing is enforced — a Job Object needs `windows-sys`, which is not
a dependency here — and the environment allowlist is all a run gets.

`network` decides where a run may go: `unrestricted` (the default), `none`, or
`allowlist`. Under `allowlist` the runner starts an HTTP proxy on the loopback
for the length of the run, points the agent at it, and refuses any host
`allowed_hosts` does not name — exactly, or as a suffix when the entry starts
with a dot (`.github.com` covers `api.github.com`). Naming no hosts means the
registries and APIs a build usually needs. Every refused host is reported on
the task as `network denied: <host>`, so a missing entry shows up as itself
rather than as a mystery failure. On macOS this is a real boundary: seatbelt
lets the run reach nothing but the proxy's port. On Linux it is the proxy
variables only until a network namespace lands, so a process that ignores them
is not stopped; `none` is enforced there today, with `--unshare-net`.

An agent that hits a refusal it wants lifted calls the `request_network` MCP
tool (or just tries the host and lets the automatic `network denied` stand as
the request); either way it lands as an event on the task. A person answers
with `lgtm allow <task> <host>`, which adds the host to that task's own
allowlist. Nothing pauses to wait for the answer — `claude -p` has no seam for
that mid-run — so the grant takes effect on the task's next run: a follow-up,
or a retry.

A malformed file changes nothing: unknown or ill-typed keys are logged and
the defaults stay.

## Workspace

- `crates/protocol` — wire types shared by every binary
- `crates/orchestrator` — task state, runner WebSocket, HTTP API, policy
- `crates/agent` — the runner: git worktrees, agent runs, checks, review
- `crates/client` — HTTP/WebSocket client for the orchestrator API
- `crates/diff` — patch parsing, unified/split layouts, and the file tree for the review pane
- `crates/github` — pull requests, issues, CI status
- `crates/linear` — Linear issues
- `crates/cli` — `lgtm`, the developer command
- `apps/desktop` — the GPUI desktop app

Checks: `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`.

## Remote and ephemeral runners

Runners connect out, so they can run anywhere: another machine, a
container, a spot instance. `docker/agent.Dockerfile` builds a runner image
(`lgtm-agent` plus the Claude Code CLI). See
[docs/remote-runners.md](docs/remote-runners.md) for TLS with a self-signed
certificate, `--ephemeral`/`--max-tasks` runners that clean themselves up,
having the orchestrator provision runners on demand, and running over
Tailscale with no TLS at all.

## Notifications

You should not have to watch LGTM. The desktop app raises an OS notification
when a task needs a person — ready for review, failed, timed out, runner
lost, merged, asks for a permission, conflicts with its base branch, or a
pull request review requests changes. Settings → Notifications turns it off;
it is on by default.

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

A runner that disconnects for good posts without a task, since it may have
been running several:

```json
{
  "runner": "box-1",
  "line": "runner box-1 disconnected"
}
```

Delivery is best effort: a webhook nobody answers is logged and dropped.

## Security

Every orchestrator API call and runner connection carries a shared token.
`lgtm serve --tls-cert/--tls-key` serves over TLS; see
[docs/remote-runners.md](docs/remote-runners.md). With a `GITHUB_TOKEN` set
on the orchestrator, runners push with a per-push token handed over for
that one push and need no GitHub credentials of their own — the agent
process never receives it — though the mirror clone at the start of a run
still uses the runner's own credentials.

## Docs

- [docs/remote-runners.md](docs/remote-runners.md) — TLS, ephemeral
  runners, provisioning, Tailscale
- [docs/release.md](docs/release.md) — cutting a release, installing,
  upgrading

LGTM used to be a Tauri desktop review app. That code is preserved at the
git tag `v0.1.0-tauri`.
