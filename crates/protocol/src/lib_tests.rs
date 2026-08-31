use super::*;

fn sample_task() -> Task {
    Task {
        id: "0123abcd".into(),
        spec: TaskSpec {
            repository: "https://github.com/arsenstorm/lgtm.git".into(),
            base_branch: "main".into(),
            prompt: "add a /health endpoint".into(),
            executor: Executor::Claude,
            runner: Some("compute".into()),
            issue: Some(IssueRef {
                owner: "arsenstorm".into(),
                repo: "lgtm".into(),
                number: 7,
            }),
            linear: Some(LinearRef {
                id: "uuid".into(),
                identifier: "ENG-123".into(),
                url: "https://linear.app/w/issue/ENG-123".into(),
            }),
            kind: TaskKind::Plan,
            parent: Some("00000000".into()),
            depends_on: vec!["11111111".into()],
            depends_on_condition: DependsOn::Completed,
            batch: Some("b1".into()),
            sandbox: Some(SandboxProfile::Strict),
            requirements: vec!["docker".into()],
            goal: Some("g1".into()),
            review_executor: None,
            model: Some("opus".into()),
            allowed_hosts: Vec::new(),
            session: Some("s1".into()),
            created_by: None,
        },
        status: TaskStatus::Queued,
        runner: None,
        created_at: 1,
        result: None,
        error: None,
        pull_request: Some(PullRequest {
            number: 12,
            url: "https://github.com/arsenstorm/lgtm/pull/12".into(),
        }),
        ci: Some(CiStatus {
            state: CiState::Pending,
            url: "https://github.com/arsenstorm/lgtm/pull/12/checks".into(),
        }),
        pr_review: None,
        executions: vec![Execution {
            attempt: 1,
            runner: "compute".into(),
            executor: Executor::Claude,
            model: Some("claude-opus-4".into()),
            started_at: 2,
            finished_at: Some(3),
            status: ExecutionStatus::Completed,
            error: None,
            cost_usd: 0.42,
            validation: Vec::new(),
            artefacts: Vec::new(),
        }],
        scratchpad: "## Findings\n- the parser is in src/parse.rs\n".into(),
        workspace: None,
        created_by: None,
    }
}

fn round_trip<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(v: T) {
    let json = serde_json::to_string(&v).unwrap();
    let back: T = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back, "{json}");
}

