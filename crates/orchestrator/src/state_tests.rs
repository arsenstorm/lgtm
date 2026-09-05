//! Unit tests for `state.rs`: pure transitions, no sockets and no files.

use super::*;
use crate::commands::RetryInto;
use lgtm_protocol::{
    DependsOn, Execution, ExecutionStatus, Executor, IssueRef, LinearRef, Plan, PlanStep, Policy,
    PullRequest, RunnerInfo, TaskKind, TaskResult,
};

fn info(name: &str, slots: u32, executors: Vec<Executor>) -> RunnerInfo {
    RunnerInfo {
        name: name.to_string(),
        os: "linux".into(),
        arch: "x86_64".into(),
        executors,
        slots,
        ephemeral: false,
        capabilities: Vec::new(),
        cpu_cores: 0,
        memory_mb: 0,
    }
}

/// Connects a runner over the same path `Hello` uses. The receiver comes back
/// so the caller can keep it alive and read what the runner was sent.
pub(crate) fn connect(
    state: &mut State,
    name: &str,
    slots: u32,
    conn_id: u64,
) -> mpsc::UnboundedReceiver<OrchestratorMessage> {
    let (tx, rx) = mpsc::unbounded_channel();
    state.runner_hello(
        info(name, slots, vec![Executor::Claude]),
        Vec::new(),
        Conn { tx, conn_id },
    );
    rx
}

fn spec(executor: Executor, runner: Option<&str>) -> TaskSpec {
    TaskSpec {
        repository: "https://example.com/repo.git".into(),
        base_branch: "main".into(),
        prompt: "do the thing".into(),
        executor,
        runner: runner.map(str::to_string),
        issue: None,
        linear: None,
        kind: TaskKind::Run,
        parent: None,
        depends_on: Vec::new(),
        depends_on_condition: Default::default(),
        batch: None,
        sandbox: None,
        requirements: Vec::new(),
        goal: None,
        review_executor: None,
        model: None,
        reasoning_effort: None,
        allowed_hosts: Vec::new(),
        created_by: None,
    }
}

pub(crate) fn create(state: &mut State, executor: Executor) -> Task {
    state.create_task(spec(executor, None)).unwrap().0
}

#[test]
fn a_created_task_carries_the_workspace_when_set() {
    let mut state = State {
        workspace: Some("acme".into()),
        queue_without_runners: true,
        ..State::default()
    };
    let task = create(&mut state, Executor::Claude);
    assert_eq!(task.workspace.as_deref(), Some("acme"));
}

fn status(state: &State, id: &str) -> TaskStatus {
    state.tasks[id].task.status
}

fn finished_execution(runner: &str, started_at: u64, finished_at: u64) -> Execution {
    Execution {
        attempt: 1,
        runner: runner.into(),
        executor: Executor::Claude,
        model: None,
        artefacts: Vec::new(),
        started_at,
        finished_at: Some(finished_at),
        status: ExecutionStatus::Completed,
        error: None,
        cost_usd: 0.0,
        validation: Vec::new(),
        skills: Vec::new(),
    }
}

/// Records a past task's executions without scheduling it, so `median_for`
/// and `candidate` have history to read without a runner's slot being spent.
fn add_history(state: &mut State, executions: Vec<Execution>) {
    let task = Task {
        id: state.new_id(),
        title: None,
        spec: spec(Executor::Claude, None),
        status: TaskStatus::Merged,
        runner: None,
        created_at: now_ms(),
        result: None,
        error: None,
        pull_request: None,
        ci: None,
        pr_review: None,
        executions,
        scratchpad: String::new(),
        files: Vec::new(),
        workspace: None,
        created_by: None,
        archived: false,
    };
    state
        .tasks
        .insert(task.id.clone(), TaskRecord::new(task, Vec::new()));
}

#[test]
fn refuses_tasks_no_runner_could_run() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let (tx, _codex) = mpsc::unbounded_channel();
    state.runner_hello(
        info("codexonly", 1, vec![Executor::Codex]),
        Vec::new(),
        Conn { tx, conn_id: 2 },
    );

    assert!(state.check_eligible(&spec(Executor::Claude, None)).is_ok());
    assert_eq!(state.candidate(&spec(Executor::Claude, None)).unwrap(), "a");
    // The only Codex runner wins despite the Claude runner being idle.
    assert_eq!(
        state.candidate(&spec(Executor::Codex, None)).unwrap(),
        "codexonly"
    );
    assert_eq!(
        state
            .check_eligible(&spec(Executor::Claude, Some("codexonly")))
            .unwrap_err(),
        "runner codexonly does not have claude"
    );
    assert_eq!(
        state
            .check_eligible(&spec(Executor::Claude, Some("ghost")))
            .unwrap_err(),
        "runner ghost is not connected"
    );
    state.runners.clear();
    assert_eq!(
        state
            .check_eligible(&spec(Executor::Claude, None))
            .unwrap_err(),
        "no eligible runner"
    );
}

#[test]
fn fifo_assignment_respects_slots() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let _b = connect(&mut state, "b", 2, 2);

    let one = create(&mut state, Executor::Claude);
    let two = create(&mut state, Executor::Claude);
    let three = create(&mut state, Executor::Claude);
    let four = create(&mut state, Executor::Claude);

    assert_eq!(
        one.runner.as_deref(),
        Some("b"),
        "b has the most free slots"
    );
    assert_eq!(
        two.runner.as_deref(),
        Some("a"),
        "one free each, a sorts first"
    );
    assert_eq!(three.runner.as_deref(), Some("b"));
    assert_eq!(four.runner, None);
    assert_eq!(status(&state, &four.id), TaskStatus::Queued);
    assert_eq!(state.runners["a"].running.len(), 1);
    assert_eq!(state.runners["b"].running.len(), 2);
}

#[test]
fn queued_task_assigned_on_connect() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    create(&mut state, Executor::Claude);
    let queued = create(&mut state, Executor::Claude);
    assert_eq!(queued.runner, None);

    let mut b = connect(&mut state, "b", 2, 2);
    assert_eq!(
        state.tasks[&queued.id].task.runner.as_deref(),
        Some("b"),
        "the new runner picks up the backlog"
    );
    assert!(matches!(
        b.try_recv().unwrap(),
        OrchestratorMessage::Start { task, .. } if task.id == queued.id
    ));
}

#[test]
fn reconnect_within_grace_keeps_tasks() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let task = create(&mut state, Executor::Claude);
    state.apply_event(
        &task.id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );

    let stale = state.disconnect("a", 1).unwrap();
    assert!(!state.runners["a"].is_connected());
    assert_eq!(status(&state, &task.id), TaskStatus::Running);

    let (tx, _rx) = mpsc::unbounded_channel();
    state.runner_hello(
        info("a", 1, vec![Executor::Claude]),
        vec![task.id.clone()],
        Conn { tx, conn_id: 2 },
    );
    assert_eq!(status(&state, &task.id), TaskStatus::Running);
    assert!(state.runners["a"].running.contains(&task.id));

    assert_eq!(
        state.expire_runner("a", stale),
        None,
        "the old timer is a no-op"
    );
    assert!(state.runners.contains_key("a"));
    assert_eq!(status(&state, &task.id), TaskStatus::Running);
}

#[test]
fn reconnect_missing_task_is_lost_and_reassigned_back_onto_the_only_runner() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let task = create(&mut state, Executor::Claude);
    state.apply_event(
        &task.id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.disconnect("a", 1).unwrap();

    let (tx, _rx) = mpsc::unbounded_channel();
    let changed = state.runner_hello(
        info("a", 1, vec![Executor::Claude]),
        Vec::new(),
        Conn { tx, conn_id: 2 },
    );
    assert!(changed.contains(&task.id));
    assert!(
        state.tasks[&task.id]
            .events
            .iter()
            .any(|stored| matches!(stored.event, TaskEvent::RunnerLost)),
        "the loss is still recorded even though it did not stick"
    );
    assert_eq!(
        status(&state, &task.id),
        TaskStatus::Queued,
        "no other runner to move to: reassigned right back onto this one"
    );
    assert!(state.tasks[&task.id].task.error.is_none());
    assert!(state.runners["a"].running.contains(&task.id));
}

#[test]
fn grace_expiry_loses_tasks_and_their_dependents() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let task = create(&mut state, Executor::Claude);
    state.apply_event(
        &task.id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    let mut waiting = spec(Executor::Claude, None);
    waiting.depends_on = vec![task.id.clone()];
    let waiting = state.create_task(waiting).unwrap().0;

    let generation = state.disconnect("a", 1).unwrap();
    let changed = state.expire_runner("a", generation).unwrap();
    assert!(changed.contains(&task.id) && changed.contains(&waiting.id));
    assert_eq!(status(&state, &task.id), TaskStatus::RunnerLost);
    assert!(state.tasks[&task.id].task.error.is_none());
    assert_eq!(status(&state, &waiting.id), TaskStatus::Failed);
    assert_eq!(
        state.tasks[&waiting.id].task.error.as_deref(),
        Some(format!("dependency {} failed", task.id).as_str())
    );
    assert!(!state.runners.contains_key("a"));
}

