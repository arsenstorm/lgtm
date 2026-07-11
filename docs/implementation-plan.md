# LGTM implementation plan

## Observed repository state (2026-07-11)

- Tauri v2 scaffold (`src-tauri/`), single `greet` command, opener plugin only.
- React 19 + Vite 7 + TypeScript 5.8 (strict), Tailwind CSS v4, full shadcn/ui
  component set under `src/components/ui/`, Lucide-equivalent icons via
  `@remixicon/react`, `next-themes`, `sonner`, `react-resizable-panels`.
- Package manager: **bun**. Lint/format: **Ultracite** (Biome) via
  `bun run check` / `bun run fix`, husky pre-commit.
- No tests, no CI, no database, no diff renderer at scaffold time.
- Three pre-existing type errors in generated shadcn components
  (duplicate JSX attributes in `calendar.tsx`/`pagination.tsx`, wrong prop
  type in `spinner.tsx`) were fixed before feature work; `tsc` and
  `cargo check` now pass.

## Dependencies added

- `@pierre/diffs` 1.2.12 — diff renderer (inspected: React `FileDiff`,
  `PatchDiff`, `MultiFileDiff`, `Virtualizer`, `WorkerPoolContextProvider`;
  parsing via `parsePatchFiles`; selection via `enableLineSelection` /
  `onLineSelected(SelectedLineRange)`; annotations via `lineAnnotations` +
  `renderAnnotation`; themes via `themeType` and Shiki themes).
- `@tauri-apps/plugin-dialog` — native repository folder picker.
- `@tauri-apps/plugin-sql` (sqlite) — persistence; versioned migrations are
  registered in Rust, all SQL statements live in `src/lib/db/`.
- Dev: `vitest`, `happy-dom`, `@testing-library/react`,
  `@testing-library/user-event`, `@testing-library/jest-dom`.

## Architecture

Frontend (React) owns navigation, session/comment state, diff rendering,
selection → `DiffAnchor` conversion, reviewer-memory analysis and all
persistence access through `src/lib/db`. Rust owns path validation, safe git
execution (no shell, timeouts, output caps), and diff/metadata retrieval.

```
src/
  lib/tauri/    typed invoke wrappers (commands.ts, errors)
  lib/db/       database open + all SQL (repositories, sessions, comments,
                memory, suggestions, file state, settings)
  lib/diff/     patch helpers: line location, DiffAnchor build, re-anchoring
  lib/memory/   normalise, fingerprint, similarity, candidate filtering
  features/     repositories/, changes/, diff/, reviews/, memory/
  app/          app shell + top-level state
src-tauri/src/
  error.rs      structured AppError (serialized {code, message, details})
  git/          exec.rs (safe runner), repository.rs, diff.rs
  commands/     repository.rs, git.rs (thin #[tauri::command] wrappers)
```

Diff flow: Rust returns one unified patch string (`git diff` with stable
`a/`/`b/` prefixes, `--no-ext-diff`, rename detection) plus base/head SHAs and
the untracked-file list. The frontend parses it once with `parsePatchFiles`,
renders per-file `FileDiff` components, and derives all anchors from the
parsed `FileDiffMetadata` hunks — never from DOM rows.

Untracked files: listed separately in the UI, not diffed (avoids touching the
index). Documented limitation.

## Milestones

1. **Repository & diff foundation** — open repo (validated in Rust), recent
   repositories, working-tree vs HEAD and branch vs base comparisons, changed
   file list, split/unified rendering, viewed state, manual refresh.
2. **Inline review workflow** — persisted review sessions, line/range
   comments anchored as `DiffAnchor`, composer (Cmd/Ctrl+Enter, Escape),
   conservative re-anchoring, review summary + Markdown export, keyboard
   navigation + command palette.
3. **Deterministic reviewer memory** — lexical normaliser behind an
   interface, trigram/shape/context/identifier weighted similarity,
   conservative thresholds and caps, ghost-comment suggestions with
   accept/edit/dismiss/disable and persisted feedback.
