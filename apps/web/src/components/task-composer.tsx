import type { ChangeEvent, FormEvent, KeyboardEvent } from "react";
import { useCallback, useEffect, useState } from "react";

import { ActionIcon } from "@/components/action-icon";
import {
  ArrowBackIcon,
  ArrowUpIcon,
  CheckIcon,
  TrashIcon,
} from "@/components/icons";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { useAction } from "@/hooks/use-action";
import { ARMED_CLASS, useArmedConfirm } from "@/hooks/use-armed-confirm";
import {
  approveTask,
  getRunners,
  rejectTask,
  retryTask,
  sendFollowUp,
} from "@/lib/lgtm/server";
import type {
  Executor,
  RunnerStatus,
  Task,
  TaskStatus,
} from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

type Action = "approve" | "reject" | "retry" | "follow-up";

const REVIEWABLE: TaskStatus[] = ["awaiting_review", "conflicted"];
const RETRYABLE: TaskStatus[] = [
  "failed",
  "timed_out",
  "runner_lost",
  "cancelled",
];
const RESPONDABLE: TaskStatus[] = [
  "awaiting_review",
  "conflicted",
  "changes_requested",
];

export const SELECT_CLASS =
  "h-8 w-full min-w-0 rounded-lg border border-input bg-background px-2.5 text-base outline-none transition-colors focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 md:text-sm dark:bg-input/30";

interface RetryOptions {
  executor?: Executor;
  runner?: string;
}

function placeholderFor(task: Task): string {
  if (task.status === "conflicted") {
    return "Tell the agent how to resolve the conflict…";
  }
  if (RESPONDABLE.includes(task.status)) {
    return "Ask for a change…";
  }
  return "The agent is working…";
}

/** The pinned chat input: follow-ups in the box, the review decisions as
 * chips beside the send button, the way Codex keeps them. */
export function TaskComposer({ task }: { task: Task }) {
  const [text, setText] = useState("");
  const { armed, arm, disarm, ref: rejectRef } = useArmedConfirm();
  const { pending, busy, run } = useAction<Action>({ onStart: disarm });

  const reviewable = REVIEWABLE.includes(task.status);
  const respondable = RESPONDABLE.includes(task.status);
  const retryable = RETRYABLE.includes(task.status);
  const working = task.status === "queued" || task.status === "running";

  const submit = useCallback(
    async (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      const body = text.trim();
      if (!body || busy) {
        return;
      }
      const sent = await run(
        "follow-up",
        () => sendFollowUp({ data: { id: task.id, text: body } }),
        "Follow-up sent — the agent resumes with it"
      );
      if (sent) {
        setText("");
      }
    },
    [busy, run, task.id, text]
  );

  const submitOnEnter = useCallback(
    (event: KeyboardEvent<HTMLTextAreaElement>) => {
      if (event.key === "Enter" && !event.shiftKey) {
        event.preventDefault();
        event.currentTarget.form?.requestSubmit();
      }
    },
    []
  );

  const changeText = useCallback(
    (event: ChangeEvent<HTMLTextAreaElement>) =>
      setText(event.currentTarget.value),
    []
  );

  const approve = useCallback(
    () =>
      run(
        "approve",
        () => approveTask({ data: task.id }),
        "Task approved — branch pushed"
      ),
    [run, task.id]
  );

  const reject = useCallback(
    () =>
      armed
        ? run(
            "reject",
            () => rejectTask({ data: task.id }),
            "Task rejected — worktree discarded"
          )
        : arm(),
    [arm, armed, run, task.id]
  );

  // A failed run ends the conversation with what went wrong and the way to
  // go again, in the place a reply would be.
  if (retryable) {
    return <RetryPanel key={task.id} task={task} />;
  }

  if (!(respondable || working)) {
    return null;
  }

  return (
    <div className="sticky bottom-0 bg-linear-to-t from-60% from-background to-transparent pt-6 pb-4">
      <form
        className="flex flex-col gap-1 rounded-2xl border bg-background p-2 shadow-xs transition-colors focus-within:border-ring dark:shadow-none"
        onSubmit={submit}
      >
        <Textarea
          aria-label="Follow-up instructions for the agent"
          className="max-h-40 min-h-9 resize-none border-0 bg-transparent px-1.5 py-1 shadow-none focus-visible:ring-0 disabled:bg-transparent dark:bg-transparent"
          disabled={!respondable || busy}
          onChange={changeText}
          onKeyDown={submitOnEnter}
          placeholder={placeholderFor(task)}
          value={text}
        />
        <div className="flex items-center gap-2">
          {reviewable ? (
            <>
              <Button
                disabled={busy}
                onClick={approve}
                size="sm"
                type="button"
                variant={task.status === "conflicted" ? "outline" : "default"}
              >
                <ActionIcon busy={pending === "approve"} icon={CheckIcon} />
                Approve
              </Button>
              <Button
                className={cn(armed && ARMED_CLASS)}
                disabled={busy}
                onClick={reject}
                ref={rejectRef}
                size="sm"
                type="button"
                variant="outline"
              >
                <ActionIcon busy={pending === "reject"} icon={TrashIcon} />
                {armed ? "Confirm reject" : "Reject"}
              </Button>
            </>
          ) : null}
          <Button
            aria-label="Send follow-up"
            className="ml-auto rounded-full"
            disabled={busy || !respondable || text.trim() === ""}
            size="icon-sm"
            type="submit"
          >
            <ActionIcon busy={pending === "follow-up"} icon={ArrowUpIcon} />
          </Button>
        </div>
      </form>
    </div>
  );
}

