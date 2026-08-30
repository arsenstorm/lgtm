//! Throughput, duration, and cost across tasks created since a cutoff. Pure:
//! the caller hands over the store's tasks and events.

use lgtm_protocol::{
    Execution, ExecutionStatus, Executor, ExecutorStats, RunnerStats, Stats, StoredEvent, Task,
    TaskEvent, TaskStatus,
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
        .find(|e| matches!(e.event, TaskEvent::Started { .. }))
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

/// `by_runner`'s row for `execution.runner` gains an attempt (and a failure,
/// and its duration in `runner_ms`), creating both on first sight so the two
/// vectors stay index-aligned.
fn record_executions(
    executions: &[Execution],
    exec_ms: &mut Vec<u64>,
    by_executor: &mut Vec<ExecutorStats>,
    by_runner: &mut Vec<RunnerStats>,
    runner_ms: &mut Vec<Vec<u64>>,
) {
    for execution in executions {
        let idx = match by_runner.iter().position(|r| r.runner == execution.runner) {
            Some(idx) => idx,
            None => {
                by_runner.push(RunnerStats {
                    runner: execution.runner.clone(),
                    ..Default::default()
                });
                runner_ms.push(Vec::new());
                by_runner.len() - 1
            }
        };
        by_runner[idx].attempts += 1;
        if execution.status == ExecutionStatus::Failed {
            by_runner[idx].failed += 1;
        }
        if let Some(finished) = execution.finished_at {
            let duration = finished.saturating_sub(execution.started_at);
            exec_ms.push(duration);
            runner_ms[idx].push(duration);
        }
        record_executor(by_executor, execution.executor, execution.status);
    }
}

/// Middle value of a sorted copy; 0 for an empty set (there is nothing to
/// report yet, not a division by zero to hide). `pub(crate)` so `state.rs`'s
/// `median_for` can reuse it rather than sort medians its own way.
pub(crate) fn median(values: &mut [u64]) -> u64 {
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
    let mut runner_ms = Vec::new();

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
        record_executions(
            &task.executions,
            &mut exec_ms,
            &mut stats.by_executor,
            &mut stats.by_runner,
            &mut runner_ms,
        );
    }

    stats.median_execution_ms = median(&mut exec_ms);
    stats.median_queue_ms = median(&mut queue_ms);
    stats.by_executor.sort_by_key(|e| e.executor.binary());
    for (entry, ms) in stats.by_runner.iter_mut().zip(runner_ms.iter_mut()) {
        entry.median_ms = median(ms);
    }
    stats.by_runner.sort_by(|a, b| a.runner.cmp(&b.runner));
    record_budget(records, &mut stats);
    stats
}

/// The daily budget and today's spend against it, over every repository in
/// view. Independent of `since`: "today" is always the real last 24h, not
/// whatever window the rest of the report covers. Two repositories in view
/// share one number, so a multi-repo report shows the higher of their
/// declared budgets rather than a figure per repository.
fn record_budget(records: &[(&Task, &[StoredEvent])], stats: &mut Stats) {
    let day_ago = crate::state::now_ms().saturating_sub(24 * 60 * 60 * 1000);
    for (task, _) in records.iter().copied() {
        let Some(result) = task.result.as_ref() else {
            continue;
        };
        if task.created_at >= day_ago {
            stats.spent_today += result.cost_usd;
        }
        if let Some(budget) = result.policy.as_ref().and_then(|p| p.budget_daily_usd) {
            stats.budget_daily_usd = Some(stats.budget_daily_usd.map_or(budget, |b| b.max(budget)));
        }
    }
}

#[cfg(test)]
#[path = "stats_tests.rs"]
mod tests;
