//! Task lifecycle: worktree, executor process, commit, validation, push, discard.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Result};
use lgtm_protocol::{knowledge_block, Memory, Task, TaskEvent, TaskId, TaskKind};
use tokio::sync::oneshot;

use crate::automation::{execute, recorded_session, restore_notes, with_notes, Run};
use crate::connection::Ctx;
use crate::git::{
    add_worktree, auth_args, branch_name, fetch, git, mirror_path, rebase_onto, remove_worktree,
    session_path, task_path, worktree_path,
};
use crate::plan::{planning_prompt, revision_prompt};

pub async fn run_task(
    task: Task,
    memories: Vec<Memory>,
    ctx: Arc<Ctx>,
    cancel: oneshot::Receiver<()>,
) {
    let result = run(&task, &memories, &ctx, cancel).await;
    finished(&task.id, &ctx, result);
}

/// A follow-up in the worktree of a task that already ran, resuming the agent
/// session when the first run recorded one.
pub async fn follow_up(
    task_id: TaskId,
    text: String,
    memories: Vec<Memory>,
    ctx: Arc<Ctx>,
    cancel: oneshot::Receiver<()>,
) {
    let result = resume(&task_id, &text, &memories, &ctx, cancel).await;
    finished(&task_id, &ctx, result);
}

fn finished(task_id: &str, ctx: &Arc<Ctx>, result: Result<()>) {
    if let Err(err) = result {
        ctx.fail(task_id, err);
    }
    ctx.running
        .lock()
        .expect("running map poisoned")
        .remove(task_id);
    ctx.task_finished();
}

async fn run(
    task: &Task,
    memories: &[Memory],
    ctx: &Arc<Ctx>,
    cancel: oneshot::Receiver<()>,
) -> Result<()> {
    let branch = branch_name(&task.id);
    let worktree = worktree_path(&ctx.data_dir, &task.id);
    tokio::fs::create_dir_all(ctx.data_dir.join("worktrees")).await?;
    tokio::fs::write(
        task_path(&ctx.data_dir, &task.id),
        serde_json::to_vec(task)?,
    )
    .await?;

    let mirror = prepare_repo(task, ctx).await?;
    add_worktree(&mirror, &worktree, &branch, &task.spec.base_branch).await?;
    restore_notes(&worktree, &task.scratchpad).await?;

    let prompt = match task.spec.kind {
        TaskKind::Plan => planning_prompt(&task.spec.prompt),
        TaskKind::Run => task.spec.prompt.clone(),
    };
    // A follow-up resumes this session, which already saw the block.
    let prompt = format!(
        "{}{}",
        knowledge_block(memories),
        with_notes(&prompt, task.spec.kind)
    );
    execute(Run::new(task, &worktree, ctx, cancel), &prompt, None).await
}

