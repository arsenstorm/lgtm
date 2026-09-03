//! `/api/tasks/{id}/artefacts`: the files a task's runs left for the reviewer.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header::CONTENT_TYPE, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use lgtm_protocol::{artefact_name, Artefact, StoredEvent, TaskEvent};
use tokio::sync::oneshot;

use super::ApiError;
use crate::persist::Persist;
use crate::state::App;

pub(super) async fn list(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Artefact>>, ApiError> {
    let state = app.state.lock().unwrap();
    let rec = state.tasks.get(&id).ok_or(not_found("task not found"))?;
    Ok(Json(listed(&rec.events)))
}

/// One entry per name, the size the latest event reported: a run that keeps
/// overwriting a screenshot has one artefact, not one per run.
fn listed(events: &[StoredEvent]) -> Vec<Artefact> {
    let mut out: Vec<Artefact> = Vec::new();
    for stored in events {
        let TaskEvent::Artefact { name, size, .. } = &stored.event else {
            continue;
        };
        match out.iter_mut().find(|found| found.name == *name) {
            Some(found) => found.size = *size,
            None => out.push(Artefact {
                name: name.clone(),
                size: *size,
            }),
        }
    }
    out
}

pub(super) async fn get(
    State(app): State<Arc<App>>,
    Path((id, name)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    if artefact_name(&name).as_deref() != Some(name.as_str()) {
        return Err(not_found("artefact not found"));
    }
    let (reply, answer) = oneshot::channel();
    let _ = app.persist.send(Persist::ReadArtefact {
        task_id: id,
        name: name.clone(),
        reply,
    });
    let bytes = answer
        .await
        .ok()
        .flatten()
        .ok_or(not_found("artefact not found"))?;
    Ok(([(CONTENT_TYPE, content_type(&name))], bytes).into_response())
}

fn not_found(msg: &str) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, msg.into())
}

/// Guessed from the extension; anything else is offered as a download rather
/// than guessed at.
fn content_type(name: &str) -> &'static str {
    match name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
    {
        Some(ext) => match ext.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            "txt" => "text/plain; charset=utf-8",
            "json" => "application/json",
            "md" => "text/markdown; charset=utf-8",
            "html" => "text/html; charset=utf-8",
            _ => "application/octet-stream",
        },
        None => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artefact(name: &str, size: usize) -> StoredEvent {
        StoredEvent {
            at: 0,
            event: TaskEvent::Artefact {
                name: name.into(),
                size,
                bytes_base64: String::new(),
            },
        }
    }

    #[test]
    fn the_listing_keeps_the_latest_size_per_name() {
        let events = vec![
            artefact("a.png", 1),
            artefact("b.png", 2),
            artefact("a.png", 30),
        ];
        assert_eq!(
            listed(&events),
            vec![
                Artefact {
                    name: "a.png".into(),
                    size: 30
                },
                Artefact {
                    name: "b.png".into(),
                    size: 2
                },
            ]
        );
    }

    /// No writer is listening, so every read comes back empty: what is left
    /// is whether the handler turns that into a 404 rather than a hang.
    fn app() -> Arc<App> {
        let (persist, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(rx);
        Arc::new(App {
            token: "tok".into(),
            state: std::sync::Mutex::new(crate::state::State::default()),
            persist,
            github: None,
            linear: None,
            webhook: None,
            orchestrate: None,
            base_url: "http://127.0.0.1:1".into(),
            orchestrating: std::sync::Mutex::new(Default::default()),
            asking: tokio::sync::Semaphore::new(crate::ASK_SLOTS),
            inferring: std::sync::Mutex::new(Default::default()),
        })
    }

    #[tokio::test]
    async fn what_is_not_there_is_not_found() {
        let app = app();
        let name = |name: &str| Path(("0123abcd".to_string(), name.to_string()));

        let missing = get(State(app.clone()), name("gone.png")).await;
        let unsafe_name = get(State(app.clone()), name("../gone.png")).await;
        let no_task = list(State(app), Path("0123abcd".to_string())).await;

        assert_eq!(missing.err().unwrap().0, StatusCode::NOT_FOUND);
        assert_eq!(unsafe_name.err().unwrap().0, StatusCode::NOT_FOUND);
        assert_eq!(no_task.err().unwrap().0, StatusCode::NOT_FOUND);
    }

    #[test]
    fn unknown_extensions_are_downloads() {
        assert_eq!(content_type("a.PNG"), "image/png");
        assert_eq!(content_type("a.bin"), "application/octet-stream");
        assert_eq!(content_type("noext"), "application/octet-stream");
    }
}
