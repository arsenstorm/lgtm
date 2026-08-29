//! The left rail: quick actions, tasks grouped by repository, and one status
//! row that opens Settings.

use crate::app::{prompt_preview, status_label, LgtmApp, Overlay, Page};
use crate::theme::{tokens, Tokens, FOOTER_H, MONO_FONT, ROW_H, SPACE, TEXT_MONO, TEXT_SECONDARY};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window,
};
use lgtm_protocol::{Batch, BatchSource, Task};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

pub const WIDTH: f32 = 240.;
const PROMPT_PREVIEW: usize = 32;
/// Sidebar rows are the one place that keeps `rounded-md`; a pill this short
/// would read as a lozenge, not a list.
const ROW_RADIUS: f32 = 8.;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `https://github.com/you/repo.git` -> `repo`. Also handles scp-style git
/// URLs, whose last `/` segment is the repository too.
pub fn repo_slug(repository: &str) -> String {
    let last = repository
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(repository);
    last.strip_suffix(".git").unwrap_or(last).to_string()
}

/// Tasks bucketed by repository slug, groups and rows newest-first, with every
/// task's children directly after it.
pub fn group_by_repo(tasks: &[Task]) -> Vec<(String, Vec<&Task>)> {
    let mut newest: Vec<&Task> = tasks.iter().collect();
    newest.sort_by_key(|task| std::cmp::Reverse(task.created_at));

    let mut groups: Vec<(String, Vec<&Task>)> = Vec::new();
    for task in newest {
        let slug = repo_slug(&task.spec.repository);
        match groups.iter_mut().find(|(name, _)| name == &slug) {
            Some((_, rows)) => rows.push(task),
            None => groups.push((slug, vec![task])),
        }
    }
    for (_, rows) in &mut groups {
        *rows = parents_first(rows);
    }
    groups
}

fn parents_first<'a>(rows: &[&'a Task]) -> Vec<&'a Task> {
    let present: HashSet<&str> = rows.iter().map(|task| task.id.as_str()).collect();
    let mut out = Vec::with_capacity(rows.len());
    for task in rows {
        // Emitted with its parent below; a child whose parent is in another
        // repository has nothing to hang under, so it stays top level.
        if task
            .spec
            .parent
            .as_deref()
            .is_some_and(|parent| present.contains(parent))
        {
            continue;
        }
        out.push(*task);
        push_children(task, rows, &mut out);
    }
    out
}

fn push_children<'a>(parent: &Task, rows: &[&'a Task], out: &mut Vec<&'a Task>) {
    if out.len() >= rows.len() {
        return;
    }
    for task in rows {
        if task.spec.parent.as_deref() == Some(parent.id.as_str()) {
            out.push(*task);
            push_children(task, rows, out);
        }
    }
}

/// Coarse age, one unit only: `12s`, `5m`, `2h`, `3d`.
pub fn relative_age(created_at: u64, now: u64) -> String {
    let secs = now.saturating_sub(created_at) / 1000;
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86_399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
}

pub fn status_color(task: &Task, tasks: &[Task], t: &Tokens) -> Hsla {
    match status_label(task, tasks) {
        "awaiting_review" => t.warning,
        "running" => t.info,
        "approved" | "merged" => t.success,
        "failed" | "rejected" | "cancelled" => t.danger,
        _ => t.muted_fg,
    }
}

/// `o/r label:L` for a GitHub label batch, `TEAM/STATE` for a Linear batch.
pub fn batch_label(source: &BatchSource) -> String {
    match source {
        BatchSource::GithubLabel { owner, repo, label } => format!("{owner}/{repo} label:{label}"),
        BatchSource::Linear { team, state } => format!("{team}/{state}"),
    }
}

pub fn render_sidebar(app: &mut LgtmApp, _window: &mut Window, cx: &mut Context<LgtmApp>) -> Div {
    let t = tokens(cx);
    div()
        .w(px(WIDTH))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .bg(t.sidebar)
        .border_r_1()
        .border_color(t.sidebar_border)
        .child(quick_actions(app, &t, cx))
        .child(
            div()
                .id("tasks")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&app.task_scroll)
                .px(px(SPACE[1]))
                .pb(px(SPACE[1]))
                .children(repository_groups(app, &t, cx)),
        )
        .child(footer(app, &t, cx))
}