/// The task as the orchestrator sent it, or `None` when this worker has no
/// file for it any more.
async fn stored_task(ctx: &Arc<Ctx>, task_id: &str) -> Option<Task> {
    let bytes = tokio::fs::read(task_path(&ctx.data_dir, task_id))
        .await
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

async fn resume(
    task_id: &TaskId,
    text: &str,
    memories: &[Memory],
    ctx: &Arc<Ctx>,
    cancel: oneshot::Receiver<()>,
) -> Result<()> {
    let Some(task) = stored_task(ctx, task_id).await else {
        bail!("task unknown to this worker (restarted?)");
    };
    let worktree = worktree_path(&ctx.data_dir, task_id);
    if !worktree.exists() {
        bail!("worktree missing");
    }
    if task.spec.kind == TaskKind::Plan {
        // A revision is a fresh process, not a resumed session, so it is told
        // again.
        let prompt = format!(
            "{}{}",
            knowledge_block(memories),
            revision_prompt(&task.spec.prompt, text)
        );
        return execute(Run::new(&task, &worktree, ctx, cancel), &prompt, None).await;
    }
    let session = recorded_session(ctx, task_id).await;
    if session.is_none() {
        tracing::warn!("no session id for {task_id}, running the follow-up fresh");
    }
    execute(Run::new(&task, &worktree, ctx, cancel), text, session).await
}

/// Clones the bare mirror or refreshes it, and records it for a later discard.
async fn prepare_repo(task: &Task, ctx: &Arc<Ctx>) -> Result<PathBuf> {
    tokio::fs::create_dir_all(ctx.data_dir.join("repos")).await?;
    tokio::fs::create_dir_all(ctx.data_dir.join("worktrees")).await?;
    let mirror = mirror_path(&ctx.data_dir, &task.spec.repository);
    ctx.mirrors
        .lock()
        .expect("mirrors poisoned")
        .insert(task.id.clone(), mirror.clone());
    if !mirror.exists() {
        let mirror_s = mirror.display().to_string();
        git(&["clone", "--bare", &task.spec.repository, &mirror_s], None).await?;
    }
    fetch(&mirror).await?;
    Ok(mirror)
}

pub async fn push_task(task_id: TaskId, token: Option<String>, ctx: Arc<Ctx>) {
    let worktree = worktree_path(&ctx.data_dir, &task_id);
    // ponytail: a clean rebase is not re-validated, so the branch is only
    // proven to apply, not to still pass; running the repository's checks
    // again here is the upgrade once a rebase starts breaking tasks.
    match rebase_for_push(&task_id, &worktree, token.as_deref(), &ctx).await {
        Ok(true) => push(task_id, &worktree, token.as_deref(), &ctx).await,
        Ok(false) => {}
        Err(err) => ctx.fail(&task_id, err),
    }
}

/// Rebases the task's branch onto its base before the push. `false` means the
/// branch conflicts, which is work for the agent instead of a push.
async fn rebase_for_push(
    task_id: &str,
    worktree: &Path,
    token: Option<&str>,
    ctx: &Arc<Ctx>,
) -> Result<bool> {
    let Some(task) = stored_task(ctx, task_id).await else {
        tracing::warn!("no task file for {task_id} (restarted?), pushing without a rebase");
        return Ok(true);
    };
    let base = task.spec.base_branch;
    match rebase_onto(worktree, &base, token).await? {
        Ok(()) => Ok(true),
        Err(files) => {
            ctx.emit(task_id, TaskEvent::Conflicted { base, files });
            Ok(false)
        }
    }
}

async fn push(task_id: TaskId, worktree: &Path, token: Option<&str>, ctx: &Arc<Ctx>) {
    let worktree = worktree.display().to_string();
    let branch = branch_name(&task_id);
    let auth = auth_args(token);
    let mut args: Vec<&str> = auth.iter().map(String::as_str).collect();
    args.extend_from_slice(&["-C", &worktree, "push", "-u", "origin", &branch]);
    match git(&args, None).await {
        Ok(_) => {
            let sha = match git(&["-C", &worktree, "rev-parse", "HEAD"], None).await {
                Ok(sha) => sha.trim().to_string(),
                Err(err) => {
                    tracing::warn!("failed to resolve pushed HEAD sha: {err:#}");
                    String::new()
                }
            };
            ctx.emit(&task_id, TaskEvent::Pushed { branch, sha });
        }
        Err(err) => ctx.fail(&task_id, err),
    }
}

pub async fn discard_task(task_id: TaskId, ctx: Arc<Ctx>) {
    let worktree = worktree_path(&ctx.data_dir, &task_id);
    let _ = tokio::fs::remove_file(session_path(&ctx.data_dir, &task_id)).await;
    let _ = tokio::fs::remove_file(task_path(&ctx.data_dir, &task_id)).await;
    let mirror = ctx
        .mirrors
        .lock()
        .expect("mirrors poisoned")
        .remove(&task_id);
    let result = match mirror {
        Some(mirror) => remove_worktree(&mirror, &worktree, &branch_name(&task_id)).await,
        // Restarted since the task ran: the mirror is unknown, so drop the files.
        None => tokio::fs::remove_dir_all(&worktree)
            .await
            .map_err(|err| anyhow::anyhow!("remove {}: {err}", worktree.display())),
    };
    match result {
        Ok(()) => ctx.emit(&task_id, TaskEvent::Discarded),
        Err(err) => ctx.fail(&task_id, err),
    }
}
