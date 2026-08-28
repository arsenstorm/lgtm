//! Task lifecycle: worktree, executor process, commit, validation, push, discard.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use lgtm_protocol::{Executor, OutputStream, Task, TaskEvent, TaskId, TaskKind, TaskResult};
use tokio::process::Command;
use tokio::sync::oneshot;

use crate::connection::Ctx;
use crate::git::{
    add_worktree, branch_name, commit, git, mirror_path, session_path, task_path, worktree_path,
};
use crate::plan::{extract_plan, planning_prompt, revision_prompt};
use crate::proc::{final_text, pump, tail_buffer, tail_lines, text_buffer};
use crate::validate::{load_validation, run_validation, tail};

pub async fn run_task(task: Task, ctx: Arc<Ctx>, cancel: oneshot::Receiver<()>) {
    if let Err(err) = run(&task, &ctx, cancel).await {
        ctx.emit(
            &task.id,
            TaskEvent::Failed {
                error: format!("{err:#}"),
            },
        );
    }
    finished(&task.id, &ctx);
}

/// A follow-up in the worktree of a task that already ran, resuming the agent
/// session when the first run recorded one.
pub async fn follow_up(
    task_id: TaskId,
    text: String,
    ctx: Arc<Ctx>,
    cancel: oneshot::Receiver<()>,
) {
    if let Err(err) = resume(&task_id, &text, &ctx, cancel).await {
        ctx.emit(
            &task_id,
            TaskEvent::Failed {
                error: format!("{err:#}"),
            },
        );
    }
    finished(&task_id, &ctx);
}

fn finished(task_id: &str, ctx: &Arc<Ctx>) {
    ctx.running
        .lock()
        .expect("running map poisoned")
        .remove(task_id);
}

async fn run(task: &Task, ctx: &Arc<Ctx>, cancel: oneshot::Receiver<()>) -> Result<()> {
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

    let prompt = match task.spec.kind {
        TaskKind::Plan => planning_prompt(&task.spec.prompt),
        TaskKind::Run => task.spec.prompt.clone(),
    };
    execute(task, &prompt, None, &worktree, ctx, cancel).await
}

async fn resume(
    task_id: &TaskId,
    text: &str,
    ctx: &Arc<Ctx>,
    cancel: oneshot::Receiver<()>,
) -> Result<()> {
    let stored = tokio::fs::read(task_path(&ctx.data_dir, task_id))
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Task>(&bytes).ok());
    let Some(task) = stored else {
        bail!("task unknown to this worker (restarted?)");
    };
    let worktree = worktree_path(&ctx.data_dir, task_id);
    if !worktree.exists() {
        bail!("worktree missing");
    }
    if task.spec.kind == TaskKind::Plan {
        let prompt = revision_prompt(&task.spec.prompt, text);
        return execute(&task, &prompt, None, &worktree, ctx, cancel).await;
    }
    let session = tokio::fs::read_to_string(session_path(&ctx.data_dir, task_id))
        .await
        .ok()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty());
    if session.is_none() {
        tracing::warn!("no session id for {task_id}, running the follow-up fresh");
    }
    execute(&task, text, session, &worktree, ctx, cancel).await
}

