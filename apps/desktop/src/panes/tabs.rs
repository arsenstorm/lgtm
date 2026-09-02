//! The Activity and Plan tabs.

use super::{muted, MARK};
use crate::app::LgtmApp;
use crate::render::{self, Kind, Line};
use crate::theme::{
    icon, Tokens, LINE_MONO, MONO_FONT, RADIUS, SPACE, TEXT_MONO, TEXT_ROW, TEXT_SECONDARY, UI_FONT,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
};
use lgtm_protocol::{StoredEvent, Task, TaskEvent};

/// The session thread's column, reused: past this the narration is past a
/// comfortable measure.
const TIMELINE_W: f32 = 720.;

/// The tone an entry carries, resolved to a colour only at render.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tone {
    Plain,
    Muted,
    Message,
    Success,
    Warning,
    Danger,
}

/// One line of the timeline: a whole run of commands or edits, or a single
/// event that speaks for itself.
#[derive(Debug, PartialEq)]
struct Entry {
    /// Where the entry starts in `events`. Events only ever append, so the
    /// index stays put and can key what the reader unfolded.
    key: usize,
    mark: Option<&'static str>,
    tone: Tone,
    title: String,
    /// The raw lines a group folds away; empty for a single event.
    body: Vec<Line>,
}

/// The runs the timeline folds. Everything else stands on its own.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Group {
    Commands,
    Files,
}

fn group_of(event: &TaskEvent) -> Option<Group> {
    match event {
        TaskEvent::Command { .. } | TaskEvent::Output { .. } => Some(Group::Commands),
        TaskEvent::FileChanged { .. } => Some(Group::Files),
        _ => None,
    }
}

fn entries(events: &[StoredEvent]) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < events.len() {
        let Some(group) = group_of(&events[at].event) else {
            out.extend(entry(at, &events[at].event));
            at += 1;
            continue;
        };
        let end = at
            + events[at..]
                .iter()
                .take_while(|stored| group_of(&stored.event) == Some(group))
                .count();
        out.extend(grouped(at, group, &events[at..end]));
        at = end;
    }
    out
}

fn grouped(key: usize, group: Group, events: &[StoredEvent]) -> Option<Entry> {
    let body: Vec<Line> = events
        .iter()
        .flat_map(|stored| render::render(&stored.event))
        .collect();
    // A run of stream-json stdout renders to nothing, and a group with nothing
    // to unfold is worse than no group at all.
    if body.is_empty() {
        return None;
    }
    let title = match group {
        Group::Commands => match commands(events) {
            0 => "Output".to_string(),
            n => format!("Ran {n} command{}", plural(n)),
        },
        Group::Files => format!("Edited {} file{}", events.len(), plural(events.len())),
    };
    Some(Entry {
        key,
        mark: None,
        tone: Tone::Muted,
        title,
        body,
    })
}

fn commands(events: &[StoredEvent]) -> usize {
    events
        .iter()
        .filter(|stored| matches!(stored.event, TaskEvent::Command { .. }))
        .count()
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

fn entry(key: usize, event: &TaskEvent) -> Option<Entry> {
    let text = render::render(event).into_iter().next()?.text;
    let (mark, tone) = match event {
        TaskEvent::Progress { .. } => (None, Tone::Plain),
        TaskEvent::Message { .. } => (None, Tone::Message),
        TaskEvent::Completed { .. } => (Some("check"), Tone::Success),
        TaskEvent::PermissionRequested { .. } | TaskEvent::NetworkDenied { .. } => {
            (Some("info"), Tone::Warning)
        }
        TaskEvent::Failed { .. }
        | TaskEvent::TimedOut { .. }
        | TaskEvent::RunnerLost
        | TaskEvent::Cancelled
        | TaskEvent::Conflicted { .. } => (Some("x"), Tone::Danger),
        _ => (None, Tone::Muted),
    };
    let title = match event {
        // The agent's narration and a person's follow-up are content, not
        // status: they keep the words they were written in.
        TaskEvent::Progress { .. } | TaskEvent::Message { .. } => text,
        TaskEvent::Started { model: Some(model) } => format!("{} · {model}", sentence(&text)),
        _ => sentence(&text),
    };
    Some(Entry {
        key,
        mark,
        tone,
        title,
        body: Vec::new(),
    })
}

/// `render` writes log lines, lowercase; the timeline reads as sentences.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// What the task did, one entry per thing it did, with the raw log a toggle
/// away for whoever wants the lines themselves.
pub(super) fn activity(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> AnyElement {
    let raw_on = app.ui.timeline_raw;
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[1]))
        .child(controls(raw_on, t, cx))
        .child(if raw_on {
            raw(app, t)
        } else {
            timeline(app, t, cx)
        })
        .into_any_element()
}

