//! `lgtm backlog`: import a batch of issues as tasks, and inspect batches.

use crate::http::Client;
use crate::{first_line_truncated, print_task_table};
use lgtm_protocol::{Batch, BatchSource, BatchSummary, Executor, Task};
use serde::{Deserialize, Serialize};

/// Body of `POST /api/batches`.
#[derive(Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct CreateBatch {
    source: BatchSource,
    repository: Option<String>,
    base_branch: String,
    executor: Executor,
    worker: Option<String>,
    plan: bool,
    approve_plans: bool,
    max: u32,
    dry_run: bool,
}

#[derive(Deserialize)]
struct BatchIssue {
    key: String,
    title: String,
    #[allow(dead_code)]
    url: String,
}

/// Body of `POST /api/batches`'s response, 200 (dry run, `batch: None`) or
/// 201 (`batch: Some`).
#[derive(Deserialize)]
struct CreateBatchResponse {
    batch: Option<Batch>,
    issues: Vec<BatchIssue>,
}

/// Body of `GET /api/batches/:id`.
#[derive(Deserialize)]
struct BatchDetail {
    batch: Batch,
    summary: BatchSummary,
    tasks: Vec<Task>,
}

/// `github o/r label:L` or `linear TEAM/STATE`, for the `list` table's
/// SOURCE column and `status`'s header line.
pub(crate) fn source_label(source: &BatchSource) -> String {
    match source {
        BatchSource::GithubLabel { owner, repo, label } => {
            format!("github {owner}/{repo} label:{label}")
        }
        BatchSource::Linear { team, state } => format!("linear {team}/{state}"),
    }
}

/// Task counts by state, for `status`'s summary line.
pub(crate) fn summary_line(summary: &BatchSummary) -> String {
    format!(
        "queued {} · blocked {} · running {} · review {} · approved {} · merged {} · failed {} · cancelled {} · rejected {}",
        summary.queued,
        summary.blocked,
        summary.running,
        summary.awaiting_review,
        summary.approved,
        summary.merged,
        summary.failed,
        summary.cancelled,
        summary.rejected,
    )
}

/// Shared by `backlog github` and `backlog linear`: posts the batch, then
/// prints `dry run: n issues` or `batch <id>: n tasks from n issues`
/// followed by one line per matched issue.
#[allow(clippy::too_many_arguments)]
pub async fn create(
    client: &Client,
    source: BatchSource,
    repository: Option<String>,
    base_branch: String,
    executor: Executor,
    worker: Option<String>,
    plan: bool,
    approve_plans: bool,
    max: u32,
    dry_run: bool,
) -> anyhow::Result<i32> {
    let body = CreateBatch {
        source,
        repository,
        base_branch,
        executor,
        worker,
        plan,
        approve_plans,
        max,
        dry_run,
    };
    let resp: CreateBatchResponse = client.post("/api/batches", Some(&body)).await?;
    match &resp.batch {
        None => println!("dry run: {} issues", resp.issues.len()),
        Some(batch) => println!(
            "batch {}: {} tasks from {} issues",
            batch.id,
            batch.task_ids.len(),
            resp.issues.len()
        ),
    }
    for issue in &resp.issues {
        println!(
            "  {}  {}",
            issue.key,
            first_line_truncated(&issue.title, 70)
        );
    }
    Ok(0)
}

pub async fn list(client: &Client) -> anyhow::Result<i32> {
    let batches: Vec<Batch> = client.get("/api/batches").await?;
    println!("{:<10}{:<16}{:<30}TASKS", "ID", "CREATED", "SOURCE");
    for b in batches {
        println!(
            "{:<10}{:<16}{:<30}{}",
            b.id,
            b.created_at,
            source_label(&b.source),
            b.task_ids.len()
        );
    }
    Ok(0)
}

pub async fn status(client: &Client, id: &str) -> anyhow::Result<i32> {
    let detail: BatchDetail = client.get(&format!("/api/batches/{id}")).await?;
    println!(
        "batch {} · {}",
        detail.batch.id,
        source_label(&detail.batch.source)
    );
    println!("{}", summary_line(&detail.summary));
    print_task_table(detail.tasks);
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_label_formats_github() {
        let source = BatchSource::GithubLabel {
            owner: "o".into(),
            repo: "r".into(),
            label: "P1".into(),
        };
        assert_eq!(source_label(&source), "github o/r label:P1");
    }

    #[test]
    fn source_label_formats_linear() {
        let source = BatchSource::Linear {
            team: "ENG".into(),
            state: "Todo".into(),
        };
        assert_eq!(source_label(&source), "linear ENG/Todo");
    }

    #[test]
    fn summary_line_lists_every_state_in_order() {
        let summary = BatchSummary {
            queued: 1,
            blocked: 2,
            running: 3,
            awaiting_review: 4,
            approved: 5,
            merged: 6,
            failed: 7,
            cancelled: 8,
            rejected: 9,
        };
        assert_eq!(
            summary_line(&summary),
            "queued 1 · blocked 2 · running 3 · review 4 · approved 5 · merged 6 · failed 7 · cancelled 8 · rejected 9"
        );
    }
}
