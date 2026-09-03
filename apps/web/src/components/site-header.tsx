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
  const task = matchRoute({ to: "/tasks/$id" });
  if (task) {
    return `Task ${task.id}`;
  }
  const todo = matchRoute({ to: "/todos/$id" });
  if (todo) {
    return `Todo ${todo.id}`;
  }
  const scratchpad = matchRoute({ to: "/scratchpads/$id" });
  if (scratchpad) {
    return `Scratchpad ${scratchpad.id}`;
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