#[test]
fn provisioning_queues_tasks_with_no_runner() {
    let mut state = State::default();
    assert_eq!(
        state.create_task(spec(Executor::Claude, None)).unwrap_err(),
        "no eligible runner"
    );

    state.queue_without_runners = true;
    let (task, _) = state.create_task(spec(Executor::Claude, None)).unwrap();
    assert_eq!(task.status, TaskStatus::Queued);
    assert!(task.runner.is_none());
    assert!(crate::provision::needs_provision(&state, 1, false));

    // An explicit runner is still refused; provisioning cannot conjure a name.
    assert_eq!(
        state
            .create_task(spec(Executor::Claude, Some("ghost")))
            .unwrap_err(),
        "runner ghost is not connected"
    );
}

#[test]
fn goodbye_removes_runner_at_once() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);

    assert!(state.runner_goodbye("a", 99).is_empty());
    assert!(
        state.runners.contains_key("a"),
        "a stale socket says nothing"
    );

    assert!(state.runner_goodbye("a", 1).is_empty());
    assert!(!state.runners.contains_key("a"));
    // No grace timer is left to fire for it.
    assert!(state.disconnect("a", 1).is_none());
}

#[test]
fn cancel_queued_task() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    create(&mut state, Executor::Claude);
    let queued = create(&mut state, Executor::Claude);

    let cancelled = state.cancel(&queued.id).unwrap();
    assert_eq!(cancelled.status, TaskStatus::Cancelled);
    assert_eq!(status(&state, &queued.id), TaskStatus::Cancelled);
}

