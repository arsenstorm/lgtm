//! Plans and the dependencies they create: approving a plan task turns its
//! steps into ordinary tasks, and a task waits for the ones it depends on.

use std::collections::{HashMap, HashSet};

use lgtm_protocol::{Task, TaskEvent, TaskId, TaskKind, TaskSpec, TaskStatus};

use crate::state::{now_ms, CmdError, State, TaskRecord};

/// How a dependency's failure reads in the error of the tasks it blocked.
fn wire_status(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Rejected => "rejected",
        TaskStatus::Cancelled => "cancelled",
        _ => "failed",
    }
}

impl State {
    /// Whether every task `spec` depends on has been approved or merged.
    pub fn deps_met(&self, spec: &TaskSpec) -> bool {
        spec.depends_on.iter().all(|id| {
            self.tasks.get(id).is_some_and(|rec| {
                matches!(rec.task.status, TaskStatus::Approved | TaskStatus::Merged)
            })
        })
    }

    /// Fails every queued task waiting on `id`, which has just failed, been
    /// cancelled or been rejected. Their own dependents go the same way, since
    /// `apply_event` calls back here.
    pub fn fail_dependents(&mut self, id: &str) -> Vec<TaskId> {
        let Some(status) = self.tasks.get(id).map(|rec| wire_status(rec.task.status)) else {
            return Vec::new();
        };
        let mut blocked: Vec<TaskId> = self
            .tasks
            .values()
            .filter(|rec| {
                rec.task.status == TaskStatus::Queued
                    && rec.task.spec.depends_on.iter().any(|dep| dep == id)
            })
            .map(|rec| rec.task.id.clone())
            .collect();
        blocked.sort();
        let mut changed = Vec::new();
        for dependent in blocked {
            changed.extend(self.apply_event(
                &dependent,
                TaskEvent::Failed {
                    error: format!("dependency {id} {status}"),
                },
            ));
        }
        changed
    }

    /// Turns an approved plan's steps into queued tasks, wired to each other by
    /// their `depends_on` keys. Returns the plan task and the ids to persist.
    pub fn approve_plan(&mut self, id: &str) -> Result<(Task, Vec<TaskId>), CmdError> {
        let rec = self.tasks.get(id).ok_or(CmdError::NotFound)?;
        if rec.task.spec.kind != TaskKind::Plan {
            return Err(CmdError::Conflict("task is not a plan".into()));
        }
        if rec.task.status != TaskStatus::AwaitingReview {
            return Err(CmdError::Conflict("task is not awaiting review".into()));
        }
        let plan = rec
            .task
            .result
            .as_ref()
            .and_then(|result| result.plan.clone())
            .ok_or_else(|| CmdError::Conflict("plan task has no plan".into()))?;
        let spec = rec.task.spec.clone();

        // Nothing is created until every key resolves, so a bad plan leaves no
        // half-built graph behind.
        let mut seen: HashSet<&str> = HashSet::new();
        for step in &plan.steps {
            for key in &step.depends_on {
                if !seen.contains(key.as_str()) {
                    return Err(CmdError::Conflict(format!("unknown step key {key}")));
                }
            }
            seen.insert(step.key.as_str());
        }

        let created_at = now_ms();
        let mut ids: HashMap<&str, TaskId> = HashMap::new();
        let mut changed = vec![id.to_string()];
        for (index, step) in plan.steps.iter().enumerate() {
            let depends_on: Vec<TaskId> = step
                .depends_on
                .iter()
                .filter_map(|key| ids.get(key.as_str()).cloned())
                .collect();
            let base_branch = match depends_on.as_slice() {
                // A single dependency's branch is the only base that has its
                // change; more than one has no branch holding all of them.
                [only] => format!("lgtm/{only}"),
                _ => spec.base_branch.clone(),
            };
            let child = Task {
                id: self.new_id(),
                spec: TaskSpec {
                    repository: spec.repository.clone(),
                    base_branch,
                    prompt: format!("{}\n\n{}", step.title, step.prompt),
                    executor: spec.executor,
                    worker: spec.worker.clone(),
                    issue: None,
                    // The plan task keeps the Linear link; several children
                    // syncing the same issue would move it back and forth.
                    linear: None,
                    kind: TaskKind::Run,
                    parent: Some(id.to_string()),
                    depends_on,
                },
                status: TaskStatus::Queued,
                worker: None,
                created_at: created_at + index as u64,
                result: None,
                error: None,
                pull_request: None,
                ci: None,
            };
            let child_id = child.id.clone();
            ids.insert(step.key.as_str(), child_id.clone());
            self.tasks
                .insert(child_id.clone(), TaskRecord::new(child, Vec::new()));
            changed.push(child_id);
        }

        if let Some(rec) = self.tasks.get_mut(id) {
            rec.task.status = TaskStatus::Approved;
        }
        tracing::info!(task = %id, steps = plan.steps.len(), "plan approved");
        changed.extend(self.schedule());
        self.tasks
            .get(id)
            .map(|rec| (rec.task.clone(), changed))
            .ok_or(CmdError::NotFound)
    }
}
