import {
  ArrowCounterClockwise,
  ArrowUp,
  Check,
  Trash,
} from "@phosphor-icons/react";
import type { FormEvent, KeyboardEvent } from "react";
import { useState } from "react";

import { ActionIcon } from "@/components/action-icon";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { useAction } from "@/hooks/use-action";
import { ARMED_CLASS, useArmedConfirm } from "@/hooks/use-armed-confirm";
import {
  approveTask,
  rejectTask,
  retryTask,
  sendFollowUp,
} from "@/lib/lgtm/server";
import type { Task, TaskStatus } from "@/lib/lgtm/types";
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

function placeholderFor(task: Task): string {
  if (task.status === "conflicted") {
    return "Tell the agent how to resolve the conflict…";
  }
  if (RESPONDABLE.includes(task.status)) {
    return "Ask for a change…";
  }
  if (task.status === "queued" || task.status === "running") {
    return "The agent is working…";
  }
  if (RETRYABLE.includes(task.status)) {
    return "Retry to run this task again";
  }
  return "This task is closed";
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

  async function submit(event: FormEvent<HTMLFormElement>) {
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
  }

  function submitOnEnter(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      event.currentTarget.form?.requestSubmit();
    }
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
          onChange={(event) => setText(event.target.value)}
          onKeyDown={submitOnEnter}
          placeholder={placeholderFor(task)}
          value={text}
        />
        <div className="flex items-center gap-2">
          {reviewable ? (
            <>
              <Button
                disabled={busy}
                onClick={() =>
                  run(
                    "approve",
                    () => approveTask({ data: task.id }),
                    "Task approved — branch pushed"
                  )
                }
                size="sm"
                type="button"
                variant={task.status === "conflicted" ? "outline" : "default"}
              >
                <ActionIcon busy={pending === "approve"} icon={Check} />
                Approve
              </Button>
              <Button
                className={cn(armed && ARMED_CLASS)}
                disabled={busy}
                onClick={() =>
                  armed
                    ? run(
                        "reject",
                        () => rejectTask({ data: task.id }),
                        "Task rejected — worktree discarded"
                      )
                    : arm()
                }
                ref={rejectRef}
                size="sm"
                type="button"
                variant="outline"
              >
                <ActionIcon busy={pending === "reject"} icon={Trash} />
                {armed ? "Confirm reject" : "Reject"}
              </Button>
            </>
          ) : null}
          {retryable ? (
            <Button
              disabled={busy}
              onClick={() =>
                run(
                  "retry",
                  () => retryTask({ data: task.id }),
                  "Task requeued"
                )
              }
              size="sm"
              type="button"
            >
              <ActionIcon
                busy={pending === "retry"}
                icon={ArrowCounterClockwise}
              />
              Retry
            </Button>
          ) : null}
          <Button
            aria-label="Send follow-up"
            className="ml-auto rounded-full"
            disabled={busy || !respondable || text.trim() === ""}
            size="icon-sm"
            type="submit"
          >
            <ActionIcon busy={pending === "follow-up"} icon={ArrowUp} />
          </Button>
        </div>
      </form>
      {reviewable || retryable ? (
        <p
          aria-live="polite"
          className="mt-2 px-2 text-muted-foreground text-xs"
        >
          {hint(task.status, armed)}
        </p>
      ) : null}
    </div>
  );
}

function hint(status: TaskStatus, armed: boolean): string {
  if (armed) {
    return "Deletes the worktree and branch. This cannot be undone.";
  }
  if (RETRYABLE.includes(status)) {
    return "Retry queues the task again on the same runner and executor, as a new paid run.";
  }
  return "Approve pushes the branch. Reject discards the work. A follow-up resumes the agent as a new paid run.";
}
