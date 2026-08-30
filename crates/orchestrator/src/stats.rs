//! Throughput, duration, and cost across tasks created since a cutoff. Pure:
//! the caller hands over the store's tasks and events.

use lgtm_protocol::{
    Execution, ExecutionStatus, Executor, ExecutorStats, Stats, StoredEvent, Task, TaskEvent,
    TaskStatus,
};

/// Counts `tasks` by status, folding `ChangesRequested` into running and
/// `TimedOut`/`RunnerLost` into failed, same as `backlog::summary` without
/// its blocked split (there is no queue to be blocked in here).
fn count_status(stats: &mut Stats, status: TaskStatus) {
    let counter = match status {
        TaskStatus::Queued => &mut stats.queued,
        TaskStatus::Running | TaskStatus::ChangesRequested => &mut stats.running,
        TaskStatus::AwaitingReview | TaskStatus::Conflicted => &mut stats.awaiting_review,
        TaskStatus::Approved => &mut stats.approved,
        TaskStatus::Merged => &mut stats.merged,
        TaskStatus::Failed | TaskStatus::TimedOut | TaskStatus::RunnerLost => &mut stats.failed,
        TaskStatus::Cancelled => &mut stats.cancelled,
        TaskStatus::Rejected => &mut stats.rejected,
    };
    *counter += 1;
}

fn first_started_at(events: &[StoredEvent]) -> Option<u64> {
    events
        .iter()
        .find(|e| matches!(e.event, TaskEvent::Started))
        .map(|e| e.at)
}

/// `attempts`/`completed`/`failed` for the executor an attempt ran under,
/// creating its row on first sight.
fn record_executor(
    by_executor: &mut Vec<ExecutorStats>,
    executor: Executor,
    status: ExecutionStatus,
) {
    let entry = match by_executor.iter_mut().find(|e| e.executor == executor) {
        Some(entry) => entry,
        None => {
            by_executor.push(ExecutorStats {
                executor,
                ..Default::default()
            });
            by_executor.last_mut().expect("just pushed")
        }
    };
    entry.attempts += 1;
    match status {
        ExecutionStatus::Completed => entry.completed += 1,
        ExecutionStatus::Failed => entry.failed += 1,
        ExecutionStatus::Running | ExecutionStatus::Cancelled => {}
    }
}

fn record_executions(
    executions: &[Execution],
    exec_ms: &mut Vec<u64>,
    by_executor: &mut Vec<ExecutorStats>,
) {
    for execution in executions {
        if let Some(finished) = execution.finished_at {
            exec_ms.push(finished.saturating_sub(execution.started_at));
        }
        record_executor(by_executor, execution.executor, execution.status);
    }
}

/// Middle value of a sorted copy; 0 for an empty set (there is nothing to
/// report yet, not a division by zero to hide).
fn median(values: &mut [u64]) -> u64 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2
    } else {
        values[mid]
    }
}

pub fn compute(records: &[(&Task, &[StoredEvent])], since: u64) -> Stats {
    let mut stats = Stats {
        since,
        ..Stats::default()
    };
    let mut exec_ms = Vec::new();
    let mut queue_ms = Vec::new();

    let filtered = records
        .iter()
        .copied()
        .filter(|(task, _)| task.created_at >= since);
    for (task, events) in filtered {
        stats.tasks += 1;
        count_status(&mut stats, task.status);
        if task.executions.len() > 1 {
            stats.retried_tasks += 1;
        }
        stats.cost_usd += task.result.as_ref().map_or(0.0, |r| r.cost_usd);
        if let Some(started) = first_started_at(events) {
            queue_ms.push(started.saturating_sub(task.created_at));
        }
        record_executions(&task.executions, &mut exec_ms, &mut stats.by_executor);
    }

    stats.median_execution_ms = median(&mut exec_ms);
    stats.median_queue_ms = median(&mut queue_ms);
    stats.by_executor.sort_by_key(|e| e.executor.binary());
    stats
}

#[cfg(test)]
#[path = "stats_tests.rs"]
mod tests;
