//! The webhook: one POST for every event a person would want to know about,
//! so Slack, email, or anything else can hang off it without a bespoke
//! integration here.

use std::sync::{Arc, OnceLock};

use lgtm_protocol::{attention, Task, TaskEvent};
use serde_json::{json, Value};

use crate::state::App;

fn payload(task: &Task, line: &str) -> Value {
    json!({
        "task_id": task.id,
        "status": task.status,
        "repository": task.spec.repository,
        "line": line,
    })
}

/// One client for the process: reqwest pools connections, and a fresh client
/// per event would throw the pool away every time.
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

/// Posts `task` and `event` to the configured webhook, if there is one and the
/// event is worth a person's time. Delivery is best effort: a webhook nobody
/// is listening on must not hold up the task it describes.
pub fn deliver(app: &Arc<App>, task: &Task, event: &TaskEvent) {
    let Some(url) = app.webhook.clone() else {
        return;
    };
    let Some(line) = attention(task, event) else {
        return;
    };
    let body = payload(task, &line);
    tokio::spawn(async move {
        match client().post(&url).json(&body).send().await {
            Ok(response) if !response.status().is_success() => {
                tracing::warn!(%url, status = %response.status(), "webhook refused the event");
            }
            Err(err) => tracing::warn!(%url, %err, "webhook delivery failed"),
            Ok(_) => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{Executor, TaskKind, TaskSpec, TaskStatus};

    fn awaiting_review() -> Task {
        Task {
            id: "0123abcd".into(),
            spec: TaskSpec {
                repository: "https://github.com/o/r.git".into(),
                base_branch: "main".into(),
                prompt: "add a /health endpoint".into(),
                executor: Executor::Claude,
                worker: None,
                issue: None,
                linear: None,
                kind: TaskKind::Run,
                parent: None,
                depends_on: Vec::new(),
                batch: None,
                sandbox: None,
                requirements: Vec::new(),
                goal: None,
            },
            status: TaskStatus::AwaitingReview,
            worker: None,
            created_at: 1,
            result: None,
            error: None,
            pull_request: None,
            ci: None,
            executions: Vec::new(),
            scratchpad: String::new(),
        }
    }

    #[test]
    fn the_payload_carries_the_task_its_status_and_the_line() {
        let task = awaiting_review();
        let body = payload(&task, "add a /health endpoint: ready for review");
        assert_eq!(body["task_id"], "0123abcd");
        assert_eq!(body["status"], "awaiting_review");
        assert_eq!(body["repository"], "https://github.com/o/r.git");
        assert_eq!(body["line"], "add a /health endpoint: ready for review");
    }
}