4. **GitHub PR integration** (next phase) — PAT in OS keychain, PR by URL,
   grouped review submission, historical comment import.
5. **Optional model assist** (later) — applicability check/adaptation only,
   behind explicit opt-in.

## Deviations from the brief

- None of substance so far. Untracked-file diffing deferred (allowed by the
  brief). GitHub (M4) and model assist (M5) are scheduled after the local
  loop is verified, per the brief's ordering.

## Risks

- `@pierre/diffs` interaction API nuances (selection sides `deletions` /
  `additions`, partial patches) — mitigated by deriving all mapping from the
  documented `Hunk` block indices and unit-testing the mapping.
- Re-anchoring correctness — mitigated with fixture-driven tests and a
  conservative "mark outdated" default.
- Large diffs — mitigated by `Virtualizer`/worker pool and Rust output caps.

## Progress log

- 2026-07-11: Repo inspected, `@pierre/diffs` API surveyed, deps added,
  scaffold type errors fixed, baseline `tsc`/`vite build`/`cargo check` green.
- 2026-07-11: **Milestone 1 complete.** Rust backend (safe git exec,
  repository open/validation, working-tree + branch diffs, 16 tests), SQLite
  schema v1 + typed persistence layer, M1 frontend (shell, picker, file list,
  FileDiff viewer, viewed state, refresh, themes). App launches via
  `tauri dev`. 179 vitest tests + 16 cargo tests green.
- 2026-07-11: Core review/memory logic landed with tests: patch line mapping
  + DiffAnchor building, tiered conservative re-anchoring, lexical
  normaliser, weighted similarity + feedback adjustment, candidate
  extraction/filtering engine (M3 acceptance suite passing). Decisions:
  Biome's `useConsistentTypeDefinitions` disabled (codebase standardises on
  `type`), `@/` alias wired into vite/vitest, TS target bumped to ES2022.
  `docs/security.md` added.
- 2026-07-11: **Milestones 2 and 3 complete.** Inline review workflow:
  selection → DiffAnchor composer, persisted draft comments with edit/delete,
  re-anchor-on-refresh (outdated marking), review summary sheet with Markdown
  export, keyboard shortcuts + command palette. Reviewer memory UI: comment
  saves seed memory examples behind quality gates and a settings toggle;
  suggestions generate once per diff fetch, render as ghost annotations with
  qualitative confidence and provenance, and support accept /
  edit-and-accept / dismiss / never-again with persisted feedback. Final
  state: 187 vitest tests + 16 cargo tests green; `tsc`, `vite build`,
  Ultracite check and `cargo fmt/check` all clean; app runs via `tauri dev`.
  README and `docs/security.md` written.
- 2026-07-11: **Milestone 4 complete.** GitHub PR integration: fine-grained
  PAT in the OS keychain (`keyring`), PR-by-URL review mode reusing the whole
  local pipeline (parsed PR patch → anchors → comments → suggestions),
  grouped review submission (Comment/Approve/Request changes) guarded by a
  head-SHA re-check with exactly-once semantics, and explicit, cancellable,
  deduplicated import of past review comments into reviewer memory. Rust:
  reqwest client (30s timeout, 10 MiB cap), migration 002. Final counts:
  212 vitest + 41 cargo tests. Live testing policy: PRs on
  github.com/arsenstorm/lgtm only (dogfooding LGTM on itself).
- Deviations from the brief, all deliberate: composer Markdown preview
  omitted (no Markdown renderer dependency; bodies render as plain text —
  also a security posture), `?` not bound as a palette shortcut (Cmd/Ctrl+K
  only), hunk-level keyboard navigation deferred (renderer exposes no
  per-hunk navigation API on FileDiff), suggestion generation runs in one
  pass per fetch rather than incrementally per file (engine is bounded by
  window caps; measure before optimising).