/// The event log is text a person reads: the payload comes off the event and
/// only the writer ever holds the bytes.
#[test]
fn an_artefact_leaves_its_bytes_behind() {
    let mut state = State::default();
    let _idle = connect(&mut state, "idle", 1, 1);
    let id = create(&mut state, Executor::Claude).id;

    state.apply_event(
        &id,
        TaskEvent::Artefact {
            name: "shot.png".into(),
            size: 3,
            bytes_base64: "TWFu".into(),
        },
    );

    let rec = &state.tasks[&id];
    let TaskEvent::Artefact {
        bytes_base64, size, ..
    } = &rec.events.last().unwrap().event
    else {
        panic!("not an artefact");
    };
    assert!(bytes_base64.is_empty());
    assert_eq!(*size, 3);
    assert_eq!(
        rec.artefacts,
        vec![("shot.png".to_string(), b"Man".to_vec())]
    );

    let dir = std::env::temp_dir().join(format!("lgtm-artefact-test-{}", now_ms()));
    crate::persist::write_artefact(&dir, &id, "shot.png", &rec.artefacts[0].1);
    assert!(dir.join(&id).join("shot.png").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn interrupt_routes_to_the_running_task_runner() {
    let mut state = State::default();
    let mut a = connect(&mut state, "a", 1, 1);
    let id = create(&mut state, Executor::Claude).id;
    state.apply_event(
        &id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    assert_eq!(status(&state, &id), TaskStatus::Running);

    let task = state.interrupt(&id).unwrap();
    assert_eq!(task.status, TaskStatus::Running, "not a status of its own");
    let sent: Vec<OrchestratorMessage> = std::iter::from_fn(|| a.try_recv().ok()).collect();
    assert!(sent
        .iter()
        .any(|msg| matches!(msg, OrchestratorMessage::Interrupt { task_id } if task_id == &id)));
}

#[test]
fn interrupt_refuses_a_task_that_is_not_running() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let queued = create(&mut state, Executor::Claude);

    assert!(matches!(
        state.interrupt(&queued.id),
        Err(CmdError::Conflict(_))
    ));
}

#[test]
fn apply_event_transitions() {
    let mut state = State::default();
    let _idle = connect(&mut state, "idle", 1, 1);
    let id = create(&mut state, Executor::Claude).id;
    assert!(state.runners["idle"].running.contains(&id));

    state.apply_event(
        &id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    assert_eq!(status(&state, &id), TaskStatus::Running);

    let result = TaskResult {
        branch: format!("lgtm/{id}"),
        diff: "diff".into(),
        changed_files: vec!["a.rs".into()],
        validation: Vec::new(),
        plan: None,
        review: None,
        policy: None,
        cost_usd: 0.0,
    };
    state.apply_event(&id, TaskEvent::Completed { result });
    assert_eq!(status(&state, &id), TaskStatus::AwaitingReview);
    assert!(state.tasks[&id].task.result.is_some());
    assert!(state.runners["idle"].running.is_empty());

    state.apply_event(
        &id,
        TaskEvent::Pushed {
            branch: format!("lgtm/{id}"),
            sha: "deadbeef".into(),
        },
    );
    assert_eq!(status(&state, &id), TaskStatus::Approved);
    assert_eq!(state.tasks[&id].events.len(), 3);
    assert_eq!(state.tasks[&id].pushed_sha().as_deref(), Some("deadbeef"));
}

#[test]
fn file_changed_collects_this_attempts_files_once_each() {
    let mut state = State::default();
    let _idle = connect(&mut state, "idle", 1, 1);
    let id = create(&mut state, Executor::Claude).id;

    state.apply_event(
        &id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    for path in ["a.rs", "b.rs", "a.rs"] {
        state.apply_event(&id, TaskEvent::FileChanged { path: path.into() });
    }
    assert_eq!(state.tasks[&id].task.files, vec!["a.rs", "b.rs"]);

    state.apply_event(
        &id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    assert!(state.tasks[&id].task.files.is_empty());
}

/// Drives a task all the way to `Approved` on a GitHub repository.
fn approved(state: &mut State, issue: Option<u64>, sha: &str) -> TaskId {
    let mut spec = spec(Executor::Claude, None);
    spec.repository = "https://github.com/arsenstorm/lgtm.git".into();
    spec.prompt =
        "add a /health endpoint that reports the build sha and the uptime\n\ndetails".into();
    spec.issue = issue.map(|number| IssueRef {
        owner: "arsenstorm".into(),
        repo: "lgtm".into(),
        number,
    });
    let id = state.create_task(spec).unwrap().0.id;
    state.apply_event(
        &id,
        TaskEvent::Pushed {
            branch: format!("lgtm/{id}"),
            sha: sha.into(),
        },
    );
    id
}

#[test]
fn pull_request_plan_needs_github_and_approval() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let id = approved(&mut state, Some(7), "cafe1234");

    let plan = state.pull_request_plan(&id, true).unwrap();
    assert_eq!(plan.pull.repo.owner, "arsenstorm");
    assert_eq!(plan.pull.repo.repo, "lgtm");
    assert_eq!(plan.pull.head, format!("lgtm/{id}"));
    assert_eq!(plan.pull.base, "main");
    assert_eq!(plan.sha, "cafe1234");
    assert!(plan.pull.title.chars().count() <= 72, "{}", plan.pull.title);
    assert!(!plan.pull.title.contains('\n'));
    assert!(plan.pull.body.contains("Closes #7"));
    assert!(plan
        .pull
        .body
        .contains(&format!("Created by LGTM task {id}")));

    assert!(state.pull_request_plan(&id, false).is_none(), "github off");
    assert!(state.pull_request_plan("nope", true).is_none());

    // A task without an issue says nothing about closing one.
    let plain = approved(&mut state, None, "beef");
    let plan = state.pull_request_plan(&plain, true).unwrap();
    assert!(!plan.pull.body.contains("Closes"));

    // Not approved yet, and not on GitHub, are both nothing to open.
    let running = create(&mut state, Executor::Claude).id;
    state.apply_event(
        &running,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    assert_eq!(status(&state, &running), TaskStatus::Running);
    assert!(state.pull_request_plan(&running, true).is_none());
    let elsewhere = create(&mut state, Executor::Claude).id;
    state.apply_event(
        &elsewhere,
        TaskEvent::Pushed {
            branch: "b".into(),
            sha: "abc".into(),
        },
    );
    assert_eq!(status(&state, &elsewhere), TaskStatus::Approved);
    assert!(
        state.pull_request_plan(&elsewhere, true).is_none(),
        "example.com is not GitHub"
    );
}

#[test]
fn merge_only_from_approved() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let id = approved(&mut state, None, "cafe1234");

    let merged = state.mark_merged(&id).unwrap().0;
    assert_eq!(merged.status, TaskStatus::Merged);
    assert_eq!(status(&state, &id), TaskStatus::Merged);
    assert!(TaskStatus::Merged.is_terminal());

    // Merged is terminal, so nothing re-opens it.
    assert!(matches!(
        state.mark_merged(&id),
        Err(CmdError::Conflict(msg)) if msg == "task is not approved"
    ));
    assert!(matches!(state.mark_merged("nope"), Err(CmdError::NotFound)));
    assert!(
        state.pull_request_plan(&id, true).is_none(),
        "a merged task is not approved any more"
    );

    state.apply_event(
        &id,
        TaskEvent::Pushed {
            branch: "b".into(),
            sha: "later".into(),
        },
    );
    assert_eq!(status(&state, &id), TaskStatus::Merged);
}

#[test]
fn message_requires_awaiting_review() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let id = create(&mut state, Executor::Claude).id;
    state.apply_event(
        &id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    assert_eq!(status(&state, &id), TaskStatus::Running);

    assert!(matches!(
        state.message(&id, "too soon".into(), None),
        Err(CmdError::Conflict(_))
    ));

    let result = TaskResult {
        branch: format!("lgtm/{id}"),
        diff: "diff".into(),
        changed_files: vec!["a.rs".into()],
        validation: Vec::new(),
        plan: None,
        review: None,
        policy: None,
        cost_usd: 0.0,
    };
    state.apply_event(
        &id,
        TaskEvent::Completed {
            result: result.clone(),
        },
    );
    assert_eq!(status(&state, &id), TaskStatus::AwaitingReview);
    assert!(
        state.runners["a"].running.is_empty(),
        "slot freed on completion"
    );

    let (task, changed) = state.message(&id, "keep going".into(), None).unwrap();
    assert_eq!(task.status, TaskStatus::ChangesRequested);
    assert!(changed.contains(&id));
    match &state.tasks[&id].events.last().unwrap().event {
        TaskEvent::Message { text, .. } => assert_eq!(text.as_str(), "keep going"),
        other => panic!("expected a Message event, got {other:?}"),
    }
    assert!(
        state.runners["a"].running.contains(&id),
        "slot taken again for the follow-up"
    );

    state.apply_event(
        &id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    assert_eq!(status(&state, &id), TaskStatus::Running);

    state.apply_event(&id, TaskEvent::Completed { result });
    assert_eq!(status(&state, &id), TaskStatus::AwaitingReview);
    assert!(
        state.runners["a"].running.is_empty(),
        "slot freed again after the follow-up run"
    );
}

#[test]
fn allow_host_adds_once_and_is_idempotent() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let id = create(&mut state, Executor::Claude).id;

    let (task, changed) = state.allow_host(&id, "registry.internal".into()).unwrap();
    assert_eq!(task.spec.allowed_hosts, vec!["registry.internal"]);
    assert!(changed.contains(&id));
    assert!(matches!(
        state.tasks[&id].events.last().unwrap().event,
        TaskEvent::HostAllowed { ref host } if host == "registry.internal"
    ));

    // Already granted: a no-op, not a second event.
    let before = state.tasks[&id].events.len();
    let (task, _) = state.allow_host(&id, "registry.internal".into()).unwrap();
    assert_eq!(task.spec.allowed_hosts, vec!["registry.internal"]);
    assert_eq!(state.tasks[&id].events.len(), before);
}

/// A follow-up carries the orchestrator's current task, so the runner's
/// stale on-disk copy doesn't shadow a host allowed since the last run.
#[test]
fn a_follow_up_carries_the_current_spec() {
    let mut state = State::default();
    let mut rx = connect(&mut state, "a", 1, 1);
    let id = create(&mut state, Executor::Claude).id;
    state.allow_host(&id, "registry.internal".into()).unwrap();
    state.apply_event(
        &id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        &id,
        TaskEvent::Completed {
            result: TaskResult {
                branch: format!("lgtm/{id}"),
                diff: "diff".into(),
                changed_files: vec!["a.rs".into()],
                validation: Vec::new(),
                plan: None,
                review: None,
                policy: None,
                cost_usd: 0.0,
            },
        },
    );
    state.message(&id, "keep going".into(), None).unwrap();
    let sent = std::iter::from_fn(|| rx.try_recv().ok())
        .find_map(|msg| match msg {
            OrchestratorMessage::Message { task, .. } => task,
            _ => None,
        })
        .expect("a Message with a task");
    assert_eq!(sent.spec.allowed_hosts, vec!["registry.internal"]);
}

/// The last follow-up the runner was sent, ignoring everything else on the
/// socket.
fn last_message(rx: &mut mpsc::UnboundedReceiver<OrchestratorMessage>) -> Option<String> {
    let mut last = None;
    while let Ok(msg) = rx.try_recv() {
        if let OrchestratorMessage::Message { text, .. } = msg {
            last = Some(text);
        }
    }
    last
}

#[test]
fn a_conflict_becomes_work_for_the_agent() {
    let mut state = State::default();
    let mut a = connect(&mut state, "a", 1, 1);
    let id = create(&mut state, Executor::Claude).id;
    state.apply_event(
        &id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        &id,
        TaskEvent::Completed {
            result: TaskResult {
                branch: format!("lgtm/{id}"),
                diff: "diff".into(),
                changed_files: vec!["a.rs".into()],
                validation: Vec::new(),
                plan: None,
                review: None,
                policy: None,
                cost_usd: 0.0,
            },
        },
    );
    state.apply_event(
        &id,
        TaskEvent::Conflicted {
            base: "main".into(),
            files: vec!["a.rs".into(), "b.rs".into()],
        },
    );
    assert_eq!(status(&state, &id), TaskStatus::Conflicted);
    assert!(
        state.runners["a"].running.is_empty(),
        "a push takes no slot to free"
    );

    let (task, changed) = state.message(&id, "keep going".into(), None).unwrap();
    assert_eq!(task.status, TaskStatus::ChangesRequested);
    assert!(changed.contains(&id));
    assert_eq!(
        last_message(&mut a).unwrap(),
        "The branch conflicts with main on: a.rs, b.rs. Rebase onto origin/main, \
         resolve the conflicts, finish the rebase, then continue with: keep going"
    );
}

/// A retry that changes nothing about where the task runs.
fn same_place() -> RetryInto {
    RetryInto {
        runner: None,
        executor: None,
    }
}

#[test]
fn retry_queues_a_failed_task_as_a_second_attempt() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let id = create(&mut state, Executor::Claude).id;
    state.apply_event(
        &id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        &id,
        TaskEvent::Failed {
            error: "boom".into(),
        },
    );
    assert_eq!(status(&state, &id), TaskStatus::Failed);

    let (task, changed) = state.retry(&id, same_place()).unwrap();
    assert!(changed.contains(&id));
    assert_eq!(task.status, TaskStatus::Queued);
    assert_eq!(task.runner.as_deref(), Some("a"), "scheduled again");
    assert!(task.error.is_none());
    assert!(matches!(
        state.tasks[&id].events.last().unwrap().event,
        TaskEvent::Requeued {
            runner: None,
            executor: Executor::Claude
        }
    ));

    state.apply_event(
        &id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    assert_eq!(status(&state, &id), TaskStatus::Running);
    let executions = &state.tasks[&id].task.executions;
    assert_eq!(executions.len(), 2);
    assert_eq!(executions[1].attempt, 2);
}

#[test]
fn retry_refuses_an_executor_the_runner_does_not_have() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let id = state
        .create_task(spec(Executor::Claude, Some("a")))
        .unwrap()
        .0
        .id;
    // Failed, not RunnerLost: without a policy a failure does not auto-reassign,
    // so the task is still here for this manual retry to be refused.
    state.apply_event(
        &id,
        TaskEvent::Failed {
            error: "boom".into(),
        },
    );

    let into = RetryInto {
        runner: None,
        executor: Some(Executor::Codex),
    };
    assert!(matches!(
        state.retry(&id, into),
        Err(CmdError::Conflict(msg)) if msg == "runner a does not have codex"
    ));
    assert_eq!(
        status(&state, &id),
        TaskStatus::Failed,
        "a refused retry leaves the task where it was"
    );
}

#[test]
fn retry_refuses_a_task_still_under_review() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let id = create(&mut state, Executor::Claude).id;
    state.apply_event(
        &id,
        TaskEvent::Completed {
            result: TaskResult {
                branch: format!("lgtm/{id}"),
                diff: "diff".into(),
                changed_files: vec!["a.rs".into()],
                validation: Vec::new(),
                plan: None,
                review: None,
                policy: None,
                cost_usd: 0.0,
            },
        },
    );
    assert!(matches!(
        state.retry(&id, same_place()),
        Err(CmdError::Conflict(_))
    ));
}

/// The `Requeued` events on a task, in order.
fn requeues(state: &State, id: &str) -> Vec<TaskEvent> {
    state.tasks[id]
        .events
        .iter()
        .map(|stored| stored.event.clone())
        .filter(|event| matches!(event, TaskEvent::Requeued { .. }))
        .collect()
}

#[test]
fn a_lost_task_is_reassigned_to_the_other_runner() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let _b = connect(&mut state, "b", 1, 2);
    let id = create(&mut state, Executor::Claude).id;
    let first = state.tasks[&id].task.runner.clone().unwrap();

    let changed = state.apply_event(&id, TaskEvent::RunnerLost);
    assert!(changed.contains(&id));
    assert_eq!(
        status(&state, &id),
        TaskStatus::Queued,
        "no policy at all still gets one reassign"
    );
    let second = state.tasks[&id].task.runner.clone().unwrap();
    assert_ne!(first, second, "moved off the runner that lost it");
    assert_eq!(requeues(&state, &id).len(), 1);
}

/// A completed run's result declaring `reassign` tries.
fn result_with_reassign(reassign: u32) -> TaskResult {
    TaskResult {
        policy: Some(Policy {
            reassign,
            ..Policy::default()
        }),
        ..run_result()
    }
}

#[test]
fn a_failed_task_with_reassign_one_requeues_once_then_stays_failed() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let id = create(&mut state, Executor::Claude).id;
    state.apply_event(
        &id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        &id,
        TaskEvent::Completed {
            result: result_with_reassign(1),
        },
    );

    state.apply_event(
        &id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        &id,
        TaskEvent::Failed {
            error: "boom".into(),
        },
    );
    assert_eq!(status(&state, &id), TaskStatus::Queued, "reassigned once");
    assert_eq!(requeues(&state, &id).len(), 1);

    state.apply_event(
        &id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        &id,
        TaskEvent::Failed {
            error: "boom again".into(),
        },
    );
    assert_eq!(
        status(&state, &id),
        TaskStatus::Failed,
        "reassign budget already spent"
    );
    assert_eq!(requeues(&state, &id).len(), 1);
}

