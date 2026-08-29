//! Pumping an executor's output to the orchestrator, and what its JSON lines
//! carry: the session id and the agent's final answer. Claude and Codex say
//! the same things in different shapes, so each parser branches on the executor.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use lgtm_protocol::{Executor, OutputStream, TaskEvent, TaskId};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

use crate::connection::Ctx;
use crate::validate::TAIL_LINES;

pub type Tail = Arc<Mutex<VecDeque<String>>>;

pub fn tail_buffer() -> Tail {
    Arc::new(Mutex::new(VecDeque::new()))
}

pub fn tail_lines(tail: &Tail) -> Vec<String> {
    tail.lock()
        .expect("tail poisoned")
        .iter()
        .cloned()
        .collect()
}

/// What the agent said, as the run goes: the `result` line when one arrives,
/// the assistant text blocks otherwise.
#[derive(Default)]
pub struct Answer {
    result: Option<String>,
    assistant: String,
}

pub type Text = Arc<Mutex<Answer>>;

pub fn text_buffer() -> Text {
    Arc::new(Mutex::new(Answer::default()))
}

pub fn final_text(text: &Text) -> String {
    let answer = text.lock().expect("answer poisoned");
    answer
        .result
        .clone()
        .unwrap_or_else(|| answer.assistant.clone())
}

/// Dollars reported by every `result` line seen so far. Shared across the runs
/// of one task, so retries and the review add to the same total.
pub type Cost = Arc<Mutex<f64>>;

pub fn cost_buffer() -> Cost {
    Arc::new(Mutex::new(0.0))
}

pub fn cost_total(cost: &Cost) -> f64 {
    *cost.lock().expect("cost poisoned")
}

/// What one run of the pump records besides forwarding the line.
#[derive(Default, Clone)]
pub struct Sinks {
    pub tail: Option<Tail>,
    /// Where to write the session id the run announces.
    pub session: Option<PathBuf>,
    pub text: Option<Text>,
    pub cost: Option<Cost>,
}

/// Forwards one output stream of a run to the orchestrator, recording what
/// `sinks` asks for on the way.
pub struct Pump {
    pub ctx: Arc<Ctx>,
    pub task_id: TaskId,
    pub stream: OutputStream,
    pub executor: Executor,
    pub sinks: Sinks,
}

impl Pump {
    pub async fn run<R: AsyncRead + Unpin>(mut self, reader: R) {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            self.record(&line).await;
            let stream = self.stream;
            let events = match stream {
                OutputStream::Stdout => structured(&line, self.executor),
                OutputStream::Stderr => Vec::new(),
            };
            self.ctx
                .emit(&self.task_id, TaskEvent::Output { stream, line });
            for event in events {
                self.ctx.emit(&self.task_id, event);
            }
        }
    }

    async fn record(&mut self, line: &str) {
        let executor = self.executor;
        let sinks = &mut self.sinks;
        if let Some(text) = &sinks.text {
            capture_answer(line, text, executor);
        }
        if let (Some(cost), Some(spent)) = (&sinks.cost, result_cost(line, executor)) {
            *cost.lock().expect("cost poisoned") += spent;
        }
        if let (Some(path), Some(id)) = (&sinks.session, init_session_id(line, executor)) {
            if let Err(err) = tokio::fs::write(path, &id).await {
                tracing::warn!("write {}: {err}", path.display());
            }
            sinks.session = None;
        }
        if let Some(tail) = &sinks.tail {
            push_tail(tail, line);
        }
    }
}

fn push_tail(tail: &Tail, line: &str) {
    let mut tail = tail.lock().expect("tail poisoned");
    tail.push_back(line.to_string());
    if tail.len() > TAIL_LINES {
        tail.pop_front();
    }
}

fn capture_answer(line: &str, text: &Text, executor: Executor) {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return;
    };
    match executor {
        Executor::Claude => claude_answer(&value, text),
        Executor::Codex => codex_answer(&value, text),
    }
}

