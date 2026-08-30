//! The History tab: the project's finished tasks, newest first.

use super::{muted, tasks_of};
use crate::app::LgtmApp;
use crate::labels::{header_preview, status_label};
use crate::tasks::{duration, status_color};
use crate::theme::{Tokens, RADIUS, SPACE, TEXT_SECONDARY};
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
};
use lgtm_protocol::{Execution, Task};

/// The tasks in `slug` that are done with, newest first.
pub fn terminal_tasks<'a>(app: &'a LgtmApp, slug: &str) -> Vec<&'a Task> {
    newest_first(tasks_of(app, slug))
}

fn newest_first(tasks: Vec<&Task>) -> Vec<&Task> {
    let mut rows: Vec<&Task> = tasks
        .into_iter()
        .filter(|task| task.status.is_terminal())
        .collect();
    rows.sort_by_key(|task| std::cmp::Reverse(task.created_at));
    rows
}

/// Wall-clock time from the first attempt starting to the last one ending;
/// `None` while no attempt has finished.
pub fn ran_for(executions: &[Execution]) -> Option<u64> {
    let started = executions.iter().map(|run| run.started_at).min()?;
    let finished = executions.iter().filter_map(|run| run.finished_at).max()?;
    Some(finished.saturating_sub(started))
}

pub(super) fn rows(
    app: &LgtmApp,
    slug: &str,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Vec<AnyElement> {
    let rows = terminal_tasks(app, slug);
    if rows.is_empty() {
        return vec![muted("Nothing has finished in this project yet.", t)];
    }
    rows.into_iter()
        .map(|task| row(app, task, t, cx).into_any_element())
        .collect()
}

fn row(app: &LgtmApp, task: &Task, t: &Tokens, cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    let id = task.id.clone();
    let cost = task.result.as_ref().map(|r| r.cost_usd).unwrap_or(0.0);
    div()
        .id(SharedString::from(format!("history:{}", task.id)))
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .p(px(SPACE[1]))
        .rounded(px(RADIUS))
        .bg(t.card)
        .border_1()
        .border_color(t.border)
        .cursor_pointer()
        .hover(|this| this.bg(t.muted))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(header_preview(&task.spec.prompt)),
        )
        .child(cell(
            status_label(task, &app.tasks).to_string(),
            status_color(task, &app.tasks, t),
        ))
        .child(cell(
            ran_for(&task.executions).map(duration).unwrap_or_default(),
            t.muted_fg,
        ))
        .child(cell(format!("${cost:.2}"), t.muted_fg))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select(id.clone(), cx)))
}

fn cell(text: String, tone: gpui::Hsla) -> Div {
    div()
        .w(px(96.))
        .flex_shrink_0()
        .truncate()
        .text_size(px(TEXT_SECONDARY))
        .text_color(tone)
        .child(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{ExecutionStatus, TaskStatus};

    #[test]
    fn history_keeps_the_finished_tasks_newest_first() {
        let finished = |at: u64, status| {
            let mut task = crate::project::tests::task("repo", status);
            task.created_at = at;
            task
        };
        let tasks = [
            finished(1, TaskStatus::Merged),
            finished(3, TaskStatus::Running),
            finished(2, TaskStatus::Failed),
        ];
        let ages: Vec<u64> = newest_first(tasks.iter().collect())
            .iter()
            .map(|task| task.created_at)
            .collect();
        assert_eq!(ages, vec![2, 1]);
    }

    fn run(started_at: u64, finished_at: Option<u64>) -> Execution {
        Execution {
            attempt: 1,
            runner: "r".into(),
            executor: Default::default(),
            model: None,
            started_at,
            finished_at,
            status: ExecutionStatus::Completed,
            error: None,
            cost_usd: 0.0,
            validation: vec![],
        }
    }

    #[test]
    fn a_run_spans_the_first_start_to_the_last_finish() {
        let runs = [run(1_000, Some(4_000)), run(10_000, Some(12_000))];
        assert_eq!(ran_for(&runs), Some(11_000));
        assert_eq!(ran_for(&[run(1_000, None)]), None);
        assert_eq!(ran_for(&[]), None);
    }
}
