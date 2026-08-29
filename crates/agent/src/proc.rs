//! Pumping an executor's output to the orchestrator, and what its stream-json
//! lines carry: the session id and the agent's final answer.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use lgtm_protocol::{OutputStream, TaskEvent, TaskId};
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
    /// Where to write the session id from the init line.
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
    pub sinks: Sinks,
}

impl Pump {
    pub async fn run<R: AsyncRead + Unpin>(mut self, reader: R) {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            self.record(&line).await;
            let stream = self.stream;
            self.ctx
                .emit(&self.task_id, TaskEvent::Output { stream, line });
        }
    }

    async fn record(&mut self, line: &str) {
        let sinks = &mut self.sinks;
        if let Some(text) = &sinks.text {
            capture_answer(line, text);
        }
        if let (Some(cost), Some(spent)) = (&sinks.cost, result_cost(line)) {
            *cost.lock().expect("cost poisoned") += spent;
        }
        if let (Some(path), Some(id)) = (&sinks.session, init_session_id(line)) {
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

fn capture_answer(line: &str, text: &Text) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    match value.get("type").and_then(serde_json::Value::as_str) {
        Some("result") => {
            if let Some(result) = value.get("result").and_then(serde_json::Value::as_str) {
                text.lock().expect("answer poisoned").result = Some(result.to_string());
            }
        }
        Some("assistant") => {
            let blocks = value
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(serde_json::Value::as_array);
            for block in blocks.into_iter().flatten() {
                if let Some(part) = block.get("text").and_then(serde_json::Value::as_str) {
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

/// What the run cost, from Claude's stream-json `result` line. Every other
/// line is ignored, and so is a result line without a cost.
pub fn result_cost(line: &str) -> Option<f64> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value.get("type")?.as_str()? != "result" {
        return None;
    }
    value.get("total_cost_usd")?.as_f64()
}

/// The `session_id` of Claude's stream-json init line, so a follow-up can
/// `--resume` it. Every other line is ignored.
pub fn init_session_id(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value.get("type")?.as_str()? != "system" || value.get("subtype")?.as_str()? != "init" {
        return None;
    }
    Some(value.get("session_id")?.as_str()?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_session_id_from_the_init_line() {
        let line =
            r#"{"type":"system","subtype":"init","cwd":"/x","session_id":"abc-123","tools":[]}"#;
        assert_eq!(init_session_id(line).as_deref(), Some("abc-123"));
    }

    #[test]
    fn prefers_the_result_line_over_assistant_blocks() {
        let text = text_buffer();
        capture_answer(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"thinking "}]}}"#,
            &text,
        );
        capture_answer(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"aloud"}]}}"#,
            &text,
        );
        assert_eq!(final_text(&text), "thinking aloud");
        capture_answer(r#"{"type":"result","result":"the plan"}"#, &text);
        assert_eq!(final_text(&text), "the plan");
    }

    #[test]
    fn reads_the_cost_from_the_result_line() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":12,"total_cost_usd":0.0731,"result":"done"}"#;
        assert_eq!(result_cost(line), Some(0.0731));
        assert!(result_cost(r#"{"type":"assistant","message":{}}"#).is_none());
        assert!(result_cost(r#"{"type":"result","result":"done"}"#).is_none());
        assert!(result_cost("not json").is_none());
    }

    #[test]
    fn ignores_other_lines() {
        assert!(init_session_id(r#"{"type":"assistant","message":{}}"#).is_none());
        assert!(init_session_id("Reading files...").is_none());
        assert!(init_session_id(r#"{"type":"system","subtype":"init"}"#).is_none());
    }
}