fn controls(raw_on: bool, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    div().flex().justify_end().child(
        div()
            .id("activity-raw")
            .px(px(SPACE[1]))
            .rounded(px(RADIUS))
            .cursor_pointer()
            .text_size(px(TEXT_SECONDARY))
            .text_color(t.muted_fg)
            .when(raw_on, |this| this.bg(t.muted))
            .hover(|this| this.bg(t.muted))
            .child("Raw")
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.ui.timeline_raw = !this.ui.timeline_raw;
                cx.notify();
            })),
    )
}

/// The one genuinely monospace view: a stream of command lines and output,
/// where a colour means something is wrong rather than something happened.
fn raw(app: &LgtmApp, t: &Tokens) -> AnyElement {
    if app.lines.is_empty() {
        return muted("Nothing yet.", t);
    }
    div()
        .flex()
        .flex_col()
        .font_family(MONO_FONT)
        .text_size(px(TEXT_MONO))
        .line_height(px(LINE_MONO))
        .children(app.lines.iter().map(|line| mono_line(line, t)))
        .into_any_element()
}

fn timeline(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> AnyElement {
    let entries = entries(&app.events);
    if entries.is_empty() {
        return muted("Nothing yet.", t);
    }
    div()
        .flex()
        .flex_col()
        // A measure cap: the agent's narration is prose, and a line that runs
        // the whole pane is past reading width.
        .max_w(px(TIMELINE_W))
        .font_family(UI_FONT)
        .text_size(px(TEXT_ROW))
        .children(
            entries
                .into_iter()
                .map(|entry| entry_row(app, entry, t, cx)),
        )
        .into_any_element()
}

fn entry_row(app: &LgtmApp, entry: Entry, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    if entry.body.is_empty() {
        return line_row(&entry, t);
    }
    let Entry {
        key, title, body, ..
    } = entry;
    let open = app.ui.timeline_expanded.contains(&key);
    let chevron = if open {
        "chevron-down"
    } else {
        "chevron-right"
    };
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .id(SharedString::from(format!("timeline-{key}")))
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .cursor_pointer()
                .hover(|this| this.bg(t.muted))
                .child(icon(chevron, MARK, t.muted_fg))
                .child(div().text_color(t.muted_fg).child(title))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if !this.ui.timeline_expanded.remove(&key) {
                        this.ui.timeline_expanded.insert(key);
                    }
                    cx.notify();
                })),
        )
        .when(open, |this| {
            this.child(
                div()
                    .flex()
                    .flex_col()
                    .pl(px(SPACE[3]))
                    .font_family(MONO_FONT)
                    .text_size(px(TEXT_MONO))
                    .line_height(px(LINE_MONO))
                    .children(body.iter().map(|line| mono_line(line, t))),
            )
        })
}

/// Like Review's check rows: the mark carries the tone, the words stay plain.
fn line_row(entry: &Entry, t: &Tokens) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .when_some(entry.mark, |this, mark| {
            this.child(icon(mark, MARK, tone_color(entry.tone, t)))
        })
        .when(entry.tone == Tone::Muted, |this| {
            this.text_color(t.muted_fg)
        })
        .when(entry.tone == Tone::Message, |this| {
            this.font_weight(FontWeight::MEDIUM)
        })
        .child(entry.title.clone())
}

fn tone_color(tone: Tone, t: &Tokens) -> Hsla {
    match tone {
        Tone::Success => t.success,
        Tone::Warning => t.warning,
        Tone::Danger => t.danger,
        Tone::Plain | Tone::Message => t.fg,
        Tone::Muted => t.muted_fg,
    }
}

