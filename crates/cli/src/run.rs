//! `lgtm run`: stream a submitted task's events and render them live.

use lgtm_client::Client;
use lgtm_protocol::{Review, Task, TaskEvent, TaskResult};

use crate::render;

pub async fn announce_and_stream(client: &Client, task: Task) -> anyhow::Result<i32> {
    match task.worker.as_deref() {
        Some(worker) => eprintln!("task {} → {worker}", task.id),
        None => eprintln!("task {} queued, waiting for a worker", task.id),
    }
    stream(client, &task.id, 0).await
}

/// Connect to the task's event stream from event index `from` and render
/// events until a terminal one arrives. Shared by `run` (from 0) and `tell`
/// (from the count of events already seen, so a follow-up doesn't replay
/// the whole history).
pub async fn stream(client: &Client, task_id: &str, from: usize) -> anyhow::Result<i32> {
    let mut events = client.events(task_id, from).await?;
    let mut stdout = std::io::stdout();
    while let Some(stored) = events.next().await {
        render::render(&stored.event, &mut stdout)?;
        match stored.event {
            TaskEvent::Completed { result } => return Ok(finish(&result, &mut stdout)?),
            TaskEvent::Failed { error } => {
                eprintln!("error: {error}");
                return Ok(1);
            }
            TaskEvent::Cancelled => {
                eprintln!("cancelled");
                return Ok(130);
            }
            _ => {}
        }
    }
    eprintln!("connection closed");
    Ok(1)
}

/// Prints the result and picks the exit code: 3 when the checks or the
/// reviewer would block it, 0 otherwise.
fn finish(result: &TaskResult, stdout: &mut std::io::Stdout) -> std::io::Result<i32> {
    if let Some(plan) = &result.plan {
        println!();
        render::print_plan(plan, stdout)?;
        return Ok(0);
    }
    println!("\n{} files changed", result.changed_files.len());
    println!("{}", result.diff);
    render::print_validation(&result.validation, stdout)?;
    if let Some(review) = &result.review {
        render::print_review(review, stdout)?;
    }
    render::print_cost(result.cost_usd, stdout)?;
    let blocking = result.review.as_ref().is_some_and(Review::has_blocking);
    Ok(if result.validation_failed() || blocking {
        3
    } else {
        0
    })
}