fn quick_actions(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let home = app.selected.is_none() && app.page == Page::Home;
    let batches = app.selected.is_none() && app.page == Page::Batches;
    div()
        .flex()
        .flex_col()
        .p(px(SPACE[1]))
        .gap(px(2.))
        .border_b_1()
        .border_color(t.sidebar_border)
        .child(
            action_row("new-task", "＋", "New task", "⌘N", home, t)
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.go_home(window, cx))),
        )
        .child(
            action_row(
                "search",
                "⌕",
                "Search",
                "⌘K",
                app.overlay == Overlay::Palette,
                t,
            )
            .on_click(
                cx.listener(|this, _: &ClickEvent, window, cx| this.open_palette(window, cx)),
            ),
        )
        .child(
            action_row("batches", "▣", "Batches", "", batches, t).on_click(
                cx.listener(|this, _: &ClickEvent, _, cx| this.show_page(Page::Batches, cx)),
            ),
        )
        .child(
            action_row(
                "settings",
                "⚙",
                "Settings",
                "",
                app.overlay == Overlay::Settings,
                t,
            )
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_settings(false, cx))),
        )
}

/// The shared sidebar row: 28px tall, `text-xs`, `accent` on hover, `accent`
/// plus full-strength text when it is the current one.
fn row_shell(id: impl Into<SharedString>, active: bool, t: &Tokens) -> gpui::Stateful<Div> {
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .h(px(ROW_H))
        .px(px(SPACE[1]))
        .rounded(px(ROW_RADIUS))
        .cursor_pointer()
        .text_size(px(TEXT_SECONDARY))
        .text_color(if active { t.fg } else { t.muted_fg })
        .when(active, |this| this.bg(t.muted))
        .hover(|this| this.bg(t.muted))
}

fn action_row(
    id: &'static str,
    glyph: &'static str,
    label: &'static str,
    key: &'static str,
    active: bool,
    t: &Tokens,
) -> gpui::Stateful<Div> {
    row_shell(id, active, t)
        .child(div().w(px(14.)).text_color(t.muted_fg).child(glyph))
        .child(div().flex_1().child(label))
        .child(div().text_color(t.muted_fg).child(key))
}

fn repository_groups(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let visible: Vec<Task> = app.tasks.clone();

    if visible.is_empty() {
        return vec![div()
            .px(px(SPACE[1]))
            .py(px(SPACE[2]))
            .text_size(px(TEXT_SECONDARY))
            .text_color(t.muted_fg)
            .child("No tasks yet")
            .into_any_element()];
    }

    let now = now_ms();
    let mut out = Vec::new();
    for (slug, rows) in group_by_repo(&visible) {
        out.push(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[0]))
                .h(px(ROW_H))
                .px(px(SPACE[1]))
                .pt(px(SPACE[1]))
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.muted_fg)
                .font_weight(FontWeight::MEDIUM)
                .child("📁")
                .child(slug)
                .into_any_element(),
        );
        let mut seen_batch: HashSet<&str> = HashSet::new();
        for task in rows {
            if let Some(id) = task.spec.batch.as_deref() {
                if seen_batch.insert(id) {
                    if let Some(batch) = app.batches.iter().find(|b| b.id == id) {
                        out.push(batch_header(batch, t).into_any_element());
                    }
                }
            }
            out.push(task_row(app, task, now, t, cx).into_any_element());
        }
    }
    out
}

fn batch_header(batch: &Batch, t: &Tokens) -> Div {
    div()
        .px(px(SPACE[2]))
        .py(px(2.))
        .font_family(MONO_FONT)
        .text_size(px(TEXT_MONO))
        .text_color(t.muted_fg)
        .child(format!("▣ {}", batch_label(&batch.source)))
}

fn task_row(
    app: &LgtmApp,
    task: &Task,
    now: u64,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> gpui::Stateful<Div> {
    let id = task.id.clone();
    let active = app.selected.as_deref() == Some(id.as_str());
    let child = task.spec.parent.is_some();
    let prompt = prompt_preview(&task.spec.prompt, PROMPT_PREVIEW);
    let dot = status_color(task, &app.tasks, t);

    row_shell(SharedString::from(format!("task-{id}")), active, t)
        .pl(px(if child { SPACE[2] } else { SPACE[1] }))
        .font_family(MONO_FONT)
        .text_size(px(TEXT_MONO))
        .child(
            div()
                .flex_shrink_0()
                .w(px(6.))
                .h(px(6.))
                .rounded_full()
                .bg(dot),
        )
        .child(div().flex_1().min_w_0().truncate().child(if child {
            format!("↳ {prompt}")
        } else {
            prompt
        }))
        .child(
            div()
                .flex_shrink_0()
                .text_color(t.muted_fg)
                .child(relative_age(task.created_at, now)),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select(id.clone(), cx)))
}

