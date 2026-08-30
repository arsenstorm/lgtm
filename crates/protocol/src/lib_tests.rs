use super::*;

fn sample_task() -> Task {
    Task {
        id: "0123abcd".into(),
        spec: TaskSpec {
            repository: "https://github.com/arsenstorm/lgtm.git".into(),
            base_branch: "main".into(),
            prompt: "add a /health endpoint".into(),
            executor: Executor::Claude,
            worker: Some("compute".into()),
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
            batch: Some("b1".into()),
            sandbox: Some(SandboxProfile::Strict),
            requirements: vec!["docker".into()],
            goal: Some("g1".into()),
        },
        status: TaskStatus::Queued,
        worker: None,
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
        executions: vec![Execution {
            attempt: 1,
            worker: "compute".into(),
            executor: Executor::Claude,
            started_at: 2,
            finished_at: Some(3),
            status: ExecutionStatus::Completed,
            error: None,
            cost_usd: 0.42,
            validation: Vec::new(),
        }],
    }
}

fn round_trip<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(v: T) {
    let json = serde_json::to_string(&v).unwrap();
    let back: T = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back, "{json}");
}

#[test]
fn every_message_round_trips() {
    let info = WorkerInfo {
        name: "compute".into(),
        os: "windows".into(),
        arch: "x86_64".into(),
        executors: vec![Executor::Claude],
        slots: 2,
        ephemeral: true,
        capabilities: vec!["os:windows".into(), "docker".into()],
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
        }),
        policy: Some(Policy {
            auto_approve: true,
            auto_merge: false,
        }),
        cost_usd: 0.42,
    };
    assert!(result.review.as_ref().unwrap().has_blocking());
    assert!(result.validation_failed());
    for event in [
        TaskEvent::Started,
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
        TaskEvent::Validating {
            names: vec!["test".into(), "lint".into()],
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
            worker: Some("compute".into()),
            executor: Executor::Codex,
        },
        TaskEvent::Requeued {
            worker: None,
            executor: Executor::Claude,
        },
        TaskEvent::AutoApproved,
        TaskEvent::AutoMerged,
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
        round_trip(WorkerMessage::Event {
            task_id: "0123abcd".into(),
            event,
        });
    }
    round_trip(WorkerMessage::Hello {
        token: "t".into(),
        info: info.clone(),
        running: vec!["0123abcd".into()],
        version: PROTOCOL_VERSION,
    });
    round_trip(WorkerMessage::Goodbye);
    round_trip(WorkerStatus {
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
        OrchestratorMessage::Message {
            task_id: "0123abcd".into(),
            text: "again".into(),
            memories: vec![memory(None, "deploys are manual")],
        },
        OrchestratorMessage::Push {
            task_id: "0123abcd".into(),
        },
        OrchestratorMessage::Discard {
            task_id: "0123abcd".into(),
        },
        OrchestratorMessage::Rejected {
            reason: "protocol version 0, this orchestrator speaks 1".into(),
        },
    ] {
        round_trip(msg);
    }
}

#[test]
fn phase_one_frames_still_parse() {
    let info: WorkerInfo =
        serde_json::from_str(r#"{"name":"w","os":"linux","arch":"x86_64","executors":["claude"]}"#)
            .unwrap();
    assert_eq!(info.slots, 1);
    assert!(!info.ephemeral);
    assert!(info.capabilities.is_empty());
    let hello: WorkerMessage = serde_json::from_str(
        r#"{"type":"hello","token":"t","info":{"name":"w","os":"linux","arch":"x86_64","executors":[]}}"#,
    )
    .unwrap();
    assert!(matches!(hello, WorkerMessage::Hello { running, .. } if running.is_empty()));
    let result: TaskResult =
        serde_json::from_str(r#"{"branch":"lgtm/0123abcd","diff":"","changed_files":[]}"#).unwrap();
    assert!(result.validation.is_empty());
    assert!(result.review.is_none() && result.policy.is_none() && result.cost_usd == 0.0);
    let task: Task = serde_json::from_str(
        r#"{"id":"0123abcd","spec":{"repository":"r","base_branch":"main","prompt":"p","executor":"claude","worker":null},"status":"approved","worker":"w","created_at":1,"result":null,"error":null}"#,
    )
    .unwrap();
    assert!(task.pull_request.is_none() && task.ci.is_none() && task.spec.issue.is_none());
    assert!(task.spec.linear.is_none());
    assert_eq!(task.spec.kind, TaskKind::Run);
    assert!(task.spec.parent.is_none() && task.spec.depends_on.is_empty());
    assert!(task.spec.batch.is_none() && task.spec.goal.is_none());
    assert!(task.executions.is_empty());
    assert!(task.spec.sandbox.is_none());
    assert!(task.spec.requirements.is_empty());
    let pushed: TaskEvent = serde_json::from_str(r#"{"type":"pushed","branch":"b"}"#).unwrap();
    assert!(matches!(pushed, TaskEvent::Pushed { sha, .. } if sha.is_empty()));
}

fn memory(repository: Option<&str>, content: &str) -> Memory {
    Memory {
        id: "0123abcd".into(),
        repository: repository.map(String::from),
        content: content.into(),
        created_at: 1,
    }
}

#[test]
fn memory_applies_to_its_repository_or_all() {
    let scoped = memory(Some("https://github.com/arsenstorm/lgtm.git"), "c");
    assert!(scoped.applies_to("https://github.com/arsenstorm/lgtm.git"));
    assert!(!scoped.applies_to("https://github.com/arsenstorm/other.git"));
    assert!(memory(None, "c").applies_to("anything"));
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
    let hello: WorkerMessage = serde_json::from_str(
        r#"{"type":"hello","token":"t","info":{"name":"w","os":"linux","arch":"x86_64","executors":[]}}"#,
    )
    .unwrap();
    assert!(matches!(hello, WorkerMessage::Hello { version: 0, .. }));
}

#[test]
fn todo_round_trips() {
    round_trip(Todo {
        id: "0123abcd".into(),
        repository: Some("https://github.com/arsenstorm/lgtm.git".into()),
        title: "add a /health endpoint".into(),
        description: "should return 200 while workers are connected".into(),
        status: TodoStatus::InProgress,
        created_at: 1,
        task: Some("11111111".into()),
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
    ] {
        assert_eq!(SandboxProfile::parse(name), Some(profile));
        assert_eq!(profile.as_str(), name);
    }
    assert_eq!(SandboxProfile::parse("x"), None);
}

#[test]
fn has_all_requires_every_requirement_in_capabilities() {
    let info = WorkerInfo {
        name: "w".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        executors: vec![],
        slots: 1,
        ephemeral: false,
        capabilities: vec!["os:linux".into(), "docker".into()],
    };
    assert!(info.has_all(&[]));
    assert!(info.has_all(&["docker".into()]));
    assert!(!info.has_all(&["node".into()]));
}

#[test]
fn tags_are_snake_case_type_fields() {
    let json = serde_json::to_string(&OrchestratorMessage::HelloAck).unwrap();
    assert_eq!(json, r#"{"type":"hello_ack"}"#);
    let json = serde_json::to_string(&TaskStatus::AwaitingReview).unwrap();
    assert_eq!(json, r#""awaiting_review""#);
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
    goal_status(&tasks.iter().collect::<Vec<&Task>>())
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