function RetryPanel({ task }: { task: Task }) {
  const { pending, busy, run } = useAction<"retry">();
  const [runners, setRunners] = useState<RunnerStatus[]>([]);
  const [runnerName, setRunnerName] = useState("");
  const [executor, setExecutor] = useState<Executor | "">("");

  useEffect(() => {
    let active = true;
    setRunnerName("");
    setExecutor("");
    getRunners()
      .then((next) => {
        if (active) {
          setRunners(next);
        }
      })
      .catch(() => {
        if (active) {
          setRunners([]);
        }
      });
    return () => {
      active = false;
    };
  }, []);

  const sortedRunners = [...runners].sort((a, b) =>
    a.info.name.localeCompare(b.info.name)
  );
  const routedRunnerName = runnerName || task.spec.runner;
  const routedRunner = routedRunnerName
    ? runners.find((candidate) => candidate.info.name === routedRunnerName)
    : undefined;
  const compatibleExecutors = routedRunner
    ? routedRunner.info.executors
    : [...new Set(runners.flatMap((runner) => runner.info.executors))];
  const executorChoices = compatibleExecutors.filter(
    (candidate) => candidate !== task.spec.executor
  );
  const runnerChoices = sortedRunners.filter(
    (runner) =>
      runner.info.name !== task.spec.runner && runner.info.executors.length > 0
  );
  const effectiveExecutor = executor || task.spec.executor;
  const retryCompatible =
    !routedRunner || routedRunner.info.executors.includes(effectiveExecutor);

  const changeRunner = useCallback(
    (event: ChangeEvent<HTMLSelectElement>) => {
      const next = event.currentTarget.value;
      setRunnerName(next);
      let selected: RunnerStatus | undefined;
      if (next) {
        selected = runners.find((runner) => runner.info.name === next);
      } else if (task.spec.runner) {
        selected = runners.find(
          (runner) => runner.info.name === task.spec.runner
        );
      }
      if (selected && !selected.info.executors.includes(effectiveExecutor)) {
        setExecutor(selected.info.executors[0] ?? "");
      }
    },
    [effectiveExecutor, runners, task.spec.runner]
  );

  const changeExecutor = useCallback(
    (event: ChangeEvent<HTMLSelectElement>) => {
      const next = event.currentTarget.value;
      if (next === "" || next === "claude" || next === "codex") {
        setExecutor(next);
      }
    },
    []
  );

  const submit = useCallback(
    async (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      if (busy) {
        return;
      }
      const options: RetryOptions = {
        executor: executor || undefined,
        runner: runnerName || undefined,
      };
      await run(
        "retry",
        () => retryTask({ data: { id: task.id, ...options } }),
        "Task requeued"
      );
    },
    [busy, executor, runnerName, run, task.id]
  );

  const form = (
    <form
      className={cn(
        "grid gap-3 p-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] sm:items-end",
        task.error
          ? "border-destructive/15 border-t"
          : "rounded-xl border bg-background"
      )}
      onSubmit={submit}
    >
      <label
        className="flex min-w-0 flex-col gap-1.5"
        htmlFor={`retry-runner-${task.id}`}
      >
        <span className="font-medium text-muted-foreground text-xs">
          Machine
        </span>
        <select
          className={SELECT_CLASS}
          disabled={busy || runnerChoices.length === 0}
          id={`retry-runner-${task.id}`}
          onChange={changeRunner}
          value={runnerName}
        >
          <option value="">{sameRunnerLabel(task)}</option>
          {runnerChoices.map((runner) => (
            <option key={runner.info.name} value={runner.info.name}>
              {runner.info.name} · {runner.info.os}
            </option>
          ))}
        </select>
      </label>
      <label
        className="flex min-w-0 flex-col gap-1.5"
        htmlFor={`retry-executor-${task.id}`}
      >
        <span className="font-medium text-muted-foreground text-xs">
          Executor
        </span>
        <select
          className={SELECT_CLASS}
          disabled={busy || executorChoices.length === 0}
          id={`retry-executor-${task.id}`}
          onChange={changeExecutor}
          value={executor}
        >
          <option
            disabled={!compatibleExecutors.includes(task.spec.executor)}
            value=""
          >
            Same executor · {executorLabel(task.spec.executor)}
          </option>
          {executorChoices.map((choice) => (
            <option key={choice} value={choice}>
              {executorLabel(choice)}
            </option>
          ))}
        </select>
      </label>
      <Button
        className={cn(
          "w-full sm:w-auto",
          task.error &&
            "border-destructive/20 text-destructive hover:bg-destructive/10 hover:text-destructive"
        )}
        disabled={busy || !retryCompatible}
        size="sm"
        type="submit"
        variant="outline"
      >
        <ActionIcon busy={pending === "retry"} icon={ArrowBackIcon} />
        {pending === "retry" ? "Retrying…" : "Retry this task"}
      </Button>
    </form>
  );

  if (!task.error) {
    return <div className="pb-4">{form}</div>;
  }

  return (
    <div className="overflow-hidden rounded-xl border border-destructive/30 bg-destructive/5">
      <pre className="whitespace-pre-wrap p-4 font-mono text-destructive/90 text-xs leading-relaxed [overflow-wrap:anywhere]">
        {task.error}
      </pre>
      {form}
    </div>
  );
}

function sameRunnerLabel(task: Task): string {
  if (task.spec.runner) {
    return `Same machine · ${task.spec.runner}`;
  }
  return task.runner
    ? `Automatic · last used ${task.runner}`
    : "Automatic routing";
}

function executorLabel(executor: Executor): string {
  return executor === "claude" ? "Claude" : "Codex";
}
