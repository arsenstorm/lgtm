//! Linear side effects: moving the issue and commenting on it. Every call here
//! runs off the state lock, on its own task, and never changes a task's status.

use std::sync::Arc;

use lgtm_protocol::{LinearRef, TaskId, TaskStatus};

use crate::state::{linear_sync_plan, App, LinearSync};

/// Reads the plan for a transition that just happened and hands it to `sync`.
/// A no-op when Linear is off or the task did not come from an issue.
pub fn after_transition(app: &Arc<App>, task_id: &str, previous: TaskStatus, pr_recorded: bool) {
    if app.linear.is_none() {
        return;
    }
    let planned = {
        let state = app.state.lock().unwrap();
        state.tasks.get(task_id).and_then(|rec| {
            let plan = linear_sync_plan(&rec.task, previous, pr_recorded);
            let linear_ref = rec.task.spec.linear.clone()?;
            (!plan.is_empty()).then_some((linear_ref, plan))
        })
    };
    if let Some((linear_ref, plan)) = planned {
        sync(app.clone(), task_id.to_string(), linear_ref, plan);
    }
}

/// Runs the plan against Linear. Failures are logged and dropped: a task's
/// progress does not depend on the issue tracker keeping up.
pub fn sync(app: Arc<App>, task_id: TaskId, linear_ref: LinearRef, plan: Vec<LinearSync>) {
    let Some(linear) = app.linear.clone() else {
        return;
    };
    tokio::spawn(async move {
        for item in plan {
            let result = match &item {
                LinearSync::Move(target) => move_issue(&linear, &linear_ref, *target).await,
                LinearSync::Comment(body) => linear.comment(&linear_ref.id, body).await,
            };
            if let Err(err) = result {
                tracing::warn!(
                    task = %task_id,
                    issue = %linear_ref.identifier,
                    %err,
                    "linear sync failed",
                );
            }
        }
    });
}

/// The issue is fetched again for its team, which is what workflow states hang
/// off. One extra call per transition is cheaper than tracking teams here.
async fn move_issue(
    linear: &lgtm_linear::Linear,
    linear_ref: &LinearRef,
    target: lgtm_linear::Target,
) -> anyhow::Result<()> {
    let issue = linear.issue(&linear_ref.identifier).await?;
    let states = linear.states(&issue.team_id).await?;
    let Some(state) = lgtm_linear::pick_state(&states, target) else {
        tracing::info!(
            issue = %linear_ref.identifier,
            ?target,
            "no workflow state matches, skipping",
        );
        return Ok(());
    };
    linear.move_issue(&linear_ref.id, &state.id).await
}
