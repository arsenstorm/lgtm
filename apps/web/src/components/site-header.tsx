import { useMatchRoute } from "@tanstack/react-router";

import { SidebarToggleIcon } from "@/components/icons";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { useSidebar } from "@/components/ui/sidebar";
import type { Chat, Task, TaskStatus } from "@/lib/lgtm/types";
import { cn, taskTitle } from "@/lib/utils";

const TITLES = [
  ["/tasks", "Tasks"],
  ["/runners", "Runners"],
  ["/todos", "Todos"],
  ["/memories", "Memories"],
  ["/scratchpads", "Scratchpads"],
  ["/sessions", "Sessions"],
  ["/activity", "Activity"],
] as const;

/** The header names where you are; the pages themselves lead with content. */
function useTitle(tasks: Task[], chats: Chat[]): string {
  const matchRoute = useMatchRoute();
  // The composer is the home route; the task list lives at /tasks.
  if (matchRoute({ to: "/" })) {
    return "New task";
  }
  const match = matchRoute({ to: "/tasks/$id" });
  if (match) {
    const task = tasks.find((candidate) => candidate.id === match.id);
    return task ? taskTitle(task) : `Task ${match.id}`;
  }
  const chat = matchRoute({ to: "/chats/$id" });
  if (chat) {
    return chats.find((candidate) => candidate.id === chat.id)?.title ?? "Chat";
  }
  // The hex id says nothing a reader wants; the todo page leads with its own
  // display id, and a scratchpad with its title.
  if (matchRoute({ to: "/todos/$id" })) {
    return "Todo";
  }
  if (matchRoute({ to: "/scratchpads/$id" })) {
    return "Scratchpad";
  }
  for (const [to, title] of TITLES) {
    if (matchRoute({ to })) {
      return title;
    }
  }
  return "LGTM";
}

export function SiteHeader({
  tasks,
  chats,
  task,
  leftSidebar,
}: {
  chats: Chat[];
  leftSidebar: { shown: boolean; toggle: () => void };
  task?: Task;
  tasks: Task[];
}) {
  const title = useTitle(tasks, chats);
  const rightSidebar = useSidebar();
  const rightShown = rightSidebar.isMobile
    ? rightSidebar.openMobile
    : rightSidebar.open;

  return (
    <header className="flex h-(--header-height) shrink-0 items-center gap-2 border-b">
      <div className="flex w-full min-w-0 items-center gap-3 px-4 lg:px-6">
        <Button
          aria-expanded={leftSidebar.shown}
          aria-label={
            leftSidebar.shown
              ? "Hide navigation sidebar"
              : "Show navigation sidebar"
          }
          className="aria-expanded:!bg-transparent aria-expanded:!text-foreground -ml-1"
          onClick={leftSidebar.toggle}
          size="icon-sm"
          variant="ghost"
        >
          <SidebarToggleIcon show={!leftSidebar.shown} />
        </Button>
        <Separator
          className="data-[orientation=vertical]:h-4"
          orientation="vertical"
        />
        <h1 className="min-w-0 truncate font-medium text-base">{title}</h1>
        {task ? (
          <div className="ml-auto flex shrink-0 items-center gap-2">
            <StatusPill status={task.status} />
            <Button
              aria-expanded={rightShown}
              aria-label={
                rightShown ? "Hide task sidebar" : "Show task sidebar"
              }
              className="aria-expanded:!bg-transparent aria-expanded:!text-foreground"
              onClick={rightSidebar.toggleSidebar}
              size="icon-sm"
              variant="ghost"
            >
              <SidebarToggleIcon className="-scale-x-100" show={!rightShown} />
            </Button>
          </div>
        ) : null}
      </div>
    </header>
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
    <Badge
      aria-label={`Task status: ${words}`}
      className={cn("gap-1.5", STATUS_TONE[status])}
      variant="outline"
    >
      <span
        aria-hidden="true"
        className="size-1.5 shrink-0 rounded-full bg-current"
        data-icon="inline-start"
      />
      <span className="first-letter:uppercase">{words}</span>
    </Badge>
  );
}
