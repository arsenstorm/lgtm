import { useMatchRoute } from "@tanstack/react-router";

import { Separator } from "@/components/ui/separator";
import { SidebarTrigger } from "@/components/ui/sidebar";

const TITLES = [
  ["/runners", "Runners"],
  ["/todos", "Todos"],
  ["/memories", "Memories"],
  ["/scratchpads", "Scratchpads"],
  ["/sessions", "Sessions"],
  ["/activity", "Activity"],
] as const;

/** The header names where you are; the pages themselves lead with content. */
function useTitle(): string {
  const matchRoute = useMatchRoute();
  // Ahead of `/tasks/$id`: a static segment must win however the two are
  // ordered by the matcher.
  if (matchRoute({ to: "/tasks/new" })) {
    return "New task";
  }
  const task = matchRoute({ to: "/tasks/$id" });
  if (task) {
    return `Task ${task.id}`;
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
  return "Tasks";
}

export function SiteHeader() {
  const title = useTitle();

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
