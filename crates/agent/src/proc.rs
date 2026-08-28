//! Pumping an executor's output to the orchestrator, and the session id its
//! first line carries.

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

pub async fn pump<R: AsyncRead + Unpin>(
    reader: R,
    stream: OutputStream,
    ctx: Arc<Ctx>,
    task_id: TaskId,
    tail: Option<Tail>,
    session: Option<PathBuf>,
) {
    let mut session = session;
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(path) = &session {
            if let Some(id) = init_session_id(&line) {
                if let Err(err) = tokio::fs::write(path, &id).await {
                    tracing::warn!("write {}: {err}", path.display());
                }
                session = None;
            }
        }
        if let Some(tail) = &tail {
            let mut tail = tail.lock().expect("tail poisoned");
            tail.push_back(line.clone());
            if tail.len() > TAIL_LINES {
                tail.pop_front();
            }
        }
        ctx.emit(&task_id, TaskEvent::Output { stream, line });
    }
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
    fn ignores_other_lines() {
        assert!(init_session_id(r#"{"type":"assistant","message":{}}"#).is_none());
        assert!(init_session_id("Reading files...").is_none());
        assert!(init_session_id(r#"{"type":"system","subtype":"init"}"#).is_none());
    }
}