fn claude_answer(value: &Value, text: &Text) {
    match value.get("type").and_then(Value::as_str) {
        Some("result") => {
            if let Some(result) = value.get("result").and_then(Value::as_str) {
                text.lock().expect("answer poisoned").result = Some(result.to_string());
            }
        }
        Some("assistant") => {
            let blocks = value.pointer("/message/content").and_then(Value::as_array);
            for block in blocks.into_iter().flatten() {
                if let Some(part) = block.get("text").and_then(Value::as_str) {
                    text.lock()
                        .expect("answer poisoned")
                        .assistant
                        .push_str(part);
                }
            }
        }
        _ => {}
    }
}

/// Codex ends a turn with an `agent_message`, so the newest one is its answer
/// the way Claude's `result` line is.
fn codex_answer(value: &Value, text: &Text) {
    let Some(item) = completed_item(value, "agent_message") else {
        return;
    };
    if let Some(said) = item.get("text").and_then(Value::as_str) {
        text.lock().expect("answer poisoned").result = Some(said.to_string());
    }
}

/// What a Codex line says went wrong: a failed turn, or a completed item that
/// is an error. Both read like Claude's failed `result` line, so the renderers
/// print them the same way.
pub fn codex_error(value: &Value) -> Option<&str> {
    match value.get("type")?.as_str()? {
        "turn.failed" => value.pointer("/error/message")?.as_str(),
        "item.completed" => completed_item(value, "error")?.get("message")?.as_str(),
        _ => None,
    }
}

/// The `item` of an `item.completed` line, when it is of the given kind.
fn completed_item<'a>(value: &'a Value, kind: &str) -> Option<&'a Value> {
    if value.get("type")?.as_str()? != "item.completed" {
        return None;
    }
    let item = value.get("item")?;
    (item.get("type")?.as_str()? == kind).then_some(item)
}

/// One `Progress` event's ceiling. Agents narrate at length, and the raw
/// `Output` line still carries the whole thing for anyone who wants it.
const PROGRESS_MAX: usize = 2000;

/// What a stdout line says the agent did, so clients render events instead of
/// scraping the executor's JSON.
pub fn structured(line: &str, executor: Executor) -> Vec<TaskEvent> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    match executor {
        Executor::Claude => claude_structured(&value),
        Executor::Codex => codex_structured(&value),
    }
}

/// Only `assistant` lines carry anything.
fn claude_structured(value: &Value) -> Vec<TaskEvent> {
    if value.get("type").and_then(Value::as_str) != Some("assistant") {
        return Vec::new();
    }
    let blocks = value.pointer("/message/content").and_then(Value::as_array);
    blocks
        .into_iter()
        .flatten()
        .filter_map(block_event)
        .collect()
}

fn codex_structured(value: &Value) -> Vec<TaskEvent> {
    if let Some(item) = completed_item(value, "command_execution") {
        return command_event(item.get("command")).into_iter().collect();
    }
    if let Some(item) = completed_item(value, "file_change") {
        return changed_files(item.get("changes"));
    }
    let said = completed_item(value, "agent_message").and_then(|item| item.get("text"));
    progress_event(said).into_iter().collect()
}

fn command_event(command: Option<&Value>) -> Option<TaskEvent> {
    Some(TaskEvent::Command {
        command: command?.as_str()?.to_string(),
    })
}

fn changed_files(changes: Option<&Value>) -> Vec<TaskEvent> {
    changes
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|change| change.get("path")?.as_str())
        .map(|path| TaskEvent::FileChanged {
            path: path.to_string(),
        })
        .collect()
}

fn block_event(block: &Value) -> Option<TaskEvent> {
    let field = |key: &str| block.get(key).and_then(Value::as_str);
    match field("type")? {
        "text" => progress_event(block.get("text")),
        "tool_use" => tool_event(field("name")?, block.get("input")?),
        _ => None,
    }
}

fn progress_event(text: Option<&Value>) -> Option<TaskEvent> {
    let text = text?.as_str()?.trim();
    (!text.is_empty()).then(|| TaskEvent::Progress {
        text: text.chars().take(PROGRESS_MAX).collect(),
    })
}