#[test]
fn a_failed_task_without_the_policy_stays_failed() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let id = create(&mut state, Executor::Claude).id;
    state.apply_event(
        &id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        &id,
        TaskEvent::Failed {
            error: "boom".into(),
        },
    );
    assert_eq!(status(&state, &id), TaskStatus::Failed);
    assert!(requeues(&state, &id).is_empty());
}

#[test]
fn a_follow_up_carries_the_memories() {
    let mut state = State::default();
    let mut a = connect(&mut state, "a", 1, 1);
    let memory = state.create_memory(
        Some(spec(Executor::Claude, None).repository),
        "no yarn".into(),
        MemorySource::User,
        None,
        None,
    );
    let id = create(&mut state, Executor::Claude).id;
    let result = TaskResult {
        branch: format!("lgtm/{id}"),
        diff: "diff".into(),
        changed_files: vec!["a.rs".into()],
        validation: Vec::new(),
        plan: None,
        review: None,
        policy: None,
        cost_usd: 0.0,
    };
    state.apply_event(&id, TaskEvent::Completed { result });
    state.message(&id, "keep going".into(), None).unwrap();

    let frames: Vec<OrchestratorMessage> = std::iter::from_fn(|| a.try_recv().ok()).collect();
    assert!(frames.iter().any(|frame| matches!(
        frame,
        OrchestratorMessage::Message { memories, .. } if memories.as_slice() == [memory.clone()]
    )));
}

#[test]
fn a_start_frame_carries_the_skills() {
    let mut state = State::default();
    let mut a = connect(&mut state, "a", 1, 1);
    let repository = spec(Executor::Claude, None).repository;
    let skill = state
        .create_skill(
            Some(repository),
            "---\nname: review\ndescription: Review a PR.\n---\nSteps.".into(),
            Vec::new(),
            None,
            MemorySource::User,
            None,
            None,
        )
        .unwrap();
    create(&mut state, Executor::Claude);

    assert!(matches!(
        a.try_recv().unwrap(),
        OrchestratorMessage::Start { skills, .. } if skills == vec![skill.clone()]
    ));
}

#[test]
fn a_follow_up_carries_the_skills() {
    let mut state = State::default();
    let mut a = connect(&mut state, "a", 1, 1);
    let skill = state
        .create_skill(
            Some(spec(Executor::Claude, None).repository),
            "---\nname: review\ndescription: Review a PR.\n---\nSteps.".into(),
            Vec::new(),
            None,
            MemorySource::User,
            None,
            None,
        )
        .unwrap();
    let id = create(&mut state, Executor::Claude).id;
    let result = TaskResult {
        branch: format!("lgtm/{id}"),
        diff: "diff".into(),
        changed_files: vec!["a.rs".into()],
        validation: Vec::new(),
        plan: None,
        review: None,
        policy: None,
        cost_usd: 0.0,
    };
    state.apply_event(&id, TaskEvent::Completed { result });
    state.message(&id, "keep going".into(), None).unwrap();

    let frames: Vec<OrchestratorMessage> = std::iter::from_fn(|| a.try_recv().ok()).collect();
    assert!(frames.iter().any(|frame| matches!(
        frame,
        OrchestratorMessage::Message { skills, .. } if skills.as_slice() == [skill.clone()]
    )));
}

#[test]
fn a_proposed_skill_is_not_handed_out_until_approved() {
    let mut state = State::default();
    let repository = spec(Executor::Claude, None).repository;
    let skill = state
        .create_skill(
            Some(repository.clone()),
            "---\nname: review\ndescription: Review a PR.\n---\nSteps.".into(),
            Vec::new(),
            None,
            MemorySource::Agent,
            Some("t1".into()),
            None,
        )
        .unwrap();
    assert_eq!(skill.verification, Verification::AgentProposed);
    assert!(state.skills_for(&repository).is_empty());

    let approved = state.approve_skill(&skill.id).unwrap();
    assert_eq!(approved.verification, Verification::UserApproved);
    assert_eq!(state.skills_for(&repository), [approved]);
}

#[test]
fn a_repository_skill_shadows_the_workspace_one_of_the_same_name() {
    let mut state = State::default();
    let repository = spec(Executor::Claude, None).repository;
    state
        .create_skill(
            None,
            "---\nname: review\ndescription: Review any PR.\n---\nSteps.".into(),
            Vec::new(),
            None,
            MemorySource::User,
            None,
            None,
        )
        .unwrap();
    state
        .create_skill(
            Some(repository.clone()),
            "---\nname: review\ndescription: Review this repository's PRs.\n---\nSteps.".into(),
            Vec::new(),
            None,
            MemorySource::User,
            None,
            None,
        )
        .unwrap();

    let here = state.skills_for(&repository);
    assert_eq!(here.len(), 1);
    assert_eq!(here[0].repository.as_deref(), Some(repository.as_str()));

    let elsewhere = state.skills_for("https://example.com/other.git");
    assert_eq!(elsewhere.len(), 1);
    assert_eq!(elsewhere[0].repository, None);
}

#[test]
fn a_skill_that_is_not_a_skill_is_refused() {
    let mut state = State::default();
    assert!(state
        .create_skill(
            None,
            "just text".into(),
            Vec::new(),
            None,
            MemorySource::User,
            None,
            None
        )
        .is_err());
    assert!(state.skills.is_empty());
}

