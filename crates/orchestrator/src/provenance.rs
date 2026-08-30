//! `GET /api/provenance/:sha`: reconstructs why a commit exists from LGTM's
//! own records, for `lgtm why`. Pure: the caller hands over the state.

use lgtm_protocol::{plan_versions, PlanVersion, Provenance, StoredEvent, TaskEvent};

use crate::state::{State, TaskRecord};

/// Below this many characters a prefix is too likely to hit more than one
/// task by accident, so it isn't treated as a match.
const MIN_PREFIX: usize = 7;

fn sha_matches(recorded: &str, input: &str) -> bool {
    recorded == input || (input.len() >= MIN_PREFIX && recorded.starts_with(input))
}

fn pushed_matching(rec: &TaskRecord, sha: &str) -> bool {
    rec.events.iter().any(|stored| match &stored.event {
        TaskEvent::Pushed { sha: recorded, .. } => sha_matches(recorded, sha),
        _ => false,
    })
}

/// The task LGTM pushed `sha` from.
// ponytail: a commit that only exists as a pull request's squash-merge sha
// isn't found here, since LGTM never records that sha anywhere; only the
// pushed branch head is indexed. Add a merge-sha lookup via `pull_request`
// if that gap bites.
fn find_task<'a>(state: &'a State, sha: &str) -> Option<&'a TaskRecord> {
    state.tasks.values().find(|rec| pushed_matching(rec, sha))
}

/// The latest version of the plan `task` was a step of, when it was created
/// from one.
fn plan_for(state: &State, parent: &str) -> Option<PlanVersion> {
    let prec = state.tasks.get(parent)?;
    plan_versions(&prec.task, &prec.events)
        .into_iter()
        .next_back()
}

/// What approved the push: policy, when it recorded one, or else the push
/// itself — a person's approval leaves no event of its own.
fn approval(rec: &TaskRecord, sha: &str) -> Option<StoredEvent> {
    rec.events
        .iter()
        .find(|stored| matches!(stored.event, TaskEvent::AutoApproved))
        .or_else(|| {
            rec.events.iter().find(|stored| {
                matches!(&stored.event, TaskEvent::Pushed { sha: recorded, .. } if sha_matches(recorded, sha))
            })
        })
        .cloned()
}

pub fn find(state: &State, sha: &str) -> Option<Provenance> {
    let rec = find_task(state, sha)?;
    let task = rec.task.clone();
    let goal = task
        .spec
        .goal
        .as_deref()
        .and_then(|id| state.goals.get(id))
        .cloned();
    let plan = task
        .spec
        .parent
        .as_deref()
        .and_then(|parent| plan_for(state, parent));
    let review = task
        .result
        .as_ref()
        .and_then(|result| result.review.clone());
    let decisions = rec
        .events
        .iter()
        .filter(|stored| {
            matches!(
                stored.event,
                TaskEvent::PolicyDecision { .. } | TaskEvent::Orchestrated { .. }
            )
        })
        .cloned()
        .collect();
    let approval = approval(rec, sha);
    Some(Provenance {
        task,
        goal,
        plan,
        review,
        decisions,
        approval,
    })
}

#[cfg(test)]
#[path = "provenance_tests.rs"]
mod tests;
