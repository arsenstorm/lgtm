import { SidebarSimple, Stack, XCircle } from "@phosphor-icons/react";
import { useEffect, useState } from "react";

import { FilePath } from "@/components/file-path";
import { TaskActions } from "@/components/task-actions";
import { TaskSummaryPanel } from "@/components/task-summary-panel";
import { TaskTranscript } from "@/components/task-transcript";
import { TimeAgo } from "@/components/time-ago";
import { Button } from "@/components/ui/button";
import type { Overlap, TaskDetail, TaskStatus } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

const PANEL_KEY = "lgtm-task-panel-open";

/** Reads the stored choice after paint: reading during render would disagree
 * with the panel-open markup the server sent and mismatch on hydration. */
function usePanelOpen() {
  const [open, setOpen] = useState(true);

  useEffect(() => {
    try {
      const stored = window.localStorage.getItem(PANEL_KEY);
      if (stored !== null) {
        setOpen(stored === "true");
      }
    } catch {
      // Unavailable storage just means the panel starts open every visit.
    }
  }, []);

  function toggle() {
    setOpen((current) => {
      try {
        window.localStorage.setItem(PANEL_KEY, String(!current));
      } catch {
        // Same trade as above: the toggle still works, it is not remembered.
      }
      return !current;
    });
  }

  return { open, toggle };
}

export function TaskDetailView({ detail }: { detail: TaskDetail }) {
  const { task, overlaps } = detail;
  const { open, toggle } = usePanelOpen();

  return (
    <div className="mx-auto flex w-full max-w-7xl flex-col gap-6 px-4 py-6 sm:px-6 lg:px-8">
      <header className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <StatusPill status={task.status} />
        <span className="font-mono text-muted-foreground text-sm">
          {task.id}
        </span>
        <span aria-hidden className="text-muted-foreground/40">
          ·
        </span>
        <TimeAgo
          at={task.created_at}
          className="text-muted-foreground text-sm"
        />
        <Button
          aria-expanded={open}
          aria-label={open ? "Hide summary panel" : "Show summary panel"}
          className="ml-auto text-muted-foreground"
          onClick={toggle}
          size="icon-sm"
          variant="ghost"
        >
          {/* The glyph draws its panel on the left; flipping it points at the
              panel this button actually controls. */}
          <SidebarSimple aria-hidden="true" className="size-4.5 -scale-x-100" />
        </Button>
      </header>

      {task.error ? <ErrorPanel error={task.error} /> : null}
      {overlaps.length > 0 ? <OverlapPanel overlaps={overlaps} /> : null}

      <div
        className={cn(
          "grid min-w-0 gap-10",
          open && "lg:grid-cols-[minmax(0,1fr)_minmax(0,26rem)]"
        )}
      >
        <section
          aria-label="Task transcript"
          className="flex min-w-0 flex-col gap-8"
        >
          <TaskTranscript events={detail.events} task={task} />
          <TaskActions task={task} />
        </section>
        {open ? (
          <TaskSummaryPanel
            className="lg:sticky lg:top-6 lg:max-h-[calc(100dvh-3rem)] lg:self-start lg:overflow-y-auto lg:pb-2"
            detail={detail}
          />
        ) : null}
      </div>
    </div>
  );
}

function ErrorPanel({ error }: { error: string }) {
  return (
    <section className="rounded-lg border border-destructive/30 bg-destructive/5 p-4">
      <h2 className="flex items-center gap-2 font-medium text-destructive text-sm">
        <XCircle className="size-4 shrink-0" />
        This task failed
      </h2>
      <pre className="mt-3 whitespace-pre-wrap text-xs leading-relaxed [overflow-wrap:anywhere]">
        {error}
      </pre>
    </section>
  );
}

function OverlapPanel({ overlaps }: { overlaps: Overlap[] }) {
  return (
    <section className="rounded-lg border border-amber-500/35 bg-amber-500/5 p-4">
      <h2 className="flex items-center gap-2 font-medium text-amber-700 text-sm dark:text-amber-400">
        <Stack className="size-4 shrink-0" />
        {overlaps.length === 1
          ? "One other unmerged task touches these files"
          : `${overlaps.length} other unmerged tasks touch these files`}
      </h2>
      <p className="mt-1 max-w-[54ch] text-pretty text-muted-foreground text-sm">
        Merging this task may conflict with work that has not landed yet.
      </p>
      <ul className="mt-3 flex flex-col gap-2">
        {overlaps.map((overlap) => (
          <li
            className="flex flex-wrap items-baseline gap-x-3 gap-y-1"
            key={overlap.task}
          >
            <span className="font-mono text-amber-800 text-xs dark:text-amber-300">
              {overlap.task}
            </span>
            <span className="flex min-w-0 flex-wrap gap-x-3 gap-y-1">
              {overlap.files.map((file) => (
                <FilePath className="text-xs" key={file} path={file} />
              ))}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}

const STATUS_TONE: Record<TaskStatus, string> = {
  approved:
    "border-emerald-500/35 bg-emerald-500/10 text-emerald-700 dark:text-emerald-400",
  awaiting_review:
    "border-amber-500/35 bg-amber-500/10 text-amber-700 dark:text-amber-400",
  cancelled: "border-border bg-muted text-muted-foreground",
  changes_requested:
    "border-amber-500/35 bg-amber-500/10 text-amber-700 dark:text-amber-400",
  conflicted:
    "border-amber-500/35 bg-amber-500/10 text-amber-700 dark:text-amber-400",
  failed: "border-destructive/35 bg-destructive/10 text-destructive",
  merged:
    "border-emerald-500/35 bg-emerald-500/10 text-emerald-700 dark:text-emerald-400",
  queued: "border-border bg-muted text-muted-foreground",
  rejected: "border-destructive/35 bg-destructive/10 text-destructive",
  runner_lost: "border-destructive/35 bg-destructive/10 text-destructive",
  running: "border-sky-500/35 bg-sky-500/10 text-sky-700 dark:text-sky-300",
  timed_out: "border-destructive/35 bg-destructive/10 text-destructive",
};

function StatusPill({ status }: { status: TaskStatus }) {
  const words = status.replace(/_/g, " ");

  return (
    <span
      className={cn(
        "inline-flex h-6 items-center gap-1.5 rounded-full border px-2.5 font-medium text-xs",
        STATUS_TONE[status]
      )}
    >
      <span aria-hidden className="size-1.5 shrink-0 rounded-full bg-current" />
      <span className="inline-block first-letter:uppercase">{words}</span>
    </span>
  );
}