#[test]
fn every_message_round_trips() {
    let info = RunnerInfo {
        name: "compute".into(),
        os: "windows".into(),
        arch: "x86_64".into(),
        executors: vec![Executor::Claude],
        slots: 2,
        ephemeral: true,
        capabilities: vec!["os:windows".into(), "docker".into()],
        cpu_cores: 8,
        memory_mb: 32_768,
    };
    let result = TaskResult {
        branch: "lgtm/0123abcd".into(),
        diff: "--- a\n+++ b\n".into(),
        changed_files: vec!["HEALTH.md".into()],
        validation: vec![ValidationResult {
            name: "test".into(),
            command: "bun test".into(),
            ok: false,
            output_tail: "1 failed".into(),
        }],
        plan: Some(Plan {
            steps: vec![PlanStep {
                key: "schema".into(),
                title: "Add schema".into(),
                prompt: "Add the table".into(),
                depends_on: vec![],
            }],
        }),
        review: Some(Review {
            findings: vec![Finding {
                severity: Severity::Blocking,
                file: "src/a.rs".into(),
                line: Some(3),
                message: "unwrap on user input".into(),
            }],
            executor: Some(Executor::Codex),
        }),
        policy: Some(Policy {
            auto_approve: true,
            auto_merge: false,
            max_diff_lines: Some(300),
            protected_files: vec!["migrations/*".into()],
            budget_per_task_usd: Some(2.0),
            reassign: 1,
            budget_daily_usd: Some(50.0),
        }),
        cost_usd: 0.42,
    };
    assert!(result.review.as_ref().unwrap().has_blocking());
    assert!(result.validation_failed());
    for event in [
        TaskEvent::Started {
            model: Some("opus".into()),
        },
        TaskEvent::Message {
            text: "use the existing helper".into(),
        },
        TaskEvent::Output {
            stream: OutputStream::Stdout,
            line: "{}".into(),
        },
        TaskEvent::Command {
            command: "cargo test".into(),
        },
        TaskEvent::FileChanged {
            path: "src/lib.rs".into(),
        },
        TaskEvent::Progress {
            text: "reading the config".into(),
        },
        TaskEvent::Scratchpad {
            content: "- the parser is in src/parse.rs\n".into(),
        },
        TaskEvent::Validating {
            names: vec!["test".into(), "lint".into()],
        },
        TaskEvent::NetworkDenied {
            host: "evil.example".into(),
        },
        TaskEvent::PermissionRequested {
            kind: "network".into(),
            target: "registry.internal".into(),
            reason: "install a private package".into(),
        },
        TaskEvent::HostAllowed {
            host: "registry.internal".into(),
        },
        TaskEvent::Completed {
            result: result.clone(),
        },
        TaskEvent::Failed {
            error: "boom".into(),
        },
        TaskEvent::TimedOut { secs: 3600 },
        TaskEvent::RunnerLost,
        TaskEvent::Cancelled,
        TaskEvent::Retry {
            attempt: 1,
            reason: "checks failed".into(),
        },
        TaskEvent::Requeued {
            runner: Some("compute".into()),
            executor: Executor::Codex,
        },
        TaskEvent::Requeued {
            runner: None,
            executor: Executor::Claude,
        },
        TaskEvent::PolicyDecision {
            action: "approve".into(),
            allowed: false,
            reasons: vec!["touches protected file migrations/001.sql".into()],
        },
        TaskEvent::PolicyDecision {
            action: "merge".into(),
            allowed: true,
            reasons: vec!["checks passed".into(), "no blocking findings".into()],
        },
        TaskEvent::AutoApproved,
        TaskEvent::AutoMerged,
        TaskEvent::Conflicted {
            base: "main".into(),
            files: vec!["src/lib.rs".into()],
        },
        TaskEvent::Pushed {
            branch: "lgtm/0123abcd".into(),
            sha: "abc123".into(),
        },
        TaskEvent::Discarded,
    ] {
        round_trip(StoredEvent {
            at: 2,
            event: event.clone(),
        });
        round_trip(RunnerMessage::Event {
            task_id: "0123abcd".into(),
            event,
        });
    }
    round_trip(RunnerMessage::Hello {
        token: "t".into(),
        info: info.clone(),
        running: vec!["0123abcd".into()],
        version: PROTOCOL_VERSION,
    });
    round_trip(RunnerMessage::Goodbye);
    round_trip(RunnerStatus {
        info,
        running: vec!["0123abcd".into()],
    });
    round_trip(Batch {
        id: "b1".into(),
        created_at: 3,
        source: BatchSource::GithubLabel {
            owner: "o".into(),
            repo: "r".into(),
            label: "P1".into(),
        },
        repository: "https://github.com/o/r.git".into(),
        task_ids: vec!["0123abcd".into()],
        approve_plans: true,
        workspace: None,
        created_by: None,
    });
    round_trip(BatchSource::Linear {
        team: "ENG".into(),
        state: "Todo".into(),
    });
    round_trip(BatchSummary::default());
    for msg in [
        OrchestratorMessage::HelloAck,
        OrchestratorMessage::Start {
            task: Box::new(sample_task()),
            memories: vec![memory(Some("r"), "ship small commits")],
        },
        OrchestratorMessage::Cancel {
            task_id: "0123abcd".into(),
        },
        OrchestratorMessage::Interrupt {
            task_id: "0123abcd".into(),
        },
        OrchestratorMessage::Message {
            task_id: "0123abcd".into(),
            text: "again".into(),
            memories: vec![memory(None, "deploys are manual")],
            task: None,
        },
        OrchestratorMessage::Message {
            task_id: "0123abcd".into(),
            text: "again, with an allowed host".into(),
            memories: vec![],
            task: Some(Box::new(sample_task())),
        },
        OrchestratorMessage::Push {
            task_id: "0123abcd".into(),
            token: None,
        },
        OrchestratorMessage::Push {
            task_id: "0123abcd".into(),
            token: Some("ghs_abc".into()),
        },
        OrchestratorMessage::Discard {
            task_id: "0123abcd".into(),
        },
        OrchestratorMessage::Rejected {
            reason: "protocol version 0, this orchestrator speaks 1".into(),
        },
        OrchestratorMessage::TerminalOpen {
            task_id: "0123abcd".into(),
        },
        OrchestratorMessage::TerminalInput {
            task_id: "0123abcd".into(),
            data: "ls\r".into(),
        },
        OrchestratorMessage::TerminalClose {
            task_id: "0123abcd".into(),
        },
    ] {
        round_trip(msg);
    }
}

