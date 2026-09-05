//! Unit tests for `provenance.rs`: matching a commit back to the task, goal,
//! plan, review, decisions and approval that produced it.

use lgtm_protocol::{Executor, Finding, Review, Severity, TaskEvent};

use super::*;
use crate::state::tests::{children, connect, create, goal_spec, planned, run_result, step};
use crate::state::State;

fn push(state: &mut State, id: &str, sha: &str) {
    state.apply_event(
        id,
        TaskEvent::Pushed {
            branch: format!("lgtm/{id}"),
            sha: sha.into(),
        },
    );
}

#[test]
fn full_sha_match_carries_the_goal_review_and_decisions() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let (goal, spec) = goal_spec(&mut state);
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
        TaskEvent::PolicyDecision {
            action: "approve".into(),
            allowed: true,
            reasons: vec!["checks passed".into()],
        },
    );
    let mut result = run_result();
    result.review = Some(Review {
        findings: vec![Finding {
            severity: Severity::Warning,
            file: "a.rs".into(),
            line: None,
            message: "nit".into(),
        }],
        executor: Some(Executor::Claude),
    });
    state.apply_event(&id, TaskEvent::Completed { result });
    let sha = "deadbeefcafef00d1234567890abcdef12345678";
    push(&mut state, &id, sha);

    let found = find(&state, sha).unwrap();
    assert_eq!(found.task.id, id);
    assert_eq!(found.goal.map(|g| g.id), Some(goal));
    assert!(found.review.is_some(), "the completed result's review");
    assert_eq!(found.decisions.len(), 1);
    assert!(
        matches!(found.approval.unwrap().event, TaskEvent::Pushed { .. }),
        "no auto-approval was recorded, so the push itself is the approval"
    );
}

#[test]
fn prefix_match_needs_at_least_seven_characters() {
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
            result: run_result(),
        },
    );
    push(&mut state, &id, "deadbeefcafef00d");

    assert_eq!(find(&state, "deadbee").unwrap().task.id, id);
    assert!(
        find(&state, "deadbe").is_none(),
        "six characters is too short to trust as a prefix"
    );
}

#[test]
fn no_match_when_nothing_was_ever_pushed_with_that_sha() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    create(&mut state, Executor::Claude);
    assert!(find(&state, "0000000").is_none());
}

#[test]
fn approval_is_auto_approved_or_else_the_push_itself() {
    let mut state = State::default();
    let _w = connect(&mut state, "w", 1, 1);
    let plan = planned(&mut state, vec![step("a", &[])]);
    state.approve_plan(&plan).unwrap();
    let child = children(&state, &plan).remove(0);

    state.apply_event(
        &child.id,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        &child.id,
        TaskEvent::Completed {
            result: run_result(),
        },
    );
    state.apply_event(&child.id, TaskEvent::AutoApproved);
    let auto_sha = "cafefeed1234567890abcdef1234567890abcdef";
    push(&mut state, &child.id, auto_sha);

    let auto = find(&state, auto_sha).unwrap();
    assert!(matches!(
        auto.approval.unwrap().event,
        TaskEvent::AutoApproved
    ));
    assert_eq!(
        auto.plan.map(|v| v.task),
        Some(plan),
        "the child's plan is the one it was approved from"
    );

    let by_hand = create(&mut state, Executor::Claude).id;
    state.apply_event(
        &by_hand,
        TaskEvent::Started {
            model: None,
            skills: Vec::new(),
        },
    );
    state.apply_event(
        &by_hand,
        TaskEvent::Completed {
            result: run_result(),
        },
    );
    push(&mut state, &by_hand, "1234567abcdef");

    let found = find(&state, "1234567abcdef").unwrap();
    assert!(
        matches!(found.approval.unwrap().event, TaskEvent::Pushed { .. }),
        "a person approved this one, which leaves only the push behind"
    );
}
