import { Link } from "@tanstack/react-router";
import { useCallback, useState } from "react";

import { PageHeading } from "@/components/page-heading";
import { EmptyTasks, STATUS, TONE_TEXT } from "@/components/task-list";
import { TimeAgo } from "@/components/time-ago";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { Stats, Task, TaskStatus } from "@/lib/lgtm/types";
import { cn, taskTitle } from "@/lib/utils";

type Bucket = "needs_you" | "in_flight" | "done" | "did_not_land";

/** In the order a person works through them: what needs a human first. */
const BUCKETS: { key: Bucket; label: string; quiet: string }[] = [
  { key: "needs_you", label: "Needs you", quiet: "Nothing needs you" },
  { key: "in_flight", label: "In flight", quiet: "Nothing running" },
  { key: "done", label: "Done", quiet: "Nothing landed this week" },
  {
    key: "did_not_land",
    label: "Didn’t land",
    quiet: "Nothing failed this week",
  },
];

// Outcomes pile up for as long as the orchestrator runs, so their counts only
// mean something inside a window. Work that still needs someone is bounded by
// definition and shows in full.
const WINDOWED: ReadonlySet<Bucket> = new Set(["done", "did_not_land"]);

const PREVIEW = 5;

const USD = new Intl.NumberFormat("en-US", {
  currency: "USD",
  style: "currency",
});

function bucketOf(status: TaskStatus): Bucket {
  const { tone } = STATUS[status];
  if (tone === "attention") {
    return "needs_you";
  }
  if (tone === "live" || status === "queued") {
    return "in_flight";
  }
  if (tone === "done") {
    return "done";
  }
  return "did_not_land";
}

export function TaskTriage({ stats, tasks }: { stats: Stats; tasks: Task[] }) {
  if (tasks.length === 0) {
    return <EmptyTasks />;
  }
  const groups: Record<Bucket, Task[]> = {
    did_not_land: [],
    done: [],
    in_flight: [],
    needs_you: [],
  };
  for (const task of tasks) {
    groups[bucketOf(task.status)].push(task);
  }
  const recent = tasks.filter((task) => task.created_at >= stats.since).length;

  return (
    <div className="flex flex-col gap-10">
      <PageHeading meta={recent} title="Tasks">
        <p className="truncate text-muted-foreground text-sm tabular-nums">
          {USD.format(stats.cost_usd)} spent
          <span className="text-muted-foreground/60"> · </span>
          {USD.format(stats.spent_today)} today
        </p>
      </PageHeading>
      <TooltipProvider delay={300}>
        {BUCKETS.map((bucket) => (
          <Section
            key={bucket.key}
            label={bucket.label}
            quiet={bucket.quiet}
            since={WINDOWED.has(bucket.key) ? stats.since : 0}
            tasks={groups[bucket.key]}
          />
        ))}
      </TooltipProvider>
    </div>
  );
}

function Section({
  label,
  quiet,
  since,
  tasks,
}: {
  label: string;
  quiet: string;
  since: number;
  tasks: Task[];
}) {
  const [all, setAll] = useState(false);
  const toggle = useCallback(() => setAll((current) => !current), []);
  const recent = tasks.filter((task) => task.created_at >= since);
  const shown = all ? tasks : recent.slice(0, PREVIEW);
  const hidden = tasks.length - shown.length;
  const foldedRecent = recent.length > shown.length;

  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-baseline gap-2">
        <h2 className="truncate font-medium text-sm">{label}</h2>
        <span className="truncate text-muted-foreground text-sm tabular-nums">
          {recent.length}
        </span>
      </div>
      {shown.length === 0 ? (
        <p className="truncate py-2 text-muted-foreground/70 text-sm">
          {quiet}
        </p>
      ) : (
        // Rows keep an 8px inset for their hover surface; pulling the list out
        // by the same amount puts the glyphs on the title's edge.
        <ul className="-mx-2 divide-y divide-foreground/5" role="list">
          {shown.map((task) => (
            <Row key={task.id} task={task} />
          ))}
        </ul>
      )}
      {hidden > 0 || all ? (
        <button
          className="-ml-2 max-w-full self-start truncate rounded-md px-2 py-1 text-muted-foreground text-sm transition-colors hover:text-foreground"
          onClick={toggle}
          type="button"
        >
          {all
            ? "Show fewer"
            : `Show ${hidden} ${foldedRecent ? "more" : "older"}`}
        </button>
      ) : null}
    </section>
  );
}

function Row({ task }: { task: Task }) {
  const { label, icon: Icon, tone } = STATUS[task.status];

  return (
    <li>
      <Link
        className="flex items-center gap-3 rounded-md px-2 py-2 text-sm hover:bg-foreground/4"
        params={{ id: task.id }}
        to="/tasks/$id"
      >
        <Icon
          aria-label={label}
          className={cn(
            "size-4 shrink-0",
            TONE_TEXT[tone],
            task.status === "running" &&
              "[animation-duration:1.8s] motion-safe:animate-spin"
          )}
          role="img"
        />
        <span className="min-w-0 flex-1 overflow-hidden whitespace-nowrap [mask-image:linear-gradient(to_right,black_calc(100%-1.5rem),transparent)]">
          {taskTitle(task)}
        </span>
        <span
          className={cn(
            "hidden w-32 shrink-0 truncate text-end sm:block",
            TONE_TEXT[tone]
          )}
        >
          {label}
        </span>
        <RunnerName name={task.runner} />
        <TimeAgo
          at={task.created_at}
          className="w-14 shrink-0 truncate text-end text-muted-foreground tabular-nums"
        />
      </Link>
    </li>
  );
}

/** The column is narrow on purpose; a name that does not fit gets a tooltip,
 *  and only then, so short names stay quiet on hover. */
function RunnerName({ name }: { name: string | null }) {
  const [clipped, setClipped] = useState(false);
  const measure = useCallback(
    (el: HTMLSpanElement | null) =>
      setClipped(el !== null && el.scrollWidth > el.clientWidth),
    []
  );
  const text = (
    <span
      className="hidden w-24 shrink-0 truncate text-end text-muted-foreground md:block"
      ref={measure}
    >
      {name ?? "—"}
    </span>
  );
  if (!(clipped && name)) {
    return text;
  }
  return (
    <Tooltip>
      <TooltipTrigger render={text} />
      <TooltipContent>{name}</TooltipContent>
    </Tooltip>
  );
}