#[test]
fn terminal_frames_round_trip() {
    round_trip(RunnerMessage::Terminal {
        task_id: "0123abcd".into(),
        data: "$ ".into(),
    });
    round_trip(RunnerMessage::TerminalClosed {
        task_id: "0123abcd".into(),
    });
}

#[test]
fn phase_one_frames_still_parse() {
    let info: RunnerInfo =
        serde_json::from_str(r#"{"name":"w","os":"linux","arch":"x86_64","executors":["claude"]}"#)
            .unwrap();
    assert_eq!(info.slots, 1);
    assert!(!info.ephemeral);
    assert!(info.capabilities.is_empty());
    assert_eq!(info.cpu_cores, 0);
    assert_eq!(info.memory_mb, 0);
    let hello: RunnerMessage = serde_json::from_str(
        r#"{"type":"hello","token":"t","info":{"name":"w","os":"linux","arch":"x86_64","executors":[]}}"#,
    )
    .unwrap();
    assert!(matches!(hello, RunnerMessage::Hello { running, .. } if running.is_empty()));
    let result: TaskResult =
        serde_json::from_str(r#"{"branch":"lgtm/0123abcd","diff":"","changed_files":[]}"#).unwrap();
    assert!(result.validation.is_empty());
    assert!(result.review.is_none() && result.policy.is_none() && result.cost_usd == 0.0);
    let task: Task = serde_json::from_str(
        r#"{"id":"0123abcd","spec":{"repository":"r","base_branch":"main","prompt":"p","executor":"claude","runner":null},"status":"approved","runner":"w","created_at":1,"result":null,"error":null}"#,
    )
    .unwrap();
    assert!(task.pull_request.is_none() && task.ci.is_none() && task.spec.issue.is_none());
    assert!(task.spec.linear.is_none());
    assert_eq!(task.spec.kind, TaskKind::Run);
    assert!(task.spec.parent.is_none() && task.spec.depends_on.is_empty());
    assert!(task.spec.batch.is_none() && task.spec.goal.is_none());
    assert!(task.executions.is_empty());
    assert!(task.scratchpad.is_empty());
    assert!(task.spec.sandbox.is_none());
    assert!(task.spec.requirements.is_empty());
    assert!(task.spec.model.is_none());
    let pushed: TaskEvent = serde_json::from_str(r#"{"type":"pushed","branch":"b"}"#).unwrap();
    assert!(matches!(pushed, TaskEvent::Pushed { sha, .. } if sha.is_empty()));
    let started: TaskEvent = serde_json::from_str(r#"{"type":"started"}"#).unwrap();
    assert!(matches!(started, TaskEvent::Started { model: None }));
}

fn memory(repository: Option<&str>, content: &str) -> Memory {
    Memory {
        id: "0123abcd".into(),
        repository: repository.map(String::from),
        content: content.into(),
        created_at: 1,
        source: MemorySource::User,
        verification: Verification::UserApproved,
        proposed_by: None,
        workspace: None,
        created_by: None,
    }
}

#[test]
fn memory_is_told_to_its_repository_or_all() {
    let scoped = memory(Some("https://github.com/arsenstorm/lgtm.git"), "c");
    assert!(scoped.is_told_to("https://github.com/arsenstorm/lgtm.git"));
    assert!(!scoped.is_told_to("https://github.com/arsenstorm/other.git"));
    assert!(memory(None, "c").is_told_to("anything"));
}

#[test]
fn memory_is_not_told_to_anyone_until_approved() {
    let mut proposed = memory(None, "c");
    proposed.verification = Verification::AgentProposed;
    assert!(!proposed.is_told_to("anything"));
}

#[test]
fn memory_round_trips_source_and_proposal() {
    let mut memory = memory(Some("https://github.com/arsenstorm/lgtm.git"), "c");
    memory.source = MemorySource::Agent;
    memory.verification = Verification::AgentProposed;
    memory.proposed_by = Some("t1".into());
    let json = serde_json::to_string(&memory).unwrap();
    assert_eq!(serde_json::from_str::<Memory>(&json).unwrap(), memory);
}

#[test]
fn memory_without_verification_defaults_to_approved() {
    let memory: Memory =
        serde_json::from_str(r#"{"id":"0123abcd","repository":null,"content":"c","created_at":1}"#)
            .unwrap();
    assert_eq!(memory.source, MemorySource::User);
    assert_eq!(memory.verification, Verification::UserApproved);
    assert_eq!(memory.proposed_by, None);
    assert_eq!(memory.workspace, None);
}

#[test]
fn knowledge_block_is_empty_without_memories() {
    assert_eq!(knowledge_block(&[]), "");
}

#[test]
fn knowledge_block_lists_every_memory() {
    let memories = [memory(None, "deploys are manual"), memory(None, "no yarn")];
    assert_eq!(
        knowledge_block(&memories),
        "Project knowledge (from the team, treat as fact):\n- deploys are manual\n- no yarn\n\n"
    );
}

#[test]
fn hello_without_version_defaults_to_zero() {
    let hello: RunnerMessage = serde_json::from_str(
        r#"{"type":"hello","token":"t","info":{"name":"w","os":"linux","arch":"x86_64","executors":[]}}"#,
    )
    .unwrap();
    assert!(matches!(hello, RunnerMessage::Hello { version: 0, .. }));
}

#[test]
fn todo_round_trips() {
    round_trip(Todo {
        id: "0123abcd".into(),
        repository: Some("https://github.com/arsenstorm/lgtm.git".into()),
        title: "add a /health endpoint".into(),
        description: "should return 200 while runners are connected".into(),
        status: TodoStatus::InProgress,
        created_at: 1,
        task: Some("11111111".into()),
        priority: Priority::High,
        assignee: Some("arsen".into()),
        blockers: vec!["22222222".into()],
        workspace: None,
        created_by: None,
    });
}

#[test]
fn todo_without_new_fields_defaults() {
    let todo: Todo = serde_json::from_str(
        r#"{"id":"0123abcd","repository":null,"title":"t","status":"open","created_at":1}"#,
    )
    .unwrap();
    assert_eq!(todo.priority, Priority::Medium);
    assert_eq!(todo.assignee, None);
    assert!(todo.blockers.is_empty());
}

#[test]
fn todo_is_blocked_only_by_an_unfinished_blocker() {
    let mut blocker = Todo {
        id: "blocker1".into(),
        repository: None,
        title: "t".into(),
        description: String::new(),
        status: TodoStatus::Open,
        created_at: 1,
        task: None,
        priority: Priority::Medium,
        assignee: None,
        blockers: Vec::new(),
        workspace: None,
        created_by: None,
    };
    let todo = Todo {
        id: "todo1".into(),
        repository: None,
        title: "t".into(),
        description: String::new(),
        status: TodoStatus::Open,
        created_at: 1,
        task: None,
        priority: Priority::Medium,
        assignee: None,
        blockers: vec![blocker.id.clone()],
        workspace: None,
        created_by: None,
    };
    let mut todos = HashMap::new();
    todos.insert(blocker.id.clone(), blocker.clone());
    assert!(todo.is_blocked(&todos));

    blocker.status = TodoStatus::Done;
    todos.insert(blocker.id.clone(), blocker);
    assert!(!todo.is_blocked(&todos));
}

#[test]
fn todo_patch_round_trips_and_distinguishes_absent_from_clearing() {
    round_trip(TodoPatch {
        priority: Some(Priority::High),
        assignee: Some(Some("arsen".into())),
        blockers: Some(vec!["11111111".into()]),
    });

    let absent: TodoPatch = serde_json::from_str("{}").unwrap();
    assert_eq!(absent.assignee, None);

    let cleared: TodoPatch = serde_json::from_str(r#"{"assignee":null}"#).unwrap();
    assert_eq!(cleared.assignee, Some(None));
}

#[test]
fn session_round_trips() {
    round_trip(Session {
        id: "0123abcd".into(),
        repository: "https://github.com/arsenstorm/lgtm.git".into(),
        base_branch: "main".into(),
        title: "add a /health endpoint".into(),
        created_at: 1,
        workspace: None,
        created_by: None,
    });
}

#[test]
fn session_detail_round_trips() {
    round_trip(SessionDetail {
        session: Session {
            id: "0123abcd".into(),
            repository: "https://github.com/arsenstorm/lgtm.git".into(),
            base_branch: "main".into(),
            title: "add a /health endpoint".into(),
            created_at: 1,
            workspace: None,
            created_by: None,
        },
        tasks: vec![sample_task()],
    });
}

#[test]
fn rejected_round_trips() {
    round_trip(OrchestratorMessage::Rejected {
        reason: "protocol version 0, this orchestrator speaks 1".into(),
    });
    let json =
        serde_json::to_string(&OrchestratorMessage::Rejected { reason: "r".into() }).unwrap();
    assert_eq!(json, r#"{"type":"rejected","reason":"r"}"#);
}

#[test]
fn sandbox_profile_round_trips_through_its_wire_name() {
    for (profile, name) in [
        (SandboxProfile::Off, "off"),
        (SandboxProfile::Standard, "standard"),
        (SandboxProfile::Strict, "strict"),
        (SandboxProfile::Custom, "custom"),
    ] {
        assert_eq!(SandboxProfile::parse(name), Some(profile));
        assert_eq!(profile.as_str(), name);
    }
    assert_eq!(SandboxProfile::parse("x"), None);
}

fn runner(capabilities: Vec<String>, cpu_cores: u32, memory_mb: u64) -> RunnerInfo {
    RunnerInfo {
        name: "w".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        executors: vec![],
        slots: 1,
        ephemeral: false,
        capabilities,
        cpu_cores,
        memory_mb,
    }
}

#[test]
fn has_all_requires_every_requirement_in_capabilities() {
    let info = runner(vec!["os:linux".into(), "docker".into()], 0, 0);
    assert!(info.has_all(&[]));
    assert!(info.has_all(&["docker".into()]));
    assert!(!info.has_all(&["node".into()]));
}

#[test]
fn has_all_compares_memory_mb_and_cpu_cores_numerically() {
    let info = runner(vec![], 8, 16_384);
    assert!(info.has_all(&["cpu_cores:8".into(), "memory_mb:16384".into()]));
    assert!(info.has_all(&["cpu_cores:4".into(), "memory_mb:1024".into()]));
    assert!(!info.has_all(&["cpu_cores:16".into()]));
    assert!(!info.has_all(&["memory_mb:32768".into()]));
}

#[test]
fn has_all_never_matches_a_malformed_number() {
    let info = runner(vec![], 8, 16_384);
    assert!(!info.has_all(&["cpu_cores:many".into()]));
    assert!(!info.has_all(&["memory_mb:".into()]));
}

#[test]
fn tags_are_snake_case_type_fields() {
    let json = serde_json::to_string(&OrchestratorMessage::HelloAck).unwrap();
    assert_eq!(json, r#"{"type":"hello_ack"}"#);
    let json = serde_json::to_string(&TaskStatus::AwaitingReview).unwrap();
    assert_eq!(json, r#""awaiting_review""#);
}

fn sample_goal() -> Goal {
    Goal {
        id: "g1".into(),
        objective: "ship it".into(),
        repository: "https://example.com/r.git".into(),
        created_at: 1,
        attention: None,
        workspace: None,
        created_by: None,
    }
}

/// `goal_status` borrows its tasks, so they have to outlive the call.
fn goal_of(tasks: &[(TaskKind, TaskStatus)]) -> GoalStatus {
    let tasks: Vec<Task> = tasks
        .iter()
        .map(|&(kind, status)| {
            let mut task = sample_task();
            task.spec.kind = kind;
            task.status = status;
            task
        })
        .collect();
    goal_status(&sample_goal(), &tasks.iter().collect::<Vec<&Task>>())
}

fn sample_plan() -> Plan {
    Plan {
        steps: vec![PlanStep {
            key: "a".into(),
            title: "do a".into(),
            prompt: "do a".into(),
            depends_on: Vec::new(),
        }],
    }
}

fn completed_event(at: u64, plan: Option<Plan>) -> StoredEvent {
    StoredEvent {
        at,
        event: TaskEvent::Completed {
            result: TaskResult {
                branch: "lgtm/0123abcd".into(),
                diff: String::new(),
                changed_files: Vec::new(),
                validation: Vec::new(),
                plan,
                review: None,
                policy: None,
                cost_usd: 0.0,
            },
        },
    }
}

#[test]
fn plan_versions_supersedes_every_version_but_the_last() {
    let mut task = sample_task();
    task.status = TaskStatus::AwaitingReview;
    let events = vec![
        completed_event(1, Some(sample_plan())),
        completed_event(2, Some(sample_plan())),
    ];
    let versions = plan_versions(&task, &events);
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].version, 1);
    assert_eq!(versions[0].status, PlanStatus::Superseded);
    assert_eq!(versions[0].task, task.id);
    assert_eq!(versions[0].goal, task.spec.goal);
    assert_eq!(versions[1].version, 2);
    assert_eq!(versions[1].status, PlanStatus::AwaitingApproval);
    assert_eq!(versions[1].created_at, 2);
}

#[test]
fn plan_versions_empty_for_a_run_task() {
    let mut task = sample_task();
    task.spec.kind = TaskKind::Run;
    let events = vec![completed_event(1, None)];
    assert!(plan_versions(&task, &events).is_empty());
}

#[test]
fn plan_versions_status_follows_task_status() {
    use TaskStatus::*;
    let mapping = [
        (AwaitingReview, PlanStatus::AwaitingApproval),
        (Approved, PlanStatus::Approved),
        (Merged, PlanStatus::Approved),
        (Rejected, PlanStatus::Rejected),
        (Running, PlanStatus::Replanning),
        (Queued, PlanStatus::Replanning),
        (Failed, PlanStatus::Rejected),
        (TimedOut, PlanStatus::Rejected),
        (RunnerLost, PlanStatus::Rejected),
        (Cancelled, PlanStatus::Rejected),
    ];
    for (status, expected) in mapping {
        let mut task = sample_task();
        task.status = status;
        let versions = plan_versions(&task, &[completed_event(1, Some(sample_plan()))]);
        assert_eq!(versions[0].status, expected, "{status:?}");
    }
}

#[test]
fn goal_status_derives_each_arm() {
    use TaskKind::{Plan as P, Run as R};
    use TaskStatus::*;
    assert_eq!(goal_of(&[]), GoalStatus::Draft);
    assert_eq!(goal_of(&[(P, Queued), (R, Approved)]), GoalStatus::Planning);
    assert_eq!(goal_of(&[(P, Approved), (R, Running)]), GoalStatus::Running);
    assert_eq!(
        goal_of(&[(R, AwaitingReview), (R, Merged)]),
        GoalStatus::Review
    );
    assert_eq!(
        goal_of(&[(R, Approved), (R, Merged)]),
        GoalStatus::Completed
    );
    assert_eq!(
        goal_of(&[(R, Cancelled), (R, Rejected)]),
        GoalStatus::Cancelled
    );
    assert_eq!(goal_of(&[(R, Failed), (R, Merged)]), GoalStatus::Blocked);
}

#[test]
fn attention_outranks_every_other_arm() {
    let goal = Goal {
        attention: Some("needs a person".into()),
        ..sample_goal()
    };
    let mut task = sample_task();
    task.status = TaskStatus::Running;
    assert_eq!(goal_status(&goal, &[&task]), GoalStatus::Blocked);
    assert_eq!(goal_status(&goal, &[]), GoalStatus::Blocked);
}

/// A run task with this prompt, status, and error.
fn attention_task(prompt: &str, status: TaskStatus, error: Option<&str>) -> Task {
    let mut task = sample_task();
    task.spec.kind = TaskKind::Run;
    task.spec.prompt = prompt.into();
    task.status = status;
    task.error = error.map(str::to_string);
    task
}

fn changed(id: &str, status: TaskStatus, repository: &str, files: &[&str]) -> Task {
    let mut task = sample_task();
    task.id = id.into();
    task.status = status;
    task.spec.repository = repository.into();
    task.result = Some(TaskResult {
        branch: format!("lgtm/{id}"),
        diff: String::new(),
        changed_files: files.iter().map(|f| (*f).to_string()).collect(),
        validation: Vec::new(),
        plan: None,
        review: None,
        policy: None,
        cost_usd: 0.0,
    });
    task
}

#[test]
fn attention_names_the_task_and_why() {
    let task = attention_task("add a /health endpoint", TaskStatus::Running, None);
    let result = TaskResult {
        branch: "lgtm/0123abcd".into(),
        diff: String::new(),
        changed_files: vec![],
        validation: vec![],
        plan: None,
        review: None,
        policy: None,
        cost_usd: 0.,
    };
    let cases = [
        (
            TaskEvent::Completed { result },
            "add a /health endpoint: ready for review",
        ),
        (
            TaskEvent::Failed {
                error: "cargo test failed\nsecond line".into(),
            },
            "add a /health endpoint: failed: cargo test failed",
        ),
        (
            TaskEvent::TimedOut { secs: 3600 },
            "add a /health endpoint: timed out after 3600s",
        ),
        (TaskEvent::RunnerLost, "add a /health endpoint: runner lost"),
        (
            TaskEvent::AutoMerged,
            "add a /health endpoint: merged by policy",
        ),
        (
            TaskEvent::PermissionRequested {
                kind: "network".into(),
                target: "registry.internal".into(),
                reason: "install a private package".into(),
            },
            "add a /health endpoint: asks for registry.internal",
        ),
        (
            TaskEvent::Conflicted {
                base: "main".into(),
                files: vec!["src/lib.rs".into()],
            },
            "add a /health endpoint: conflicts with main",
        ),
        (
            TaskEvent::PrReviewed {
                state: ReviewState::ChangesRequested,
                url: "https://github.com/o/r/pull/1#pullrequestreview-1".into(),
            },
            "add a /health endpoint: PR review requested changes",
        ),
    ];
    for (event, expected) in cases {
        assert_eq!(attention(&task, &event).as_deref(), Some(expected));
    }
}

#[test]
fn an_approved_review_wants_nobody() {
    let task = attention_task("p", TaskStatus::Running, None);
    let event = TaskEvent::PrReviewed {
        state: ReviewState::Approved,
        url: "https://github.com/o/r/pull/1#pullrequestreview-1".into(),
    };
    assert_eq!(attention(&task, &event), None);
}

#[test]
fn routine_events_want_nobody() {
    let task = attention_task("p", TaskStatus::Running, None);
    for event in [
        TaskEvent::Started { model: None },
        TaskEvent::AutoApproved,
        TaskEvent::Cancelled,
        TaskEvent::Discarded,
        TaskEvent::Progress { text: "x".into() },
    ] {
        assert_eq!(attention(&task, &event), None);
    }
}

#[test]
fn a_plan_task_says_the_plan_is_ready() {
    let mut task = attention_task("design the api", TaskStatus::AwaitingReview, None);
    task.spec.kind = TaskKind::Plan;
    assert_eq!(
        attention_for_status(&task, TaskStatus::Running).as_deref(),
        Some("design the api: plan ready")
    );
}

#[test]
fn the_title_is_the_first_line_cut_to_sixty_characters() {
    let prompt = format!("{}\nrest", "x".repeat(80));
    let task = attention_task(&prompt, TaskStatus::Failed, None);
    let expected = format!("{}: failed", "x".repeat(60));
    assert_eq!(
        attention_for_status(&task, TaskStatus::Running).as_deref(),
        Some(expected.as_str())
    );
}

#[test]
fn only_a_changed_status_wants_a_person() {
    let task = attention_task("p", TaskStatus::AwaitingReview, None);
    assert_eq!(
        attention_for_status(&task, TaskStatus::AwaitingReview),
        None
    );
    assert_eq!(
        attention_for_status(&task, TaskStatus::Running).as_deref(),
        Some("p: ready for review")
    );
    let running = attention_task("p", TaskStatus::Running, None);
    assert_eq!(attention_for_status(&running, TaskStatus::Queued), None);
    let failed = attention_task("p", TaskStatus::Failed, Some("boom\nmore"));
    assert_eq!(
        attention_for_status(&failed, TaskStatus::Running).as_deref(),
        Some("p: failed: boom")
    );
}

#[test]
fn a_conflicted_status_names_the_base_branch() {
    let task = attention_task("p", TaskStatus::Conflicted, None);
    assert_eq!(
        attention_for_status(&task, TaskStatus::Running).as_deref(),
        Some("p: conflicts with main")
    );
}

#[test]
fn overlaps_only_with_live_tasks_in_the_same_repository() {
    let mine = changed(
        "aaaaaaaa",
        TaskStatus::AwaitingReview,
        "r",
        &["b.rs", "a.rs"],
    );
    let live = changed(
        "bbbbbbbb",
        TaskStatus::Running,
        "r",
        &["c.rs", "b.rs", "a.rs"],
    );
    let done = changed("cccccccc", TaskStatus::Merged, "r", &["a.rs"]);
    let elsewhere = changed("dddddddd", TaskStatus::Running, "other", &["a.rs"]);
    let apart = changed("eeeeeeee", TaskStatus::Running, "r", &["z.rs"]);
    let mut unstarted = changed("ffffffff", TaskStatus::Queued, "r", &["a.rs"]);
    unstarted.result = None;

    let others = [&mine, &live, &done, &elsewhere, &apart, &unstarted];
    assert_eq!(
        overlaps(&mine, &others),
        vec![Overlap {
            task: "bbbbbbbb".into(),
            files: vec!["a.rs".into(), "b.rs".into()],
        }]
    );
    assert!(overlaps(&apart, &others).is_empty());
}

#[test]
fn conflicted_is_live_work_and_keeps_its_wire_name() {
    assert!(!TaskStatus::Conflicted.is_terminal());
    let json = serde_json::to_string(&TaskStatus::Conflicted).unwrap();
    assert_eq!(json, "\"conflicted\"");
    assert_eq!(
        serde_json::from_str::<TaskStatus>(&json).unwrap(),
        TaskStatus::Conflicted
    );
}

#[test]
fn overlap_round_trips() {
    round_trip(Overlap {
        task: "0123abcd".into(),
        files: vec!["a.rs".into()],
    });
}

#[test]
fn depends_on_met_widens_with_the_condition() {
    let met = |cond: DependsOn, status: TaskStatus| cond.met(status);

    assert!(met(DependsOn::Approved, TaskStatus::Approved));
    assert!(met(DependsOn::Approved, TaskStatus::Merged));
    assert!(!met(DependsOn::Approved, TaskStatus::AwaitingReview));

    assert!(met(DependsOn::Completed, TaskStatus::AwaitingReview));
    assert!(met(DependsOn::Completed, TaskStatus::ChangesRequested));
    assert!(met(DependsOn::Completed, TaskStatus::Conflicted));
    assert!(met(DependsOn::Completed, TaskStatus::Approved));
    assert!(met(DependsOn::Completed, TaskStatus::Merged));
    assert!(!met(DependsOn::Completed, TaskStatus::Running));

    assert!(met(DependsOn::Merged, TaskStatus::Merged));
    assert!(!met(DependsOn::Merged, TaskStatus::Approved));
}

#[test]
fn pending_requests_are_distinct_and_exclude_already_allowed_hosts() {
    let mut spec = sample_task().spec;
    spec.allowed_hosts = vec!["already.example".into()];
    let events: Vec<StoredEvent> = [
        TaskEvent::NetworkDenied {
            host: "blocked.example".into(),
        },
        TaskEvent::NetworkDenied {
            host: "blocked.example".into(),
        },
        TaskEvent::PermissionRequested {
            kind: "network".into(),
            target: "registry.internal".into(),
            reason: "install a private package".into(),
        },
        TaskEvent::NetworkDenied {
            host: "already.example".into(),
        },
        TaskEvent::PermissionRequested {
            kind: "shell".into(),
            target: "docker".into(),
            reason: "build a container".into(),
        },
    ]
    .into_iter()
    .map(|event| StoredEvent { at: 0, event })
    .collect();
    assert_eq!(
        pending_requests(&events, &spec),
        vec![
            (
                "blocked.example".to_string(),
                "refused by the allowlist".to_string()
            ),
            (
                "registry.internal".to_string(),
                "install a private package".to_string()
            ),
        ]
    );
}

#[test]
fn worker_keyed_json_still_parses_as_runner() {
    let spec: TaskSpec = serde_json::from_str(
        r#"{"repository":"r","base_branch":"main","prompt":"p","executor":"claude","worker":"compute"}"#,
    )
    .unwrap();
    assert_eq!(spec.runner.as_deref(), Some("compute"));
    let task: Task = serde_json::from_str(
        r#"{"id":"0123abcd","spec":{"repository":"r","base_branch":"main","prompt":"p","executor":"claude","worker":null},"status":"approved","worker":"w","created_at":1,"result":null,"error":null}"#,
    )
    .unwrap();
    assert_eq!(task.runner.as_deref(), Some("w"));
    let requeued: TaskEvent =
        serde_json::from_str(r#"{"type":"requeued","worker":"w","executor":"claude"}"#).unwrap();
    assert!(
        matches!(requeued, TaskEvent::Requeued { runner, .. } if runner.as_deref() == Some("w"))
    );
}
