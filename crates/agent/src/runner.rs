//! Task lifecycle: worktree, executor process, commit, push, discard.

use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use lgtm_protocol::{Executor, OutputStream, Task, TaskEvent, TaskId, TaskResult};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::oneshot;

use crate::connection::Ctx;
use crate::git::{
    branch_name, commit_message, git, has_staged_changes, mirror_path, worktree_path, IDENTITY,
};

const STDERR_TAIL: usize = 50;

type Tail = Arc<Mutex<VecDeque<String>>>;

pub async fn run_task(task: Task, ctx: Arc<Ctx>, cancel: oneshot::Receiver<()>) {
    if let Err(err) = run(&task, &ctx, cancel).await {
        ctx.emit(
            &task.id,
            TaskEvent::Failed {
                error: format!("{err:#}"),
            },
        );
    }
    ctx.running
        .lock()
        .expect("running map poisoned")
        .remove(&task.id);
}

async fn run(task: &Task, ctx: &Arc<Ctx>, cancel: oneshot::Receiver<()>) -> Result<()> {
    let branch = branch_name(&task.id);
    let worktree = worktree_path(&ctx.data_dir, &task.id);
    let mirror = prepare_repo(task, ctx).await?;
    let mirror_s = mirror.display().to_string();
    let worktree_s = worktree.display().to_string();

    if worktree.exists() {
        let _ = git(
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
        .await;
        let _ = git(&["-C", &mirror_s, "branch", "-D", &branch], None).await;
        // `worktree remove` fails on a directory git never registered.
        let _ = tokio::fs::remove_dir_all(&worktree).await;
    }
    git(
        &[
            "-C",
            &mirror_s,
            "worktree",
            "add",
            "-b",
            &branch,
            &worktree_s,
            &task.spec.base_branch,
        ],
        None,
    )
    .await?;

    let binary = task.spec.executor.binary();
    let path = which::which(binary).with_context(|| format!("{binary} not found on PATH"))?;
    let mut cmd = Command::new(&path);
    match task.spec.executor {
        Executor::Claude => cmd.args([
            "-p",
            &task.spec.prompt,
            "--output-format",
            "stream-json",
            "--verbose",
            "--permission-mode",
            "acceptEdits",
        ]),
        Executor::Codex => cmd.args(["exec", &task.spec.prompt]),
    };
    let mut child = cmd
        .current_dir(&worktree)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("spawn {}", path.display()))?;
    ctx.emit(&task.id, TaskEvent::Started);

    let tail: Tail = Arc::new(Mutex::new(VecDeque::new()));
    let stdout = child.stdout.take().context("no stdout")?;
    let stderr = child.stderr.take().context("no stderr")?;
    let pump_out = tokio::spawn(pump(
        stdout,
        OutputStream::Stdout,
        ctx.clone(),
        task.id.clone(),
        None,
    ));
    let pump_err = tokio::spawn(pump(
        stderr,
        OutputStream::Stderr,
        ctx.clone(),
        task.id.clone(),
        Some(tail.clone()),
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
        let lines: Vec<String> = tail
            .lock()
            .expect("tail poisoned")
            .iter()
            .cloned()
            .collect();
        ctx.emit(
            &task.id,
            TaskEvent::Failed {
                error: format!("{binary} exited with {status}\n{}", lines.join("\n")),
            },
        );
        return Ok(());
    }

    let result = commit(task, &branch, &worktree).await?;
    ctx.emit(&task.id, TaskEvent::Completed { result });
    Ok(())
}

/// Clones the bare mirror or refreshes it, and records it for a later discard.
async fn prepare_repo(task: &Task, ctx: &Arc<Ctx>) -> Result<std::path::PathBuf> {
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

async fn commit(task: &Task, branch: &str, worktree: &std::path::Path) -> Result<TaskResult> {
    let cwd = Some(worktree);
    git(&["add", "-A"], cwd).await?;
    if has_staged_changes(worktree).await? {
        let message = commit_message(&task.spec.prompt);
        let mut args = IDENTITY.to_vec();
        args.extend_from_slice(&["commit", "-q", "-m", &message]);
        git(&args, cwd).await?;
    }
    let range = format!("{}...{}", task.spec.base_branch, branch);
    let diff = git(&["diff", &range], cwd).await?;
    let names = git(&["diff", "--name-only", &range], cwd).await?;
    Ok(TaskResult {
        branch: branch.to_string(),
        diff,
        changed_files: names.lines().map(str::to_string).collect(),
    })
}

async fn pump<R: AsyncRead + Unpin>(
    reader: R,
    stream: OutputStream,
    ctx: Arc<Ctx>,
    task_id: TaskId,
    tail: Option<Tail>,
) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(tail) = &tail {
            let mut tail = tail.lock().expect("tail poisoned");
            tail.push_back(line.clone());
            if tail.len() > STDERR_TAIL {
                tail.pop_front();
            }
        }
        ctx.emit(&task_id, TaskEvent::Output { stream, line });
    }
}

pub async fn push_task(task_id: TaskId, ctx: Arc<Ctx>) {
    let worktree = worktree_path(&ctx.data_dir, &task_id).display().to_string();
    let branch = branch_name(&task_id);
    match git(&["-C", &worktree, "push", "-u", "origin", &branch], None).await {
        Ok(_) => ctx.emit(&task_id, TaskEvent::Pushed { branch }),
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