fn mono_line(line: &Line, t: &Tokens) -> Div {
    let color = match line.kind {
        Kind::Text | Kind::Message => t.fg,
        Kind::Tool | Kind::Status => t.muted_fg,
        Kind::Stderr => t.danger,
    };
    div().text_color(color).child(line.text.clone())
}

pub(super) fn plan_pane(task: &Task, t: &Tokens) -> AnyElement {
    let Some(plan) = task.result.as_ref().and_then(|r| r.plan.as_ref()) else {
        return muted("No plan.", t);
    };
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[3]))
        .text_size(px(TEXT_ROW))
        .children(plan.steps.iter().enumerate().map(|(i, step)| {
            div()
                .flex()
                .flex_col()
                .gap(px(SPACE[0]))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(SPACE[1]))
                        .child(div().font_weight(FontWeight::MEDIUM).child(format!(
                            "{}. {}",
                            i + 1,
                            step.title
                        )))
                        .child(
                            div()
                                .text_size(px(TEXT_SECONDARY))
                                .text_color(t.muted_fg)
                                .child(step.key.clone()),
                        ),
                )
                .child(div().text_color(t.muted_fg).child(step.prompt.clone()))
                .when(!step.depends_on.is_empty(), |this| {
                    this.child(
                        div()
                            .text_color(t.muted_fg)
                            .child(format!("after: {}", step.depends_on.join(", "))),
                    )
                })
        }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{OutputStream, TaskResult};

    fn ev(event: TaskEvent) -> StoredEvent {
        StoredEvent { at: 0, event }
    }

    fn command(command: &str) -> StoredEvent {
        ev(TaskEvent::Command {
            command: command.into(),
        })
    }

    fn stdout(line: &str) -> StoredEvent {
        ev(TaskEvent::Output {
            stream: OutputStream::Stdout,
            line: line.into(),
        })
    }

    fn titles(events: &[StoredEvent]) -> Vec<String> {
        entries(events).into_iter().map(|e| e.title).collect()
    }

    #[test]
    fn a_run_of_commands_and_their_output_is_one_entry() {
        let events = vec![
            command("ls"),
            stdout("a.rs"),
            stdout(r#"{"type":"system","subtype":"init"}"#),
            command("cat a.rs"),
            stdout("fn main() {}"),
        ];
        let entries = entries(&events);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, 0);
        assert_eq!(entries[0].title, "Ran 2 commands");
        // The swallowed stream-json line is in the run but not in the body.
        assert_eq!(entries[0].body.len(), 4);
    }

    #[test]
    fn edits_group_and_a_progress_line_splits_the_commands() {
        let events = vec![
            command("touch a"),
            ev(TaskEvent::Progress {
                text: "Editing.".into(),
            }),
            command("touch b"),
            ev(TaskEvent::FileChanged {
                path: "src/a.rs".into(),
            }),
            ev(TaskEvent::FileChanged {
                path: "src/b.rs".into(),
            }),
        ];
        assert_eq!(
            titles(&events),
            vec![
                "Ran 1 command",
                "Editing.",
                "Ran 1 command",
                "Edited 2 files"
            ]
        );
        let edits = &entries(&events)[3];
        assert_eq!(edits.key, 3);
        assert_eq!(
            edits
                .body
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>(),
            vec!["~ src/a.rs", "~ src/b.rs"]
        );
    }

    #[test]
    fn a_whole_run_reads_as_a_script() {
        let events = vec![
            ev(TaskEvent::Started {
                model: Some("opus".into()),
            }),
            command("cargo test"),
            stdout("ok"),
            ev(TaskEvent::Progress {
                text: "Tests pass.".into(),
            }),
            ev(TaskEvent::PermissionRequested {
                kind: "host".into(),
                target: "crates.io".into(),
                reason: "fetch".into(),
            }),
            ev(TaskEvent::Completed {
                result: TaskResult {
                    branch: "b".into(),
                    diff: String::new(),
                    changed_files: vec!["src/a.rs".into()],
                    validation: Vec::new(),
                    plan: None,
                    review: None,
                    policy: None,
                    cost_usd: 0.,
                },
            }),
        ];
        assert_eq!(
            titles(&events),
            vec![
                "Agent started · opus",
                "Ran 1 command",
                "Tests pass.",
                "Permission requested: host crates.io — fetch",
                "Completed: 1 files changed",
            ]
        );
    }
}