#[test]
fn editing_a_skill_bumps_its_revision() {
    let mut state = State::default();
    let skill = state
        .create_skill(
            None,
            "---\nname: review\ndescription: Review a PR.\n---\nSteps.".into(),
            Vec::new(),
            None,
            MemorySource::User,
            None,
            None,
        )
        .unwrap();

    let edited = state
        .edit_skill(
            &skill.id,
            SkillPatch {
                content: Some(
                    "---\nname: review\ndescription: Review a PR carefully.\n---\nSteps.".into(),
                ),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(edited.revision, 2);
    assert_eq!(edited.description, "Review a PR carefully.");
    assert_eq!(edited.files, skill.files);

    let err = state.edit_skill(
        &skill.id,
        SkillPatch {
            content: Some("---\nname: review\n---\nSteps.".into()),
            ..Default::default()
        },
    );
    assert!(err.is_err());
    assert_eq!(state.skills[&skill.id].revision, 2);
}

#[test]
fn an_import_records_and_keeps_its_origin() {
    let mut state = State::default();
    let skill = state
        .create_skill(
            None,
            "---\nname: review\ndescription: Review a PR.\n---\nSteps.".into(),
            Vec::new(),
            Some("/home/a/.claude/skills/review".into()),
            MemorySource::User,
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        skill.origin.as_deref(),
        Some("/home/a/.claude/skills/review")
    );

    let kept = state
        .edit_skill(
            &skill.id,
            SkillPatch {
                content: Some(
                    "---\nname: review\ndescription: Review a PR carefully.\n---\nSteps.".into(),
                ),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        kept.origin.as_deref(),
        Some("/home/a/.claude/skills/review")
    );

    let replaced = state
        .edit_skill(
            &skill.id,
            SkillPatch {
                content: Some(
                    "---\nname: review\ndescription: Review a PR carefully.\n---\nSteps.".into(),
                ),
                origin: Some("/elsewhere".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(replaced.origin.as_deref(), Some("/elsewhere"));
}

#[test]
fn a_patch_rewrites_one_part_of_the_skill_and_keeps_the_rest() {
    let mut state = State::default();
    let skill = state
        .create_skill(
            Some("https://example.com/repo.git".into()),
            "---\nname: review\nlicense: MIT\ndescription: Old.\n---\n\nSteps.\n".into(),
            Vec::new(),
            None,
            MemorySource::User,
            None,
            None,
        )
        .unwrap();

    let renamed = state
        .edit_skill(
            &skill.id,
            SkillPatch {
                name: Some("review-pr".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(renamed.name, "review-pr");
    assert!(renamed.content.contains("license: MIT"));
    assert!(renamed.content.contains("description: Old."));
    assert!(renamed.content.contains("Steps."));

    let described = state
        .edit_skill(
            &skill.id,
            SkillPatch {
                description: Some("New.".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(described.description, "New.");

    let bodied = state
        .edit_skill(
            &skill.id,
            SkillPatch {
                body: Some("# Steps\n\n1. Read.\n".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
    assert!(bodied.content.ends_with("---\n\n# Steps\n\n1. Read.\n"));
    assert!(bodied.content.contains("name: review-pr"));
    assert!(bodied.content.contains("license: MIT"));
    assert_eq!(bodied.description, "New.");

    let moved = state
        .edit_skill(
            &skill.id,
            SkillPatch {
                repository: Some(None),
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(moved.repository, None);

    let empty = state.edit_skill(&skill.id, SkillPatch::default());
    assert!(empty.is_err());

    let revision = state.skills[&skill.id].revision;
    let bad_name = state.edit_skill(
        &skill.id,
        SkillPatch {
            name: Some("Bad Name".into()),
            ..Default::default()
        },
    );
    assert!(bad_name.is_err());
    assert_eq!(state.skills[&skill.id].revision, revision);
}

#[test]
fn a_proposed_memory_is_not_told_until_approved() {
    let mut state = State::default();
    let repository = spec(Executor::Claude, None).repository;
    let memory = state.create_memory(
        Some(repository.clone()),
        "no yarn".into(),
        MemorySource::Agent,
        Some("t1".into()),
        None,
    );
    assert_eq!(memory.verification, Verification::AgentProposed);
    assert!(state.memories_for(&repository).is_empty());

    let approved = state.approve_memory(&memory.id).unwrap();
    assert_eq!(approved.verification, Verification::UserApproved);
    assert_eq!(state.memories_for(&repository), [approved]);
}

/// A bare task in `status`, from a Linear issue or not, with no runner or
/// events behind it: `linear_sync_plan` reads nothing else.
fn linear_task(status: TaskStatus, from_linear: bool) -> Task {
    let mut spec = spec(Executor::Claude, None);
    spec.linear = from_linear.then(|| LinearRef {
        id: "uuid".into(),
        identifier: "ENG-1".into(),
        url: "https://linear.app/w/issue/ENG-1".into(),
    });
    Task {
        id: "0123abcd".into(),
        title: None,
        spec,
        status,
        runner: None,
        created_at: 1,
        result: None,
        error: None,
        pull_request: None,
        ci: None,
        pr_review: None,
        executions: Vec::new(),
        scratchpad: String::new(),
        files: Vec::new(),
        workspace: None,
        created_by: None,
        archived: false,
    }
}

#[test]
fn linear_sync_plan_follows_status() {
    use lgtm_linear::Target;

    assert!(
        linear_sync_plan(
            &linear_task(TaskStatus::Running, false),
            TaskStatus::Queued,
            false
        )
        .is_empty(),
        "a task that did not come from Linear is never synced"
    );

    let running = linear_task(TaskStatus::Running, true);
    assert_eq!(
        linear_sync_plan(&running, TaskStatus::Queued, false),
        vec![LinearSync::Move(Target::Started)]
    );
    assert_eq!(
        linear_sync_plan(&running, TaskStatus::AwaitingReview, false),
        vec![LinearSync::Move(Target::Started)],
        "a follow-up run pulls the issue back out of review"
    );
    assert_eq!(
        linear_sync_plan(
            &linear_task(TaskStatus::AwaitingReview, true),
            TaskStatus::Running,
            false
        ),
        vec![LinearSync::Move(Target::InReview)]
    );
    assert_eq!(
        linear_sync_plan(
            &linear_task(TaskStatus::Merged, true),
            TaskStatus::Approved,
            false
        ),
        vec![LinearSync::Move(Target::Completed)]
    );
    assert!(
        linear_sync_plan(
            &linear_task(TaskStatus::Failed, true),
            TaskStatus::Running,
            false
        )
        .is_empty(),
        "a failure is the developer's business, not the issue's"
    );

    let mut approved = linear_task(TaskStatus::Approved, true);
    approved.pull_request = Some(PullRequest {
        number: 12,
        url: "https://github.com/arsenstorm/lgtm/pull/12".into(),
    });
    assert_eq!(
        linear_sync_plan(&approved, TaskStatus::Approved, true),
        vec![LinearSync::Comment(
            "Pull request: https://github.com/arsenstorm/lgtm/pull/12".into()
        )]
    );
    assert!(
        linear_sync_plan(&approved, TaskStatus::Approved, false).is_empty(),
        "the comment is only for the transition that recorded the pull request"
    );
}

pub(crate) fn step(key: &str, depends_on: &[&str]) -> PlanStep {
    PlanStep {
        key: key.into(),
        title: format!("Step {key}"),
        prompt: format!("do {key}"),
        depends_on: depends_on.iter().map(|dep| (*dep).to_string()).collect(),
    }
}

/// A plan task that ran and is awaiting review with `steps` to approve.
pub(crate) fn planned(state: &mut State, steps: Vec<PlanStep>) -> TaskId {
    let mut spec = spec(Executor::Claude, None);
    spec.kind = TaskKind::Plan;
    let id = state.create_task(spec).unwrap().0.id;
    state.apply_event(
        &id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        &id,
        TaskEvent::Completed {
            result: TaskResult {
                branch: format!("lgtm/{id}"),
                diff: String::new(),
                changed_files: Vec::new(),
                validation: Vec::new(),
                plan: Some(Plan { steps }),
                review: None,
                policy: None,
                cost_usd: 0.0,
            },
        },
    );
    id
}

/// The tasks a plan created, in step order.
pub(crate) fn children(state: &State, parent: &str) -> Vec<Task> {
    let mut out: Vec<Task> = state
        .tasks
        .values()
        .filter(|rec| rec.task.spec.parent.as_deref() == Some(parent))
        .map(|rec| rec.task.clone())
        .collect();
    out.sort_by_key(|task| task.created_at);
    out
}

/// Drives a child task to `Approved` the way a runner and a reviewer would.
fn approve(state: &mut State, id: &str) {
    state.apply_event(
        id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        id,
        TaskEvent::Completed {
            result: TaskResult {
                branch: format!("lgtm/{id}"),
                diff: "diff".into(),
                changed_files: vec!["a.rs".into()],
                validation: Vec::new(),
                plan: None,
                review: None,
                policy: None,
                cost_usd: 0.0,
            },
        },
    );
    state.apply_event(
        id,
        TaskEvent::Pushed {
            branch: format!("lgtm/{id}"),
            sha: "abc".into(),
        },
    );
}

#[test]
fn approve_plan_creates_children() {
    let mut state = State::default();
    let _w = connect(&mut state, "w", 1, 1);
    let plan = planned(
        &mut state,
        vec![step("a", &[]), step("b", &["a"]), step("c", &["a", "b"])],
    );

    let (task, changed) = state.approve_plan(&plan).unwrap();
    assert_eq!(task.status, TaskStatus::Approved);
    let kids = children(&state, &plan);
    assert_eq!(kids.len(), 3);
    for kid in &kids {
        assert!(changed.contains(&kid.id));
        assert_eq!(kid.spec.kind, TaskKind::Run);
    }

    assert!(kids[0].spec.depends_on.is_empty());
    assert_eq!(kids[0].spec.base_branch, "main");
    assert_eq!(kids[0].spec.prompt, "Step a\n\ndo a");
    assert_eq!(kids[1].spec.depends_on, vec![kids[0].id.clone()]);
    assert_eq!(
        kids[1].spec.base_branch,
        format!("lgtm/{}", kids[0].id),
        "a single dependency's branch is the base"
    );
    assert_eq!(
        kids[2].spec.depends_on,
        vec![kids[0].id.clone(), kids[1].id.clone()]
    );
    assert_eq!(
        kids[2].spec.base_branch, "main",
        "two dependencies have no shared branch"
    );

    assert_eq!(kids[0].runner.as_deref(), Some("w"));
    for kid in &kids[1..] {
        assert_eq!(status(&state, &kid.id), TaskStatus::Queued);
        assert_eq!(kid.runner, None, "blocked by its dependencies");
    }

    // Cancelling a blocked task takes its own dependents with it.
    state.cancel(&kids[1].id).unwrap();
    assert_eq!(status(&state, &kids[2].id), TaskStatus::Failed);
    assert_eq!(
        state.tasks[&kids[2].id].task.error.as_deref(),
        Some(format!("dependency {} cancelled", kids[1].id).as_str())
    );
}

#[test]
fn approve_plan_rejects_unknown_key() {
    let mut state = State::default();
    let _w = connect(&mut state, "w", 1, 1);
    let plan = planned(&mut state, vec![step("a", &["zzz"])]);

    assert!(matches!(
        state.approve_plan(&plan),
        Err(CmdError::Conflict(msg)) if msg == "unknown step key zzz"
    ));
    assert!(children(&state, &plan).is_empty());
    assert_eq!(status(&state, &plan), TaskStatus::AwaitingReview);
}

#[test]
fn approve_plan_rejects_forward_reference() {
    let mut state = State::default();
    let _w = connect(&mut state, "w", 1, 1);
    let plan = planned(&mut state, vec![step("a", &["b"]), step("b", &[])]);

    assert!(matches!(
        state.approve_plan(&plan),
        Err(CmdError::Conflict(msg)) if msg == "unknown step key b"
    ));
    assert!(children(&state, &plan).is_empty());
}

#[test]
fn blocked_task_runs_after_dependency_approved() {
    let mut state = State::default();
    let _w = connect(&mut state, "w", 1, 1);
    let plan = planned(&mut state, vec![step("a", &[]), step("b", &["a"])]);
    state.approve_plan(&plan).unwrap();
    let kids = children(&state, &plan);
    let (a, b) = (kids[0].id.clone(), kids[1].id.clone());
    assert_eq!(state.tasks[&b].task.runner, None);

    approve(&mut state, &a);
    assert_eq!(status(&state, &a), TaskStatus::Approved);
    assert_eq!(
        state.tasks[&b].task.runner.as_deref(),
        Some("w"),
        "approval released the dependent task"
    );
}

/// A minimal successful run's result, for tests that only need `a` to reach
/// `AwaitingReview`.
pub(crate) fn run_result() -> TaskResult {
    TaskResult {
        branch: "lgtm/a".into(),
        diff: "diff".into(),
        changed_files: vec!["a.rs".into()],
        validation: Vec::new(),
        plan: None,
        review: None,
        policy: None,
        cost_usd: 0.0,
    }
}

/// A task depending on `a` with `condition`, scheduled behind it on a
/// single-slot runner so its release is visible.
fn waiting_on(state: &mut State, a: &str, condition: DependsOn) -> TaskId {
    let mut waiting = spec(Executor::Claude, None);
    waiting.depends_on = vec![a.to_string()];
    waiting.depends_on_condition = condition;
    state.create_task(waiting).unwrap().0.id
}

#[test]
fn completed_condition_starts_once_the_dependency_finishes_a_run() {
    let mut state = State::default();
    let _w = connect(&mut state, "w", 1, 1);
    let a = create(&mut state, Executor::Claude).id;
    let b = waiting_on(&mut state, &a, DependsOn::Completed);

    state.apply_event(
        &a,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        &a,
        TaskEvent::Completed {
            result: run_result(),
        },
    );

    assert_eq!(status(&state, &a), TaskStatus::AwaitingReview);
    assert_eq!(
        state.tasks[&b].task.runner.as_deref(),
        Some("w"),
        "Completed only needs the dependency to finish a run"
    );
}

#[test]
fn approved_condition_still_waits_once_the_dependency_finishes_a_run() {
    let mut state = State::default();
    let _w = connect(&mut state, "w", 1, 1);
    let a = create(&mut state, Executor::Claude).id;
    let b = waiting_on(&mut state, &a, DependsOn::Approved);

    state.apply_event(
        &a,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        &a,
        TaskEvent::Completed {
            result: run_result(),
        },
    );
    assert_eq!(status(&state, &a), TaskStatus::AwaitingReview);
    assert_eq!(
        state.tasks[&b].task.runner, None,
        "Approved needs more than a finished run"
    );

    state.apply_event(
        &a,
        TaskEvent::Pushed {
            branch: format!("lgtm/{a}"),
            sha: "abc".into(),
        },
    );
    assert_eq!(status(&state, &a), TaskStatus::Approved);
    assert_eq!(state.tasks[&b].task.runner.as_deref(), Some("w"));
}

#[test]
fn merged_condition_waits_past_approval() {
    let mut state = State::default();
    let _w = connect(&mut state, "w", 1, 1);
    let a = create(&mut state, Executor::Claude).id;
    let b = waiting_on(&mut state, &a, DependsOn::Merged);

    state.apply_event(
        &a,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        &a,
        TaskEvent::Completed {
            result: run_result(),
        },
    );
    state.apply_event(
        &a,
        TaskEvent::Pushed {
            branch: format!("lgtm/{a}"),
            sha: "abc".into(),
        },
    );
    assert_eq!(status(&state, &a), TaskStatus::Approved);
    assert_eq!(
        state.tasks[&b].task.runner, None,
        "Merged needs the pull request merged, not just approved"
    );

    state.mark_merged(&a).unwrap();
    assert_eq!(state.tasks[&b].task.runner.as_deref(), Some("w"));
}

#[test]
fn completed_condition_child_bases_on_the_plan_branch_not_the_dependency() {
    let mut state = State::default();
    let _w = connect(&mut state, "w", 1, 1);
    let mut plan_spec = spec(Executor::Claude, None);
    plan_spec.kind = TaskKind::Plan;
    plan_spec.depends_on_condition = DependsOn::Completed;
    let plan = state.create_task(plan_spec).unwrap().0.id;
    state.apply_event(
        &plan,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        &plan,
        TaskEvent::Completed {
            result: TaskResult {
                branch: format!("lgtm/{plan}"),
                diff: String::new(),
                changed_files: Vec::new(),
                validation: Vec::new(),
                plan: Some(Plan {
                    steps: vec![step("a", &[]), step("b", &["a"])],
                }),
                review: None,
                policy: None,
                cost_usd: 0.0,
            },
        },
    );

    state.approve_plan(&plan).unwrap();
    let kids = children(&state, &plan);
    assert_eq!(kids[1].spec.depends_on_condition, DependsOn::Completed);
    assert_eq!(
        kids[1].spec.base_branch, "main",
        "a Completed dependency's branch is not pushed yet, so the child bases on the plan's own branch"
    );
}

#[test]
fn dependency_failure_cascades() {
    let mut state = State::default();
    let _w = connect(&mut state, "w", 1, 1);
    let plan = planned(
        &mut state,
        vec![step("a", &[]), step("b", &["a"]), step("c", &["b"])],
    );
    state.approve_plan(&plan).unwrap();
    let kids = children(&state, &plan);
    let (a, b, c) = (kids[0].id.clone(), kids[1].id.clone(), kids[2].id.clone());

    let changed = state.apply_event(&a, TaskEvent::Discarded);
    assert_eq!(status(&state, &a), TaskStatus::Rejected);
    assert!(changed.contains(&b) && changed.contains(&c));
    assert_eq!(status(&state, &b), TaskStatus::Failed);
    assert_eq!(
        state.tasks[&b].task.error.as_deref(),
        Some(format!("dependency {a} rejected").as_str())
    );
    assert_eq!(status(&state, &c), TaskStatus::Failed);
    assert_eq!(
        state.tasks[&c].task.error.as_deref(),
        Some(format!("dependency {b} failed").as_str())
    );
}

#[test]
fn unknown_dependency_is_refused() {
    let mut state = State::default();
    let _w = connect(&mut state, "w", 1, 1);
    let mut spec = spec(Executor::Claude, None);
    spec.depends_on = vec!["deadbeef".into()];
    assert_eq!(
        state.check_eligible(&spec).unwrap_err(),
        "unknown dependency deadbeef"
    );
}

#[test]
fn scratchpad_is_kept_even_once_the_task_ended() {
    let mut state = State::default();
    let _w = connect(&mut state, "w", 1, 1);
    let id = create(&mut state, Executor::Claude).id;

    state.apply_event(
        &id,
        TaskEvent::Scratchpad {
            content: "the parser is in src/parse.rs".into(),
        },
    );
    assert_eq!(
        state.tasks[&id].task.scratchpad,
        "the parser is in src/parse.rs"
    );
    assert_eq!(status(&state, &id), TaskStatus::Queued);

    state.apply_event(&id, TaskEvent::Cancelled);
    state.apply_event(
        &id,
        TaskEvent::Scratchpad {
            content: "and the cancel came mid-run".into(),
        },
    );
    assert_eq!(
        state.tasks[&id].task.scratchpad,
        "and the cancel came mid-run"
    );
    assert_eq!(status(&state, &id), TaskStatus::Cancelled);
}

#[test]
fn terminal_status_survives_late_events() {
    let mut state = State::default();
    let _idle = connect(&mut state, "idle", 1, 1);
    let id = create(&mut state, Executor::Claude).id;

    state.apply_event(&id, TaskEvent::Cancelled);
    assert_eq!(status(&state, &id), TaskStatus::Cancelled);

    state.apply_event(
        &id,
        TaskEvent::Failed {
            error: "runner disconnected".into(),
        },
    );
    assert_eq!(status(&state, &id), TaskStatus::Cancelled);
    assert!(state.tasks[&id].task.error.is_none());
    assert_eq!(state.tasks[&id].events.len(), 2);
}

#[test]
fn timeout_ends_the_task_and_frees_the_slot() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let task = create(&mut state, Executor::Claude);
    state.apply_event(
        &task.id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    let queued = create(&mut state, Executor::Claude);
    assert_eq!(queued.runner, None);

    let changed = state.apply_event(&task.id, TaskEvent::TimedOut { secs: 60 });
    assert_eq!(status(&state, &task.id), TaskStatus::TimedOut);
    assert!(changed.contains(&queued.id));
    assert_eq!(
        state.tasks[&queued.id].task.runner.as_deref(),
        Some("a"),
        "the freed slot took the backlog"
    );
}

#[test]
fn requirement_restricts_scheduling_to_capable_runners() {
    let mut state = State::default();
    let _plain = connect(&mut state, "plain", 1, 1);

    let mut needs_docker = spec(Executor::Claude, None);
    needs_docker.requirements = vec!["docker".into()];
    assert_eq!(
        state.check_eligible(&needs_docker).unwrap_err(),
        "no eligible runner"
    );

    let (tx, _rx) = mpsc::unbounded_channel();
    let mut docker_runner = info("docker", 1, vec![Executor::Claude]);
    docker_runner.capabilities = vec!["docker".into()];
    state.runner_hello(docker_runner, Vec::new(), Conn { tx, conn_id: 1 });

    assert!(state.check_eligible(&needs_docker).is_ok());
    let (task, _) = state.create_task(needs_docker).unwrap();
    assert_eq!(task.runner.as_deref(), Some("docker"));
}

#[test]
fn pinned_runner_lacking_a_requirement_is_refused() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);

    let mut needs_docker = spec(Executor::Claude, Some("a"));
    needs_docker.requirements = vec!["docker".into()];
    assert_eq!(
        state.check_eligible(&needs_docker).unwrap_err(),
        "runner a lacks docker"
    );
}

/// A goal, and a spec for a task under it.
pub(crate) fn goal_spec(state: &mut State) -> (String, TaskSpec) {
    let goal = state.create_goal(
        "ship it".into(),
        "https://example.com/repo.git".into(),
        None,
    );
    let mut spec = spec(Executor::Claude, None);
    spec.goal = Some(goal.id.clone());
    (goal.id, spec)
}

#[test]
fn attention_blocks_a_goal_until_new_work_arrives() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 2, 1);
    let (goal, spec) = goal_spec(&mut state);
    let none = HashSet::new();
    state.create_task(spec.clone()).unwrap();
    assert_eq!(
        state.goal_summary(&goal, &none).unwrap().status,
        lgtm_protocol::GoalStatus::Running
    );

    assert!(state.set_attention(&goal, Some("which API?".into())));
    assert_eq!(
        state.goal_summary(&goal, &none).unwrap().status,
        lgtm_protocol::GoalStatus::Blocked
    );
    assert_eq!(state.dirty_goals, vec![goal.clone()]);

    state.create_task(spec).unwrap();
    assert!(state.goals[&goal].attention.is_none());
    assert_eq!(
        state.goal_summary(&goal, &none).unwrap().status,
        lgtm_protocol::GoalStatus::Running
    );
    assert!(!state.set_attention("nothing", None));
}

#[test]
fn a_goal_whose_loop_is_running_reads_as_planning() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 2, 1);
    let (goal, spec) = goal_spec(&mut state);
    state.create_task(spec).unwrap();
    let running = HashSet::from([goal.clone()]);
    assert_eq!(
        state.goal_summary(&goal, &running).unwrap().status,
        lgtm_protocol::GoalStatus::Planning
    );

    // Attention outranks it: the loop stopped, whatever is still finishing.
    state.set_attention(&goal, Some("which API?".into()));
    assert_eq!(
        state.goal_summary(&goal, &running).unwrap().status,
        lgtm_protocol::GoalStatus::Blocked
    );
}

#[test]
fn the_model_table_fills_only_a_spec_that_named_none() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 3, 1);
    state.models = HashMap::from([
        ("plan".to_string(), "opus".to_string()),
        ("run".to_string(), "sonnet".to_string()),
    ]);

    let (task, _) = state.create_task(spec(Executor::Claude, None)).unwrap();
    assert_eq!(task.spec.model.as_deref(), Some("sonnet"));

    let mut plan = spec(Executor::Claude, None);
    plan.kind = TaskKind::Plan;
    assert_eq!(
        state.create_task(plan).unwrap().0.spec.model.as_deref(),
        Some("opus")
    );

    let mut asked = spec(Executor::Claude, None);
    asked.model = Some("haiku".into());
    assert_eq!(
        state.create_task(asked).unwrap().0.spec.model.as_deref(),
        Some("haiku")
    );
}

#[test]
fn scrollback_caps_and_keeps_the_newest_output() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let (task, _) = state.create_task(spec(Executor::Claude, None)).unwrap();
    let rec = state.tasks.get_mut(&task.id).unwrap();
    let chunk = 1024;
    for i in 0..100 {
        rec.push_terminal(format!("[{i}]{}", "x".repeat(chunk)));
    }

    let seen = rec.scrollback();
    assert!(seen.len() <= SCROLLBACK_MAX, "{} bytes kept", seen.len());
    assert!(seen.contains("[99]"), "the newest output is gone");
    assert!(!seen.contains("[0]"), "the oldest output was not dropped");
}

/// A completed run's result reporting `cost_usd` and declaring
/// `budget_daily_usd`.
fn result_with_budget(cost_usd: f64, budget_daily_usd: Option<f64>) -> TaskResult {
    TaskResult {
        cost_usd,
        policy: Some(Policy {
            budget_daily_usd,
            ..Policy::default()
        }),
        ..run_result()
    }
}

#[test]
fn schedule_skips_a_task_over_its_repositorys_daily_budget() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let done = create(&mut state, Executor::Claude).id;
    state.apply_event(
        &done,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        &done,
        TaskEvent::Completed {
            result: result_with_budget(60.0, Some(50.0)),
        },
    );
    assert_eq!(
        state.spent_last_day(&spec(Executor::Claude, None).repository),
        60.0
    );

    let queued = create(&mut state, Executor::Claude).id;
    assert_eq!(
        state.tasks[&queued].task.runner, None,
        "over budget, left in the queue"
    );
    assert_eq!(status(&state, &queued), TaskStatus::Queued);

    let decisions: Vec<TaskEvent> = state.tasks[&queued]
        .events
        .iter()
        .map(|stored| stored.event.clone())
        .filter(|event| matches!(event, TaskEvent::PolicyDecision { .. }))
        .collect();
    assert_eq!(
        decisions.len(),
        1,
        "recorded once, not once per schedule() call"
    );
    match &decisions[0] {
        TaskEvent::PolicyDecision {
            action,
            allowed,
            reasons,
        } => {
            assert_eq!(action.as_str(), "schedule");
            assert!(!*allowed);
            assert!(reasons[0].contains("daily budget"));
        }
        other => panic!("expected a PolicyDecision, got {other:?}"),
    }

    // Nothing frees a slot, so scheduling again must not repeat the decision.
    state.schedule();
    let count = state.tasks[&queued]
        .events
        .iter()
        .filter(|stored| matches!(stored.event, TaskEvent::PolicyDecision { .. }))
        .count();
    assert_eq!(count, 1);
}

#[test]
fn schedule_ignores_the_budget_once_spend_is_back_under_it() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let done = create(&mut state, Executor::Claude).id;
    state.apply_event(
        &done,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        &done,
        TaskEvent::Completed {
            result: result_with_budget(10.0, Some(50.0)),
        },
    );

    let queued = create(&mut state, Executor::Claude).id;
    assert_eq!(
        state.tasks[&queued].task.runner.as_deref(),
        Some("a"),
        "under budget, scheduled as usual"
    );
}

#[test]
fn median_for_is_none_without_execution_history() {
    let state = State::default();
    assert_eq!(state.median_for("a", "https://example.com/repo.git"), None);
}

#[test]
fn median_for_medians_a_runners_finished_durations_in_the_repository() {
    let mut state = State::default();
    add_history(
        &mut state,
        vec![
            finished_execution("a", 0, 100),
            finished_execution("a", 0, 300),
            finished_execution("b", 0, 999),
        ],
    );
    assert_eq!(
        state.median_for("a", "https://example.com/repo.git"),
        Some(200)
    );
}

#[test]
fn candidate_breaks_a_free_slot_tie_by_median_duration_under_fastest() {
    let mut state = State {
        prefer: crate::Prefer::Fastest,
        ..Default::default()
    };
    let _a = connect(&mut state, "a", 1, 1);
    let _b = connect(&mut state, "b", 1, 2);
    add_history(
        &mut state,
        vec![
            finished_execution("a", 0, 200),
            finished_execution("b", 0, 100),
        ],
    );
    assert_eq!(state.candidate(&spec(Executor::Claude, None)).unwrap(), "b");
}

#[test]
fn candidate_prefers_a_runner_with_history_over_one_with_none_under_fastest() {
    let mut state = State {
        prefer: crate::Prefer::Fastest,
        ..Default::default()
    };
    let _a = connect(&mut state, "a", 1, 1);
    let _b = connect(&mut state, "b", 1, 2);
    add_history(&mut state, vec![finished_execution("a", 0, 500)]);
    assert_eq!(state.candidate(&spec(Executor::Claude, None)).unwrap(), "a");
}

#[test]
fn candidate_ignores_median_duration_under_the_default_prefer_slots() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let _b = connect(&mut state, "b", 1, 2);
    // "a" is the slower runner, but Prefer::Slots never looks: lowest name wins.
    add_history(
        &mut state,
        vec![
            finished_execution("a", 0, 999),
            finished_execution("b", 0, 100),
        ],
    );
    assert_eq!(state.candidate(&spec(Executor::Claude, None)).unwrap(), "a");
}

#[test]
fn a_foreign_workspace_memory_is_not_told_to_runs() {
    let mut state = State {
        workspace: Some("other".into()),
        ..State::default()
    };
    state.create_memory(None, "no yarn".into(), MemorySource::User, None, None);
    state.workspace = Some("acme".into());
    assert!(state
        .memories_for("https://example.com/repo.git")
        .is_empty());
    state.workspace = None;
    assert_eq!(state.memories_for("https://example.com/repo.git").len(), 1);
}

#[test]
fn editing_an_agent_proposal_approves_it() {
    let mut state = State::default();
    let proposed = state.create_memory(
        None,
        "the build needs bun".into(),
        MemorySource::Agent,
        None,
        None,
    );
    let approved = state.create_memory(None, "no yarn".into(), MemorySource::User, None, None);

    let edited = state
        .edit_memory(&proposed.id, "the build needs bun 1.2".into())
        .unwrap();
    assert_eq!(edited.content, "the build needs bun 1.2");
    assert_eq!(edited.verification, Verification::UserApproved);
    // An approved memory is left approved, not re-verified into anything else.
    let edited = state.edit_memory(&approved.id, "no npm".into()).unwrap();
    assert_eq!(edited.verification, Verification::UserApproved);
    assert!(state.edit_memory("deadbeef", "x".into()).is_none());
}

#[test]
fn a_todos_comments_read_oldest_first() {
    let mut state = State::default();
    let todo = state.create_todo(None, "t".into(), String::new(), None);
    let first = state
        .create_todo_comment(&todo.id, "first".into(), Some("arsen".into()))
        .unwrap();
    let second = state
        .create_todo_comment(&todo.id, "second".into(), None)
        .unwrap();
    // Two comments in the same millisecond must still read in the order they
    // were written.
    state.todo_comments.get_mut(&first.id).unwrap().created_at = 1;
    state.todo_comments.get_mut(&second.id).unwrap().created_at = 2;

    let thread = state.todo_comments(&todo.id);
    assert_eq!(
        thread.iter().map(|c| c.body.as_str()).collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert_eq!(thread[0].author.as_deref(), Some("arsen"));
    assert!(state
        .create_todo_comment("deadbeef", "orphan".into(), None)
        .is_none());
}

#[test]
fn deleting_a_todo_deletes_its_comments() {
    let mut state = State::default();
    let todo = state.create_todo(None, "t".into(), String::new(), None);
    let other = state.create_todo(None, "other".into(), String::new(), None);
    let comment = state
        .create_todo_comment(&todo.id, "mine".into(), None)
        .unwrap();
    let kept = state
        .create_todo_comment(&other.id, "theirs".into(), None)
        .unwrap();

    let removed = state.remove_todo(&todo.id).unwrap();

    assert_eq!(removed, [comment.id]);
    assert!(state.todo_comments(&todo.id).is_empty());
    assert!(state.todo_comments.contains_key(&kept.id));
    assert!(state.remove_todo(&todo.id).is_none());
}

#[test]
fn a_scratchpad_bumps_updated_at_only_when_the_content_changes() {
    let mut state = State::default();
    let pad = state.create_scratchpad(
        "Notes".into(),
        None,
        "notes".into(),
        Vec::new(),
        Some("arsen".into()),
    );
    assert_eq!(pad.updated_at, pad.created_at);
    state.scratchpads.get_mut(&pad.id).unwrap().updated_at = 1;

    let same = state
        .update_scratchpad(&pad.id, None, None, Some("notes".into()), None, None)
        .unwrap();
    assert_eq!(same.updated_at, 1);
    let archived = state
        .update_scratchpad(&pad.id, None, None, None, Some(true), None)
        .unwrap();
    assert!(archived.archived);
    assert_eq!(archived.updated_at, 1);

    let written = state
        .update_scratchpad(&pad.id, None, None, Some("more notes".into()), None, None)
        .unwrap();
    assert_eq!(written.content, "more notes");
    assert!(written.updated_at > 1);
}

#[test]
fn a_scratchpad_is_renamed_without_counting_as_an_edit() {
    let mut state = State::default();
    let pad = state.create_scratchpad(
        "09-05-09-35 Scratchpad".into(),
        None,
        "# Plan".into(),
        Vec::new(),
        None,
    );
    assert_eq!(pad.title, "09-05-09-35 Scratchpad");
    state.scratchpads.get_mut(&pad.id).unwrap().updated_at = 1;

    let renamed = state
        .update_scratchpad(&pad.id, Some("Plan".into()), None, None, None, None)
        .unwrap();
    assert_eq!(renamed.title, "Plan");
    assert_eq!(renamed.content, "# Plan");
    assert_eq!(renamed.updated_at, 1);
}

#[test]
fn a_document_saved_without_a_title_is_named_by_its_old_heading_rule() {
    assert_eq!(legacy_title("# Runner Notes\n\nthings"), "Runner Notes");
    assert_eq!(legacy_title("\nfirst line\n# later"), "later");
    assert_eq!(legacy_title("\nfirst line\nmore"), "first line");
    assert_eq!(legacy_title("  \n"), "Untitled");
}

#[test]
fn a_scratchpad_moves_between_repositories_without_counting_as_an_edit() {
    let mut state = State::default();
    let pad = state.create_scratchpad("Notes".into(), None, "prd".into(), Vec::new(), None);
    state.scratchpads.get_mut(&pad.id).unwrap().updated_at = 1;

    let moved = state
        .update_scratchpad(
            &pad.id,
            None,
            Some(Some("git@x/y.git".into())),
            None,
            None,
            None,
        )
        .unwrap();
    assert_eq!(moved.repository.as_deref(), Some("git@x/y.git"));
    assert_eq!(moved.updated_at, 1);

    let left = state
        .update_scratchpad(&pad.id, None, None, None, None, None)
        .unwrap();
    assert_eq!(left.repository.as_deref(), Some("git@x/y.git"));

    let general = state
        .update_scratchpad(&pad.id, None, Some(None), None, None, None)
        .unwrap();
    assert_eq!(general.repository, None);
}

#[test]
fn a_scratchpad_is_gone_once_deleted() {
    let mut state = State::default();
    let pad = state.create_scratchpad("Notes".into(), None, String::new(), Vec::new(), None);
    assert!(state.remove_scratchpad(&pad.id));
    assert!(!state.remove_scratchpad(&pad.id));
    assert!(state
        .update_scratchpad(&pad.id, None, None, Some("x".into()), None, None)
        .is_none());
}
