# LGTM desktop UI inventory — 2026-09-01

This is the visual baseline of the current `lgtm-desktop` app before comparing it with ChatGPT's Codex UI, UX, and developer experience.

## Capture context

- Built and launched `lgtm-desktop` from commit `d77cdab`.
- The worktree already contained uncommitted desktop-app changes. The screenshots represent that exact working state, not a clean checkout.
- Captured the native macOS app window directly. Most images are 1312 × 912 pixels; the initial Home capture is 1268 × 868 pixels because the window had not yet been resized.
- Runtime data visible during the pass: 2 projects, 2 sessions, 3 tasks, and 1 connected runner.
- No tasks were created, retried, approved, rejected, imported, or otherwise mutated. No settings were changed.
- The runner join command was deliberately not captured because it contains an authentication token.

## Global surfaces

- [03 — Home and new-session composer](./03-home.png)
- [04 — Batches, empty state](./04-batches.png)
- [05 — Workspace activity](./05-activity.png)
- [42 — Command palette](./42-command-palette.png)
- [43 — Command palette filtered to Settings](./43-command-palette-filtered.png)
- [50 — Connected runner popover](./50-runner-popover.png)
- [60 — Session with the sidebar collapsed](./60-sidebar-collapsed-session.png)

## `lgtm` project

- [06 — Overview](./06-project-lgtm-overview.png)
- [07 — Tasks](./07-project-lgtm-tasks.png)
- [08 — Goals, empty state](./08-project-lgtm-goals.png)
- [09 — Plans, empty state](./09-project-lgtm-plans.png)
- [10 — Memories, empty state and input](./10-project-lgtm-memories.png)
- [11 — TODOs](./11-project-lgtm-todos.png)
- [12 — History](./12-project-lgtm-history.png)
- [13 — Runners](./13-project-lgtm-runners.png)
- [14 — Session](./14-session-lgtm.png)

## Rejected task

- [15 — Overview](./15-task-rejected-overview.png)
- [16 — Activity](./16-task-rejected-activity.png)
- [17 — Changes](./17-task-rejected-changes.png)
- [18 — Review](./18-task-rejected-review.png)
- [19 — Notes](./19-task-rejected-notes.png)
- [20 — Terminal](./20-task-rejected-terminal.png)

## `useful-backend` project

- [21 — Overview](./21-project-useful-backend-overview.png)
- [22 — Tasks](./22-project-useful-backend-tasks.png)
- [23 — Goals, empty state](./23-project-useful-backend-goals.png)
- [24 — Plans, empty state](./24-project-useful-backend-plans.png)
- [25 — Memories, empty state and input](./25-project-useful-backend-memories.png)
- [26 — TODOs](./26-project-useful-backend-todos.png)
- [27 — History](./27-project-useful-backend-history.png)
- [28 — Runners](./28-project-useful-backend-runners.png)
- [29 — Session](./29-session-useful-backend.png)

## Approved task

- [30 — Overview](./30-task-approved-overview.png)
- [31 — Activity](./31-task-approved-activity.png)
- [32 — Changes, upper diff](./32-task-approved-changes.png)
- [61 — Changes, lower diff](./61-task-approved-changes-lower.png)
- [33 — Review](./33-task-approved-review.png)
- [34 — Notes](./34-task-approved-notes.png)
- [35 — Terminal](./35-task-approved-terminal.png)

## Failed task

- [36 — Overview](./36-task-failed-overview.png)
- [37 — Activity and failure detail](./37-task-failed-activity.png)
- [38 — Changes](./38-task-failed-changes.png)
- [39 — Review and retry action](./39-task-failed-review.png)
- [40 — Notes](./40-task-failed-notes.png)
- [41 — Terminal](./41-task-failed-terminal.png)

## Settings

- [44 — General](./44-settings-general.png)
- [45 — Orchestrator](./45-settings-orchestrator.png)
- [47 — Models](./47-settings-models.png)
- [48 — Executor model menu](./48-settings-models-executor-menu.png)
- [49 — Orchestrate model menu](./49-settings-models-orchestrate-menu.png)

## Import and composer states

- [51 — Import from GitHub](./51-import-github.png)
- [52 — Import from Linear](./52-import-linear.png)
- [53 — Composer project menu](./53-composer-project-menu.png)
- [54 — Composer options menu](./54-composer-plus-menu.png)
- [55 — Base-branch selector](./55-composer-base-branch.png)
- [56 — Plan mode selected](./56-composer-plan-chip.png)
- [57 — Runner selector](./57-composer-runner-menu.png)
- [58 — Add-repository field](./58-composer-add-repository.png)
- [59 — Session project-context popover](./59-session-project-popover.png)

## States not present in this runtime

The app and attached product picture describe more states than the current local data can demonstrate. This baseline does not invent records merely to make the inventory look complete. A later seeded-state pass should cover:

- populated Batches, Goals, and Plans, including dependency/DAG presentation;
- a plan-type task and its Plan pane;
- queued, running, awaiting-review, conflicted, blocked, cancelled, and retrying tasks;
- live terminal output and permission-request UX;
- validation checks, CI results, review findings, comments, and merge actions;
- task artefacts and richer Memories content;
- disconnected, busy, multi-runner, and remote-runner states;
- GitHub and Linear import results and error states.

## Baseline-only observations

These are factual capture notes for the later comparison, not design recommendations:

- The empty Batches surface gives its header region much more vertical space than the other top-level pages.
- Project Overview statistics appear workspace-wide while Recent activity is project-specific.
- Review is empty for the approved and rejected examples; the failed example primarily exposes Retry.
- Completed tasks' Terminal panes still display “Waiting for the shell…”.
- The Orchestrate model menu reaches the lower edge of its Settings modal.
- With the sidebar collapsed, the session title and window controls compete for the same title-bar space.

The `_debug` directory contains setup and rejected captures and is not part of this inventory.
