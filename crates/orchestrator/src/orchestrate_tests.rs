//! Unit tests for `orchestrate.rs`: the prompt the loop starts from, the
//! command it is spawned with, and the `auto` pick. No model and no sockets.

use super::*;
use lgtm_protocol::{RunnerInfo, TaskId, TaskKind, TaskSpec};
use tokio::sync::mpsc;

use crate::state::{Conn, TaskRecord};

fn connect(state: &mut State) -> mpsc::UnboundedReceiver<lgtm_protocol::OrchestratorMessage> {
    let (tx, rx) = mpsc::unbounded_channel();
    state.runner_hello(
        RunnerInfo {
            name: "w".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            executors: vec![Executor::Claude],
            slots: 4,
            ephemeral: false,
            capabilities: Vec::new(),
            cpu_cores: 0,
            memory_mb: 0,
        },
        Vec::new(),
        Conn { tx, conn_id: 1 },
    );
    rx
}

fn spec(goal: Option<String>) -> TaskSpec {
    TaskSpec {
        repository: "https://example.com/repo.git".into(),
        base_branch: "main".into(),
        prompt: "do the thing\nin detail".into(),
        executor: Executor::Claude,
        runner: None,
        issue: None,
        linear: None,
        kind: TaskKind::Run,
        parent: None,
        depends_on: Vec::new(),
        depends_on_condition: Default::default(),
        batch: None,
        sandbox: None,
        requirements: Vec::new(),
        review_executor: None,
        model: None,
        goal,
        allowed_hosts: Vec::new(),
        session: None,
    }
}

/// A connected runner, a goal, and one task under it.
fn goal_task(state: &mut State) -> (String, TaskId) {
    let goal = state.create_goal(
        "ship the thing".into(),
        "https://example.com/repo.git".into(),
    );
    let task = state.create_task(spec(Some(goal.id.clone()))).unwrap().0;
    (goal.id, task.id)
}

#[test]
fn the_prompt_names_the_goal_the_subject_and_the_tools() {
    let mut state = State::default();
    let _runner = connect(&mut state);
    let (_goal, id) = goal_task(&mut state);
    state.apply_event(
        &id,
        TaskEvent::Failed {
            error: "boom".into(),
        },
    );

    let text = prompt(&build_context(&state, &id).expect("a context"));
    assert!(text.contains("ship the thing"), "{text}");
    assert!(
        text.contains(&format!("Task {id} just ended as failed")),
        "{text}"
    );
    assert!(text.contains("with: boom"), "{text}");
    assert!(text.contains("Use the lgtm tools"), "{text}");
    assert!(text.contains("`wait`"), "{text}");
}

#[test]
fn a_task_without_a_goal_has_no_context() {
    let mut state = State::default();
    let _runner = connect(&mut state);
    let task = state.create_task(spec(None)).unwrap().0;
    assert!(build_context(&state, &task.id).is_none());
    assert!(build_context(&state, "nothing").is_none());
    // A goal that was removed leaves its tasks alone too.
    let orphan = TaskRecord::new(
        Task {
            spec: spec(Some("gone".into())),
            ..task
        },
        Vec::new(),
    );
    let id = orphan.task.id.clone();
    state.tasks.insert(id.clone(), orphan);
    assert!(build_context(&state, &id).is_none());
}

#[test]
fn both_executors_get_the_lgtm_tools_and_a_turn_ceiling() {
    let exe = Path::new("/usr/local/bin/lgtm");
    let claude = args(Executor::Claude, "go", exe);
    assert_eq!(claude[..2], ["-p", "go"]);
    assert!(claude.contains(&"mcp__lgtm__*".to_string()), "{claude:?}");
    assert_eq!(
        claude[claude.len() - 3..claude.len() - 1],
        ["default", "--mcp-config"]
    );
    assert_eq!(
        serde_json::from_str::<Value>(claude.last().unwrap()).unwrap(),
        json!({"mcpServers": {"lgtm": {"command": "/usr/local/bin/lgtm", "args": ["mcp"]}}})
    );
    let turns = claude.iter().position(|a| a == "--max-turns").unwrap();
    assert_eq!(claude[turns + 1], MAX_TURNS);

    let codex = args(Executor::Codex, "go", exe);
    assert_eq!(codex[..4], ["exec", "--json", "--sandbox", "read-only"]);
    assert!(
        codex.contains(&"mcp_servers.lgtm.command=\"/usr/local/bin/lgtm\"".to_string()),
        "{codex:?}"
    );
    assert_eq!(codex.last().unwrap(), "go");
}

#[test]
fn auto_prefers_claude_and_falls_back_to_codex() {
    assert_eq!(resolve(Choice::Auto, |_| true), Some(Executor::Claude));
    assert_eq!(
        resolve(Choice::Auto, |bin| bin == "codex"),
        Some(Executor::Codex)
    );
    assert_eq!(resolve(Choice::Auto, |_| false), None);
    // A named executor is taken whether or not this machine has it.
    assert_eq!(
        resolve(Choice::One(Executor::Codex), |_| false),
        Some(Executor::Codex)
    );
}

#[test]
fn reads_the_answer_out_of_each_executor() {
    let claude = r#"{"type":"result","result":"created task ab12"}"#;
    assert_eq!(
        answer(Executor::Claude, claude).unwrap(),
        "created task ab12"
    );
    assert!(answer(Executor::Claude, "not json").is_none());

    let codex = "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"first\"}}\n{\"type\":\"agent_message\",\"message\":\"second\"}\n";
    assert_eq!(answer(Executor::Codex, codex).unwrap(), "first\nsecond");
    assert!(answer(Executor::Codex, "{\"type\":\"token_count\"}").is_none());
}
