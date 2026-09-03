//! `/api/chats`: conversations with the read-only workspace agent. A question
//! is stored at once and answered in the background, so a thread reads
//! complete from any screen, even one opened after the answer landed.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use lgtm_protocol::{Chat, ChatRole, ChatStep, ChatTurn};
use serde::Deserialize;

use super::{conflict, ApiError, AuthedUser};
use crate::state::{now_ms, person_turn, App};

fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "chat not found".into())
}

#[derive(Deserialize)]
pub(super) struct QuestionBody {
    question: String,
}

fn question(body: Result<Json<QuestionBody>, JsonRejection>) -> Result<String, ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let question = body.question.trim();
    if question.is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "question is required".into(),
        ));
    }
    Ok(question.to_string())
}

/// Refuses before anything is stored: a question nothing can answer should
/// not sit in a thread reading as still being worked on.
fn can_answer(app: &App) -> Result<(), ApiError> {
    if app.orchestrate.is_none() {
        return Err(conflict("--orchestrate is not configured".into()));
    }
    if app.asking.available_permits() == 0 {
        return Err(conflict(
            "too many questions at once; try again shortly".into(),
        ));
    }
    Ok(())
}

pub(super) async fn list_chats(State(app): State<Arc<App>>) -> Json<Vec<Chat>> {
    let state = app.state.lock().unwrap();
    let mut chats: Vec<Chat> = state
        .chats
        .values()
        .filter(|chat| state.in_workspace(chat.workspace.as_deref()))
        .cloned()
        .collect();
    chats.sort_by_key(|chat| std::cmp::Reverse(chat.created_at));
    Json(chats)
}

pub(super) async fn get_chat(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Chat>, ApiError> {
    let state = app.state.lock().unwrap();
    state
        .chats
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or_else(not_found)
}

pub(super) async fn create_chat(
    State(app): State<Arc<App>>,
    Extension(user): Extension<AuthedUser>,
    body: Result<Json<QuestionBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Chat>), ApiError> {
    let question = question(body)?;
    can_answer(&app)?;
    let chat = {
        let mut state = app.state.lock().unwrap();
        let chat = state.create_chat(question, user.0);
        app.persist_chat(&chat);
        chat
    };
    tokio::spawn(answer(app.clone(), chat.id.clone()));
    Ok((StatusCode::CREATED, Json(chat)))
}

pub(super) async fn ask_chat(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    body: Result<Json<QuestionBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Chat>), ApiError> {
    let question = question(body)?;
    can_answer(&app)?;
    let chat = {
        let mut state = app.state.lock().unwrap();
        let waiting = state
            .chats
            .get(&id)
            .ok_or_else(not_found)?
            .turns
            .last()
            .is_some_and(|turn| turn.role == ChatRole::Person);
        if waiting {
            return Err(conflict("the agent is still answering".into()));
        }
        let chat = state
            .push_chat_turn(&id, person_turn(question))
            .ok_or_else(not_found)?;
        app.persist_chat(&chat);
        chat
    };
    tokio::spawn(answer(app.clone(), id));
    Ok((StatusCode::ACCEPTED, Json(chat)))
}

/// The whole thread as the model reads it; the last turn is the question.
fn history_prompt(turns: &[ChatTurn]) -> String {
    let Some((last, earlier)) = turns.split_last() else {
        return String::new();
    };
    if earlier.is_empty() {
        return last.text.clone();
    }
    let transcript = earlier
        .iter()
        .map(|turn| {
            let who = match turn.role {
                ChatRole::Person => "Person",
                ChatRole::Agent => "Agent",
            };
            format!("{who}: {}", turn.text)
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "Earlier in this conversation:\n\n{transcript}\n\nNow the person asks:\n{}",
        last.text
    )
}

/// One bounded pass, then the agent's turn is stored whatever happened: a
/// thread with a question and no answer would read as still being worked on.
async fn answer(app: Arc<App>, id: String) {
    let Ok(_permit) = app.asking.try_acquire() else {
        finish(
            &app,
            &id,
            failed("too many questions at once; try again shortly".into()),
        );
        return;
    };
    let prompt = {
        let state = app.state.lock().unwrap();
        state.chats.get(&id).map(|chat| history_prompt(&chat.turns))
    };
    let Some(prompt) = prompt else {
        return;
    };
    let turn = match crate::orchestrate::answer_question(app.clone(), prompt).await {
        Ok(answered) => ChatTurn {
            role: ChatRole::Agent,
            text: answered.text,
            at: now_ms(),
            refs: answered.refs,
            steps: answered
                .steps
                .into_iter()
                .map(|step| ChatStep {
                    tool: step.tool,
                    detail: step.detail,
                })
                .collect(),
            worked_ms: answered.worked_ms,
            failed: false,
        },
        Err(note) => {
            tracing::warn!(chat = %id, %note, "chat answer failed");
            failed(note)
        }
    };
    finish(&app, &id, turn);
}

fn failed(note: String) -> ChatTurn {
    ChatTurn {
        role: ChatRole::Agent,
        text: note,
        at: now_ms(),
        refs: Vec::new(),
        steps: Vec::new(),
        worked_ms: 0,
        failed: true,
    }
}

fn finish(app: &App, id: &str, turn: ChatTurn) {
    let mut state = app.state.lock().unwrap();
    if let Some(chat) = state.push_chat_turn(id, turn) {
        app.persist_chat(&chat);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::State;

    #[test]
    fn a_chat_opens_with_the_question_as_its_first_turn_and_title() {
        let mut state = State::default();
        let chat = state.create_chat("What is running right now?".into(), Some("u1".into()));
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].role, ChatRole::Person);
        assert_eq!(chat.title, "What is running right now?");
        assert_eq!(chat.created_by.as_deref(), Some("u1"));
        assert!(state.chats.contains_key(&chat.id));
    }

    #[test]
    fn the_history_prompt_carries_earlier_turns_and_ends_on_the_question() {
        let turns = vec![person_turn("first".into())];
        assert_eq!(history_prompt(&turns), "first");

        let mut answered = failed("nope".into());
        answered.failed = false;
        let turns = vec![
            person_turn("first".into()),
            answered,
            person_turn("second".into()),
        ];
        let text = history_prompt(&turns);
        assert!(
            text.starts_with("Earlier in this conversation:\n\nPerson: first\n\nAgent: nope"),
            "{text}"
        );
        assert!(text.ends_with("Now the person asks:\nsecond"), "{text}");
    }
}
