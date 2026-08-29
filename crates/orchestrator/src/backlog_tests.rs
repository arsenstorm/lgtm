//! Unit tests for `backlog.rs`: pure candidate mapping, plus the batch's
//! hands-off plan approval, which is state and no I/O.

use super::*;
use lgtm_protocol::{
    Batch, BatchSource, OrchestratorMessage, Plan, PlanStep, TaskEvent, TaskId, TaskResult,
    WorkerInfo,
};
use tokio::sync::mpsc;

use crate::state::{Conn, TaskRecord};

fn task(id: &str, status: TaskStatus, spec: TaskSpec) -> Task {
    Task {
        id: id.into(),
        spec,
        status,
        worker: None,
        created_at: 1,
        result: None,
        error: None,
        pull_request: None,
        ci: None,
    }
}

fn repo() -> lgtm_github::Repo {
    lgtm_github::Repo {
        owner: "arsenstorm".into(),
        repo: "lgtm".into(),
    }
}

fn issue(number: u64) -> lgtm_github::Issue {
    lgtm_github::Issue {
        number,
        title: format!("Issue {number}"),
        body: "please fix".into(),
        html_url: format!("https://github.com/arsenstorm/lgtm/issues/{number}"),
    }
}

fn candidate(number: u64) -> Candidate {
    github_candidate(
        &issue(number),
        &repo(),
        "main",
        Executor::Claude,
        None,
        false,
        "b0000001",
    )
}

#[test]
fn github_candidate_builds_the_from_issue_shape() {
    let candidate = github_candidate(
        &issue(12),
        &repo(),
        "trunk",
        Executor::Codex,
        Some("w".into()),
        true,
        "b0000001",
    );
    assert_eq!(candidate.key, "#12");
    assert_eq!(candidate.title, "Issue 12");
    assert_eq!(
        candidate.url,
        "https://github.com/arsenstorm/lgtm/issues/12"
    );
    let spec = candidate.spec;
    assert_eq!(spec.repository, "https://github.com/arsenstorm/lgtm.git");
    assert_eq!(spec.base_branch, "trunk");
    assert_eq!(
        spec.prompt,
        "Resolve GitHub issue #12: Issue 12\n\nplease fix"
    );
    assert_eq!(spec.executor, Executor::Codex);
    assert_eq!(spec.worker.as_deref(), Some("w"));
    assert_eq!(spec.kind, TaskKind::Plan);
    assert_eq!(spec.batch.as_deref(), Some("b0000001"));
    assert_eq!(
        spec.issue,
        Some(IssueRef {
            owner: "arsenstorm".into(),
            repo: "lgtm".into(),
            number: 12,
        })
    );
    assert!(spec.linear.is_none() && spec.parent.is_none());
}

#[test]
fn select_skips_issues_already_being_worked_on() {
    let existing = vec![
        // Queued for #1, so #1 is somebody's job already.
        task("00000001", TaskStatus::Queued, candidate(1).spec),
        // #2's last attempt failed, so it is fair game again.
        task("00000002", TaskStatus::Failed, candidate(2).spec),
    ];
    let selected = select(
        &existing,
        vec![candidate(1), candidate(2), candidate(3)],
        10,
    );
    let keys: Vec<&str> = selected.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, vec!["#2", "#3"]);
}

#[test]
fn select_honours_max_after_dropping_duplicates() {
    let existing = vec![task("00000001", TaskStatus::Running, candidate(1).spec)];
    let selected = select(
        &existing,
        vec![candidate(1), candidate(2), candidate(3), candidate(4)],
        2,
    );
    let keys: Vec<&str> = selected.iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, vec!["#2", "#3"]);
    assert!(select(&[], vec![candidate(1)], 0).is_empty());
}