fn tool_event(name: &str, input: &Value) -> Option<TaskEvent> {
    let field = |key: &str| input.get(key).and_then(Value::as_str);
    let path = match name {
        "Bash" => {
            return Some(TaskEvent::Command {
                command: field("command")?.to_string(),
            })
        }
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            field("file_path").or_else(|| field("notebook_path"))?
        }
        _ => return None,
    };
    Some(TaskEvent::FileChanged {
        path: path.to_string(),
    })
}

/// What the run cost, from Claude's stream-json `result` line. Every other
/// line is ignored, and so is a result line without a cost.
///
/// Codex reports tokens rather than dollars, and pricing them here would need a
/// per-model price table that goes stale, so a Codex run costs nothing.
pub fn result_cost(line: &str, executor: Executor) -> Option<f64> {
    if executor == Executor::Codex {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    if value.get("type")?.as_str()? != "result" {
        return None;
    }
    value.get("total_cost_usd")?.as_f64()
}

/// The session id the run announces, so a follow-up can resume it: Claude's
/// stream-json init line, Codex's `thread.started`. Every other line is
/// ignored.
pub fn init_session_id(line: &str, executor: Executor) -> Option<String> {
    let value: Value = serde_json::from_str(line).ok()?;
    let kind = value.get("type")?.as_str()?;
    match executor {
        Executor::Claude if kind == "system" && value.get("subtype")?.as_str()? == "init" => {
            Some(value.get("session_id")?.as_str()?.to_string())
        }
        Executor::Codex if kind == "thread.started" => {
            Some(value.get("thread_id")?.as_str()?.to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude(line: &str) -> Vec<TaskEvent> {
        structured(line, Executor::Claude)
    }

    fn codex(line: &str) -> Vec<TaskEvent> {
        structured(line, Executor::Codex)
    }

    #[test]
    fn reads_the_session_id_from_the_init_line() {
        let line =
            r#"{"type":"system","subtype":"init","cwd":"/x","session_id":"abc-123","tools":[]}"#;
        assert_eq!(
            init_session_id(line, Executor::Claude).as_deref(),
            Some("abc-123")
        );
    }

    #[test]
    fn reads_the_session_id_from_the_codex_thread_line() {
        let line =
            r#"{"type":"thread.started","thread_id":"01a04eb1-dc72-71a2-868b-501de228f396"}"#;
        assert_eq!(
            init_session_id(line, Executor::Codex).as_deref(),
            Some("01a04eb1-dc72-71a2-868b-501de228f396")
        );
        assert!(init_session_id(line, Executor::Claude).is_none());
        assert!(init_session_id(r#"{"type":"turn.started"}"#, Executor::Codex).is_none());
    }

    #[test]
    fn prefers_the_result_line_over_assistant_blocks() {
        let text = text_buffer();
        capture_answer(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"thinking "}]}}"#,
            &text,
            Executor::Claude,
        );
        capture_answer(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"aloud"}]}}"#,
            &text,
            Executor::Claude,
        );
        assert_eq!(final_text(&text), "thinking aloud");
        capture_answer(
            r#"{"type":"result","result":"the plan"}"#,
            &text,
            Executor::Claude,
        );
        assert_eq!(final_text(&text), "the plan");
    }

    #[test]
    fn takes_the_last_codex_agent_message_as_the_answer() {
        let text = text_buffer();
        capture_answer(
            r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"working"}}"#,
            &text,
            Executor::Codex,
        );
        assert_eq!(final_text(&text), "working");
        capture_answer(
            r#"{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"the plan"}}"#,
            &text,
            Executor::Codex,
        );
        capture_answer(
            r#"{"type":"turn.completed","usage":{"input_tokens":9,"output_tokens":5}}"#,
            &text,
            Executor::Codex,
        );
        assert_eq!(final_text(&text), "the plan");
    }

    #[test]
    fn reads_the_cost_from_the_result_line() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":12,"total_cost_usd":0.0731,"result":"done"}"#;
        assert_eq!(result_cost(line, Executor::Claude), Some(0.0731));
        assert!(result_cost(r#"{"type":"assistant","message":{}}"#, Executor::Claude).is_none());
        assert!(result_cost(r#"{"type":"result","result":"done"}"#, Executor::Claude).is_none());
        assert!(result_cost("not json", Executor::Claude).is_none());
    }

    #[test]
    fn reads_a_codex_failure() {
        let item =
            r#"{"type":"item.completed","item":{"id":"i0","type":"error","message":"boom"}}"#;
        let value: Value = serde_json::from_str(item).unwrap();
        assert_eq!(codex_error(&value), Some("boom"));

        let turn = r#"{"type":"turn.failed","error":{"message":"usage limit"}}"#;
        let value: Value = serde_json::from_str(turn).unwrap();
        assert_eq!(codex_error(&value), Some("usage limit"));

        let said = r#"{"type":"item.completed","item":{"type":"agent_message","text":"hi"}}"#;
        let value: Value = serde_json::from_str(said).unwrap();
        assert!(codex_error(&value).is_none());
    }

    #[test]
    fn a_codex_run_reports_no_cost() {
        let line = r#"{"type":"turn.completed","usage":{"input_tokens":19262,"output_tokens":5}}"#;
        assert!(result_cost(line, Executor::Codex).is_none());
    }

    #[test]
    fn structures_text_and_bash_blocks_in_order() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"  Looking.  "},{"type":"tool_use","name":"Bash","input":{"command":"ls -la"}}]}}"#;
        assert_eq!(
            claude(line),
            vec![
                TaskEvent::Progress {
                    text: "Looking.".into()
                },
                TaskEvent::Command {
                    command: "ls -la".into()
                },
            ]
        );
    }

    #[test]
    fn structures_an_edit_as_a_changed_file() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"src/a.rs","old_string":"a"}}]}}"#;
        assert_eq!(
            claude(line),
            vec![TaskEvent::FileChanged {
                path: "src/a.rs".into()
            }]
        );
    }

    #[test]
    fn structures_nothing_from_other_lines() {
        assert!(claude(r#"{"type":"result","result":"done"}"#).is_empty());
        assert!(claude("Reading files...").is_empty());
        let read = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/a.rs"}}]}}"#;
        assert!(claude(read).is_empty());
    }

    #[test]
    fn structures_codex_items() {
        let command = r#"{"type":"item.completed","item":{"id":"i0","type":"command_execution","command":"ls -la"}}"#;
        assert_eq!(
            codex(command),
            vec![TaskEvent::Command {
                command: "ls -la".into()
            }]
        );
        let change = r#"{"type":"item.completed","item":{"id":"i1","type":"file_change","changes":[{"path":"src/a.rs","kind":"update"},{"path":"src/b.rs","kind":"add"}]}}"#;
        assert_eq!(
            codex(change),
            vec![
                TaskEvent::FileChanged {
                    path: "src/a.rs".into()
                },
                TaskEvent::FileChanged {
                    path: "src/b.rs".into()
                },
            ]
        );
        let said = r#"{"type":"item.completed","item":{"id":"i2","type":"agent_message","text":"  Looking.  "}}"#;
        assert_eq!(
            codex(said),
            vec![TaskEvent::Progress {
                text: "Looking.".into()
            }]
        );
    }

    #[test]
    fn structures_nothing_from_other_codex_lines() {
        assert!(codex(r#"{"type":"thread.started","thread_id":"a"}"#).is_empty());
        assert!(codex(r#"{"type":"turn.completed","usage":{}}"#).is_empty());
        assert!(codex(r#"{"type":"item.completed","item":{"type":"reasoning"}}"#).is_empty());
        assert!(codex("Reading files...").is_empty());
    }

    #[test]
    fn truncates_long_progress_text() {
        let line = format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"{}"}}]}}}}"#,
            "é".repeat(3000)
        );
        let [TaskEvent::Progress { text }] = &claude(&line)[..] else {
            panic!("expected one progress event");
        };
        assert_eq!(text.chars().count(), PROGRESS_MAX);
    }

    #[test]
    fn ignores_other_lines() {
        assert!(
            init_session_id(r#"{"type":"assistant","message":{}}"#, Executor::Claude).is_none()
        );
        assert!(init_session_id("Reading files...", Executor::Claude).is_none());
        assert!(
            init_session_id(r#"{"type":"system","subtype":"init"}"#, Executor::Claude).is_none()
        );
    }
}
