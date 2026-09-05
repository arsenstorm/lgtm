//! A task's attempt history, derived here rather than reported: the runner
//! knows nothing about attempts, it only sends events.

use lgtm_protocol::{Execution, ExecutionStatus, SkillRef, Task, TaskEvent};

/// Folds one event into `task.executions`.
pub fn record(task: &mut Task, event: &TaskEvent, now: u64) {
    if let TaskEvent::Started { model, skills } = event {
        return start(task, model.clone(), skills.clone(), now);
    }
    let Some(exec) = task
        .executions
        .last_mut()
        .filter(|exec| exec.status == ExecutionStatus::Running)
    else {
        return;
    };
    match event {
        TaskEvent::Retry { reason, .. } => {
            finish(exec, ExecutionStatus::Failed, now);
            exec.error = Some(reason.clone());
        }
        TaskEvent::Failed { error } => {
            finish(exec, ExecutionStatus::Failed, now);
            exec.error = Some(error.clone());
        }
        TaskEvent::TimedOut { secs } => {
            finish(exec, ExecutionStatus::Failed, now);
            exec.error = Some(format!("timed out after {secs}s"));
        }
        TaskEvent::RunnerLost => {
            finish(exec, ExecutionStatus::Failed, now);
            exec.error = Some("runner lost".into());
        }
        TaskEvent::Artefact { name, .. } if !exec.artefacts.contains(name) => {
            exec.artefacts.push(name.clone())
        }
        TaskEvent::Cancelled => finish(exec, ExecutionStatus::Cancelled, now),
        TaskEvent::Completed { result } => {
            finish(exec, ExecutionStatus::Completed, now);
            exec.cost_usd = result.cost_usd;
            exec.validation = result.validation.clone();
        }
        _ => {}
    }
}

/// A fix-the-checks or review run reports `Started` again inside the attempt
/// that is already open, so only a closed history opens a new one.
fn start(task: &mut Task, model: Option<String>, skills: Vec<SkillRef>, now: u64) {
    let open = task
        .executions
        .last()
        .is_some_and(|exec| exec.status == ExecutionStatus::Running);
    if open {
        return;
    }
    task.executions.push(Execution {
        attempt: task.executions.len() as u32 + 1,
        runner: task.runner.clone().unwrap_or_default(),
        executor: task.spec.executor,
        model,
        started_at: now,
        finished_at: None,
        status: ExecutionStatus::Running,
        error: None,
        cost_usd: 0.0,
        validation: Vec::new(),
        artefacts: Vec::new(),
        skills,
    });
}

fn finish(exec: &mut Execution, status: ExecutionStatus, now: u64) {
    exec.finished_at = Some(now);
    exec.status = status;
}

#[cfg(test)]
#[path = "execution_tests.rs"]
mod tests;