#[test]
fn summary_counts_blocked_apart_from_queued() {
    let mut state = State::default();
    let done = state.new_id();
    let mut runnable = candidate(1).spec;
    runnable.depends_on = vec![done.clone()];
    let tasks = [
        task("00000001", TaskStatus::Queued, candidate(2).spec),
        task("00000002", TaskStatus::Queued, runnable),
        task("00000003", TaskStatus::Running, candidate(3).spec),
        task("00000004", TaskStatus::Merged, candidate(4).spec),
    ];
    // The dependency is unknown, so nothing can start the task waiting on it.
    let refs: Vec<&Task> = tasks.iter().collect();
    let counts = summary(&refs, &state);
    assert_eq!(counts.queued, 1);
    assert_eq!(counts.blocked, 1);
    assert_eq!(counts.running, 1);
    assert_eq!(counts.merged, 1);
    assert_eq!(counts.failed, 0);

    // Once the dependency is approved the same task counts as queued.
    state.tasks.insert(
        done.clone(),
        TaskRecord::new(
            task(&done, TaskStatus::Approved, candidate(9).spec),
            Vec::new(),
        ),
    );
    let counts = summary(&refs, &state);
    assert_eq!(counts.queued, 2);
    assert_eq!(counts.blocked, 0);
}

/// A batch whose plans need no reviewer, and one plan task in it that has run.
fn batch_with_plan(state: &mut State, approve_plans: bool) -> TaskId {
    let batch_id = state.new_batch_id();
    let mut spec = candidate(1).spec;
    spec.kind = TaskKind::Plan;
    spec.batch = Some(batch_id.clone());
    let id = state.create_task(spec).unwrap().0.id;
    state.batches.insert(
        batch_id.clone(),
        Batch {
            id: batch_id,
            created_at: 1,
            source: BatchSource::GithubLabel {
                owner: "arsenstorm".into(),
                repo: "lgtm".into(),
                label: "lgtm".into(),
            },
            repository: "https://github.com/arsenstorm/lgtm.git".into(),
            task_ids: vec![id.clone()],
            approve_plans,
        },
    );
    state.apply_event(&id, TaskEvent::Started);
    state.apply_event(
        &id,
        TaskEvent::Completed {
            result: TaskResult {
                branch: format!("lgtm/{id}"),
                diff: String::new(),
                changed_files: Vec::new(),
                validation: Vec::new(),
                plan: Some(Plan {
                    steps: vec![PlanStep {
                        key: "a".into(),
                        title: "Step a".into(),
                        prompt: "do a".into(),
                        depends_on: Vec::new(),
                    }],
                }),
                review: None,
                policy: None,
                cost_usd: 0.0,
            },
        },
    );
    id
}

fn worker(state: &mut State) -> mpsc::UnboundedReceiver<OrchestratorMessage> {
    let (tx, rx) = mpsc::unbounded_channel();
    state.worker_hello(
        WorkerInfo {
            name: "w".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            executors: vec![Executor::Claude],
            slots: 4,
            ephemeral: false,
        },
        Vec::new(),
        Conn { tx, conn_id: 1 },
    );
    rx
}

#[test]
fn batch_approve_plans_turns_a_plan_into_its_steps() {
    let mut state = State::default();
    let _w = worker(&mut state);
    let plan = batch_with_plan(&mut state, true);
    assert_eq!(state.tasks[&plan].task.status, TaskStatus::AwaitingReview);

    let changed = state.auto_approve_plan(&plan);
    assert!(changed.contains(&plan));
    assert_eq!(state.tasks[&plan].task.status, TaskStatus::Approved);
    assert!(
        state.tasks[&plan]
            .events
            .iter()
            .any(|stored| stored.event == TaskEvent::AutoApproved),
        "the plan records that nobody looked at it",
    );

    let batch = state.tasks[&plan].task.spec.batch.clone();
    let children: Vec<&Task> = state
        .tasks
        .values()
        .map(|rec| &rec.task)
        .filter(|task| task.spec.parent.as_deref() == Some(plan.as_str()))
        .collect();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].spec.batch, batch, "children join the batch");
    assert_eq!(children[0].spec.prompt, "Step a\n\ndo a");
}

#[test]
fn a_plan_outside_an_approving_batch_waits_for_a_person() {
    let mut state = State::default();
    let _w = worker(&mut state);
    let plan = batch_with_plan(&mut state, false);
    assert!(state.auto_approve_plan(&plan).is_empty());
    assert_eq!(state.tasks[&plan].task.status, TaskStatus::AwaitingReview);
}
