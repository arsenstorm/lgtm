import type { Icon } from "@phosphor-icons/react";
import {
  ChatCircleText,
  CheckCircle,
  CircleNotch,
  Clock,
  Eye,
  GitBranch,
  GitMerge,
  MinusCircle,
  Plugs,
  Prohibit,
  Timer,
  XCircle,
} from "@phosphor-icons/react";
import { Link } from "@tanstack/react-router";

import { TimeAgo } from "@/components/time-ago";
import type { Task, TaskStatus } from "@/lib/lgtm/types";
import { cn, taskTitle } from "@/lib/utils";

/**
 * Tone groups the twelve statuses into the four things an operator actually
 * does next: wait, act, celebrate, investigate. Colour alone can't carry that,
 * so every status also keeps a distinct glyph and its spelled-out label.
 */
type Tone = "idle" | "live" | "attention" | "done" | "broken";

export const STATUS: Record<
  TaskStatus,
  { label: string; icon: Icon; tone: Tone }
> = {
  approved: { icon: CheckCircle, label: "Approved", tone: "done" },
  awaiting_review: { icon: Eye, label: "Awaiting review", tone: "attention" },
  cancelled: { icon: MinusCircle, label: "Cancelled", tone: "idle" },
  changes_requested: {
    icon: ChatCircleText,
    label: "Changes requested",
    tone: "attention",
  },
  conflicted: { icon: GitBranch, label: "Conflicted", tone: "attention" },
  failed: { icon: XCircle, label: "Failed", tone: "broken" },
  merged: { icon: GitMerge, label: "Merged", tone: "done" },
  queued: { icon: Clock, label: "Queued", tone: "idle" },
  rejected: { icon: Prohibit, label: "Rejected", tone: "idle" },
  runner_lost: { icon: Plugs, label: "Runner lost", tone: "broken" },
  running: { icon: CircleNotch, label: "Running", tone: "live" },
  timed_out: { icon: Timer, label: "Timed out", tone: "broken" },
};

const TONE_TEXT: Record<Tone, string> = {
  attention: "text-amber-700 dark:text-amber-400",
  broken: "text-red-700 dark:text-red-400",
  done: "text-emerald-700 dark:text-emerald-400",
  idle: "text-muted-foreground",
  live: "text-blue-700 dark:text-blue-400",
};

const UNITS: [ms: number, suffix: string][] = [
  [86_400_000, "d"],
  [3_600_000, "h"],
  [60_000, "m"],
  [1000, "s"],
];

/** Coarse duration — "4h", "12d". Used for ages, windows and medians alike. */
export function shortSpan(ms: number): string {
  const abs = Math.max(0, ms);
  for (const [size, suffix] of UNITS) {
    if (abs >= size) {
      return `${Math.floor(abs / size)}${suffix}`;
    }
  }
  return "0s";
}

export function TaskList({ tasks }: { tasks: Task[] }) {
  if (tasks.length === 0) {
    return (
      <div className="flex min-h-64 flex-col items-center justify-center gap-2 rounded-xl border border-foreground/15 border-dashed p-8 text-center">
        <h2 className="font-medium text-base">No tasks yet</h2>
        <p className="max-w-[52ch] text-pretty text-base text-muted-foreground sm:text-sm">
          Tasks appear here the moment the orchestrator accepts one. Queue the
          first from your terminal:
        </p>
        <code className="mt-1 text-sm">
          lgtm task &quot;fix the flaky login test&quot;
        </code>
      </div>
    );
  }

  return (
    <ul className="-mx-2 divide-y divide-foreground/5" role="list">
      {tasks.map((task) => (
        <TaskRow key={task.id} task={task} />
      ))}
    </ul>
  );
}

function TaskRow({ task }: { task: Task }) {
  const { label, icon: Icon, tone } = STATUS[task.status];
  const needsHuman = tone === "attention";

  return (
    <li>
      <Link
        className={cn(
          "relative flex flex-wrap items-center gap-x-3 gap-y-1.5 rounded-md px-2 py-2.5 text-sm hover:bg-foreground/4 sm:flex-nowrap sm:gap-x-4",
          // A bar, not just a hue: rows a human is blocking on stay findable
          // at a glance and in a screenshot printed in greyscale.
          needsHuman &&
            'before:absolute before:inset-y-1 before:start-0 before:w-0.5 before:rounded-full before:bg-amber-500 before:content-[""]'
        )}
        params={{ id: task.id }}
        to="/tasks/$id"
      >
        <span
          className={cn(
            "flex w-36 shrink-0 items-center gap-1.5 whitespace-nowrap font-medium",
            TONE_TEXT[tone]
          )}
        >
          <Icon
            aria-hidden="true"
            className={cn(
              "size-4 h-lh shrink-0",
              task.status === "running" &&
                "[animation-duration:1.8s] motion-safe:animate-spin"
            )}
          />
          {label}
        </span>

        <span className="w-18 shrink-0 font-mono text-muted-foreground tabular-nums">
          {task.id.slice(0, 8)}
        </span>

        <p className="order-last min-w-0 basis-full overflow-hidden whitespace-nowrap text-base text-foreground [mask-image:linear-gradient(to_right,black_calc(100%-1.5rem),transparent)] sm:order-none sm:flex-1 sm:basis-auto sm:text-sm">
          {taskTitle(task)}
        </p>

        <span className="w-24 shrink-0 truncate text-end text-muted-foreground">
          {task.runner ?? "—"}
        </span>

        <TimeAgo
          at={task.created_at}
          className="grow text-end text-muted-foreground tabular-nums sm:w-16 sm:shrink-0 sm:grow-0"
        />
      </Link>
    </li>
  );
}
