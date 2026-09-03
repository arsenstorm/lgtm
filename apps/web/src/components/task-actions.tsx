import {
  ArrowCounterClockwise,
  Check,
  PaperPlaneTilt,
  Trash,
} from "@phosphor-icons/react";
import type { FormEvent, ReactNode } from "react";
import { useState } from "react";

import { ActionIcon } from "@/components/action-icon";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
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

export function TaskActions({ task }: { task: Task }) {
  const [followUp, setFollowUp] = useState("");
  const { armed, arm, disarm, ref: rejectRef } = useArmedConfirm();
  const { pending, busy, run } = useAction<Action>({ onStart: disarm });

  if (RETRYABLE.includes(task.status)) {
    return (
      <Panel>
        <div className="flex flex-wrap items-center gap-3">
          <Button
            className="relative"
            disabled={busy}
            onClick={() =>
              run("retry", () => retryTask({ data: task.id }), "Task requeued")
            }
            size="lg"
          >
            <ActionIcon
              busy={pending === "retry"}
              icon={ArrowCounterClockwise}
            />
            Retry
            <TouchTarget />
          </Button>
        </div>
        <Hint>
          Queues the task again on the same runner and executor, as a new paid
          run.
        </Hint>
      </Panel>
    );
  }

  if (!REVIEWABLE.includes(task.status)) {
    return null;
  }

  // On a conflict the agent, not the reviewer, is the one who can move the task
  // forward — so the follow-up leads and approve steps back to a quiet button.
  const conflicted = task.status === "conflicted";

  async function submitFollowUp(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const text = followUp.trim();
    if (!text || busy) {
      return;
    }
    const sent = await run(
      "follow-up",
      () => sendFollowUp({ data: { id: task.id, text } }),
      "Follow-up sent"
    );
    if (sent) {
      setFollowUp("");
    }
  }

  const decide = (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-3">
        <Button
          className="relative"
          disabled={busy}
          onClick={() =>
            run(
              "approve",
              () => approveTask({ data: task.id }),
              "Task approved — branch pushed"
            )
          }
          size="lg"
          variant={conflicted ? "outline" : "default"}
        >
          <ActionIcon busy={pending === "approve"} icon={Check} />
          Approve
          <TouchTarget />
        </Button>
        <Button
          className={cn("relative", armed && ARMED_CLASS)}
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
          size="lg"
          variant="destructive"
        >
          <ActionIcon busy={pending === "reject"} icon={Trash} />
          {armed ? "Confirm reject" : "Reject"}
          <TouchTarget />
        </Button>
      </div>
      <Hint live>
        {/* Both strings are kept under one line at 60ch so arming the button
            does not reflow the form below it. */}
        {armed
          ? "Deletes the worktree and branch. This cannot be undone."
          : "Approve pushes the branch. Reject discards the work."}
      </Hint>
    </div>
  );

  const respond = (
    <form className="flex flex-col gap-2" onSubmit={submitFollowUp}>
      <div className="flex gap-3">
        <Input
          aria-label="Follow-up instructions for the agent"
          className="h-9 max-w-md"
          disabled={busy}
          name="follow-up"
          onChange={(event) => setFollowUp(event.target.value)}
          placeholder={
            conflicted
              ? "Tell the agent how to resolve the conflict…"
              : "Ask for a change…"
          }
          value={followUp}
        />
        <Button
          className="relative"
          disabled={busy || followUp.trim() === ""}
          size="lg"
          type="submit"
          variant={conflicted ? "default" : "outline"}
        >
          <ActionIcon busy={pending === "follow-up"} icon={PaperPlaneTilt} />
          Send follow-up
          <TouchTarget />
        </Button>
      </div>
      <Hint>Resumes the agent with these instructions, as a new paid run.</Hint>
    </form>
  );

  return (
    <Panel>
      {conflicted ? respond : decide}
      <div className="border-t" />
      {conflicted ? decide : respond}
    </Panel>
  );
}

function Panel({ children }: { children: ReactNode }) {
  return (
    <section
      aria-label="Task actions"
      className="flex flex-col gap-4 rounded-lg border bg-muted/30 p-4"
    >
      {children}
    </section>
  );
}

function Hint({ children, live }: { children: ReactNode; live?: boolean }) {
  return (
    <p
      aria-live={live ? "polite" : undefined}
      className="max-w-[60ch] text-pretty text-muted-foreground text-xs"
    >
      {children}
    </p>
  );
}

/** A 36px control is under the touch minimum; this grows the tap area on coarse
 *  pointers only. Sized to the row gap so neighbouring targets never overlap. */
function TouchTarget() {
  return (
    <span
      aria-hidden="true"
      className="-translate-1/2 absolute top-1/2 left-1/2 pointer-fine:hidden size-[max(100%,3rem)]"
    />
  );
}
