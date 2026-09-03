import { useMatchRoute } from "@tanstack/react-router";

import { Separator } from "@/components/ui/separator";
import { SidebarTrigger } from "@/components/ui/sidebar";
import type { Task } from "@/lib/lgtm/types";
import { taskTitle } from "@/lib/utils";

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
function useTitle(tasks: Task[]): string {
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

export function SiteHeader({ tasks }: { tasks: Task[] }) {
  const title = useTitle(tasks);

  return (
    <header className="flex h-(--header-height) shrink-0 items-center gap-2 border-b">
      <div className="flex w-full items-center gap-1 px-4 lg:gap-2 lg:px-6">
        <SidebarTrigger className="-ml-1" />
        <Separator
          className="mx-2 data-[orientation=vertical]:h-4"
          orientation="vertical"
        />
        <h1 className="font-medium text-base">{title}</h1>
      </div>
    </header>
  );
}