/// One agent run: spawn, pump, wait, commit, validate, report.
async fn execute(
    task: &Task,
    prompt: &str,
    resume: Option<String>,
    worktree: &Path,
    ctx: &Arc<Ctx>,
    cancel: oneshot::Receiver<()>,
) -> Result<()> {
    let branch = branch_name(&task.id);
    let planning = task.spec.kind == TaskKind::Plan;
    let binary = task.spec.executor.binary();
    let path = which::which(binary).with_context(|| format!("{binary} not found on PATH"))?;
    let mut cmd = Command::new(&path);
    match task.spec.executor {
        Executor::Claude => {
            cmd.args(["-p", prompt]);
            if let Some(session) = resume.as_ref().filter(|_| !planning) {
                cmd.args(["--resume", session]);
            }
            cmd.args([
                "--output-format",
                "stream-json",
                "--verbose",
                "--permission-mode",
                if planning { "default" } else { "acceptEdits" },
            ]);
        }
        Executor::Codex => {
            cmd.args(["exec", prompt]);
        }
    };
    let mut child = cmd
        .current_dir(worktree)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("spawn {}", path.display()))?;
    ctx.emit(&task.id, TaskEvent::Started);

    let stderr_tail = tail_buffer();
    let answer = planning.then(text_buffer);
    let stdout = child.stdout.take().context("no stdout")?;
    let stderr = child.stderr.take().context("no stderr")?;
    let pump_out = tokio::spawn(pump(
        stdout,
        OutputStream::Stdout,
        ctx.clone(),
        task.id.clone(),
        None,
        Some(session_path(&ctx.data_dir, &task.id)),
        answer.clone(),
    ));
    let pump_err = tokio::spawn(pump(
        stderr,
        OutputStream::Stderr,
        ctx.clone(),
        task.id.clone(),
        Some(stderr_tail.clone()),
        None,
        None,
    ));

    let waited = tokio::select! {
        status = child.wait() => Some(status),
        _ = cancel => None,
    };
    let status = match waited {
        Some(status) => status?,
        None => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            ctx.emit(&task.id, TaskEvent::Cancelled);
            return Ok(());
        }
    };
    let _ = tokio::join!(pump_out, pump_err);

    if !status.success() {
        ctx.emit(
            &task.id,
            TaskEvent::Failed {
                error: format!(
                    "{binary} exited with {status}\n{}",
                    tail(&tail_lines(&stderr_tail))
                ),
            },
        );
        return Ok(());
    }

    if let Some(answer) = answer {
        ctx.emit(&task.id, planned(&branch, &final_text(&answer)));
        return Ok(());
    }

    let mut result = commit(prompt, &task.spec.base_branch, &branch, worktree).await?;
    result.validation = run_validation(worktree, &load_validation(worktree)).await;
    ctx.emit(&task.id, TaskEvent::Completed { result });
    Ok(())
}

/// A plan run leaves no diff, so its result carries only the parsed plan.
fn planned(branch: &str, text: &str) -> TaskEvent {
    match extract_plan(text) {
        Ok(plan) => TaskEvent::Completed {
            result: TaskResult {
                branch: branch.to_string(),
                diff: String::new(),
                changed_files: Vec::new(),
                validation: Vec::new(),
                plan: Some(plan),
            },
        },
        Err(err) => TaskEvent::Failed {
            error: format!("{err:#}"),
        },
    }
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
    let mirror_s = mirror.display().to_string();
    if mirror.exists() {
        git(
            &[
                "-C",
                &mirror_s,
                "fetch",
                "--prune",
                "origin",
                "+refs/heads/*:refs/heads/*",
            ],
            None,
        )
        .await?;
    } else {
        git(&["clone", "--bare", &task.spec.repository, &mirror_s], None).await?;
    }
    Ok(mirror)
}

pub async fn push_task(task_id: TaskId, ctx: Arc<Ctx>) {
    let worktree = worktree_path(&ctx.data_dir, &task_id).display().to_string();
    let branch = branch_name(&task_id);
    match git(&["-C", &worktree, "push", "-u", "origin", &branch], None).await {
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
        Err(err) => ctx.emit(
            &task_id,
            TaskEvent::Failed {
                error: format!("{err:#}"),
            },
        ),
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
    let Some(mirror) = mirror else {
        // Restarted since the task ran: the mirror is unknown, so drop the files.
        match tokio::fs::remove_dir_all(&worktree).await {
            Ok(()) => ctx.emit(&task_id, TaskEvent::Discarded),
            Err(err) => ctx.emit(
                &task_id,
                TaskEvent::Failed {
                    error: format!("remove {}: {err}", worktree.display()),
                },
            ),
        }
        return;
    };
    let mirror_s = mirror.display().to_string();
    let worktree_s = worktree.display().to_string();
    let branch = branch_name(&task_id);
    let result = async {
        git(
            &[
                "-C",
                &mirror_s,
                "worktree",
                "remove",
                "--force",
                &worktree_s,
            ],
            None,
        )
        .await?;
        git(&["-C", &mirror_s, "branch", "-D", &branch], None).await?;
        anyhow::Ok(())
    }
    .await;
    match result {
        Ok(()) => ctx.emit(&task_id, TaskEvent::Discarded),
        Err(err) => ctx.emit(
            &task_id,
            TaskEvent::Failed {
                error: format!("{err:#}"),
            },
        ),
    }
}