/// One row: whether the orchestrator answered, and the way into Settings.
fn footer(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> gpui::Stateful<Div> {
    let status = if app.reachable {
        let n = app.workers.len();
        format!("Connected · {n} worker{}", if n == 1 { "" } else { "s" })
    } else {
        "Not connected".to_string()
    };
    div()
        .id("status")
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(SPACE[1]))
        .h(px(FOOTER_H))
        .px(px(SPACE[2]))
        .border_t_1()
        .border_color(t.sidebar_border)
        .text_size(px(TEXT_SECONDARY))
        .text_color(t.muted_fg)
        .cursor_pointer()
        .hover(|this| this.bg(t.muted))
        .child(
            div()
                .flex_shrink_0()
                .w(px(6.))
                .h(px(6.))
                .rounded_full()
                .bg(if app.reachable { t.success } else { t.danger }),
        )
        .child(div().flex_1().min_w_0().truncate().child(status))
        .child(div().flex_shrink_0().child("⚙"))
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_settings(true, cx)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{Executor, TaskKind, TaskSpec, TaskStatus};

    fn task(id: &str, repo: &str, created_at: u64, parent: Option<&str>) -> Task {
        Task {
            id: id.into(),
            spec: TaskSpec {
                repository: repo.into(),
                base_branch: "main".into(),
                prompt: "p".into(),
                executor: Executor::Claude,
                worker: None,
                issue: None,
                linear: None,
                kind: TaskKind::Run,
                parent: parent.map(String::from),
                depends_on: vec![],
                batch: None,
            },
            status: TaskStatus::Queued,
            worker: None,
            created_at,
            result: None,
            error: None,
            pull_request: None,
            ci: None,
        }
    }

    #[test]
    fn slug_drops_host_and_git_suffix() {
        assert_eq!(repo_slug("https://github.com/you/repo.git"), "repo");
        assert_eq!(repo_slug("git@github.com:you/repo.git"), "repo");
        assert_eq!(repo_slug("https://github.com/you/repo/"), "repo");
        assert_eq!(repo_slug("repo"), "repo");
    }

    #[test]
    fn groups_are_newest_first_within_and_across_repositories() {
        let tasks = vec![
            task("a", "https://x/one.git", 10, None),
            task("b", "https://x/two.git", 30, None),
            task("c", "https://x/one.git", 20, None),
        ];
        let groups = group_by_repo(&tasks);
        assert_eq!(groups[0].0, "two");
        assert_eq!(groups[1].0, "one");
        let ids: Vec<&str> = groups[1].1.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["c", "a"]);
    }

    #[test]
    fn children_follow_their_parent_even_when_newer() {
        let tasks = vec![
            task("parent", "https://x/one.git", 10, None),
            task("child", "https://x/one.git", 99, Some("parent")),
            task("other", "https://x/one.git", 50, None),
        ];
        let ids: Vec<&str> = group_by_repo(&tasks)[0]
            .1
            .iter()
            .map(|t| t.id.as_str())
            .collect();
        assert_eq!(ids, vec!["other", "parent", "child"]);
    }

    #[test]
    fn orphan_child_stays_at_top_level() {
        let tasks = vec![task("child", "https://x/one.git", 1, Some("elsewhere"))];
        assert_eq!(group_by_repo(&tasks)[0].1.len(), 1);
    }

    #[test]
    fn age_uses_one_coarse_unit() {
        let now = 10_000_000_000;
        assert_eq!(relative_age(now - 12_000, now), "12s");
        assert_eq!(relative_age(now - 5 * 60_000, now), "5m");
        assert_eq!(relative_age(now - 2 * 3_600_000, now), "2h");
        assert_eq!(relative_age(now - 3 * 86_400_000, now), "3d");
        assert_eq!(relative_age(now + 1000, now), "0s");
    }

    #[test]
    fn batch_label_formats_both_sources() {
        assert_eq!(
            batch_label(&BatchSource::GithubLabel {
                owner: "o".into(),
                repo: "r".into(),
                label: "L".into()
            }),
            "o/r label:L"
        );
        assert_eq!(
            batch_label(&BatchSource::Linear {
                team: "TEAM".into(),
                state: "STATE".into()
            }),
            "TEAM/STATE"
        );
    }
}
