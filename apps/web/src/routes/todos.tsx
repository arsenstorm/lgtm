import type { Icon } from "@phosphor-icons/react";
import { CaretRight, CheckCircle, Circle, CircleHalf } from "@phosphor-icons/react";
import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute, Link } from "@tanstack/react-router";

import { projectName } from "@/components/app-sidebar";
import { OrchestratorError } from "@/components/orchestrator-error";
import { PriorityIcon } from "@/components/priority-icon";
import { TimeAgo } from "@/components/time-ago";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { getTodos } from "@/lib/lgtm/server";
import type { Todo, TodoStatus } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/todos")({
  loader: async () => ({ todos: await getTodos() }),
  component: TodosPage,
  errorComponent: TodosError,
});

export const MARK: Record<
  TodoStatus,
  { icon: Icon; label: string; className: string }
> = {
  open: { icon: Circle, label: "Open", className: "text-muted-foreground" },
  in_progress: {
    icon: CircleHalf,
    label: "In progress",
    className: "text-amber-600 dark:text-amber-400",
  },
  done: {
    icon: CheckCircle,
    label: "Done",
    className: "text-emerald-700 dark:text-emerald-400",
  },
};


const PRIORITY_RANK = { high: 0, medium: 1, low: 2 };

// Work in flight leads, the backlog follows, finished work sinks.
const STATUS_ORDER: TodoStatus[] = ["in_progress", "open", "done"];

function TodosPage() {
  const { todos } = Route.useLoaderData();
  const openCount = todos.filter((todo) => todo.status !== "done").length;
  const groups = STATUS_ORDER.map((status) => ({
    status,
    todos: todos
      .filter((todo) => todo.status === status)
      .sort(
        (a, b) =>
          PRIORITY_RANK[a.priority] - PRIORITY_RANK[b.priority] ||
          b.created_at - a.created_at
      ),
  })).filter((group) => group.todos.length > 0);

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-7xl flex-col gap-6 px-4 py-6 sm:px-6 lg:px-8">
      <div className="flex items-baseline gap-3">
        <h1 className="font-medium text-xl tracking-tight">Todos</h1>
        <span className="text-muted-foreground text-sm tabular-nums">
          {openCount} open
        </span>
      </div>

      {groups.length === 0 ? (
        <p className="text-muted-foreground text-sm">No todos yet.</p>
      ) : (
        <div className="flex flex-col gap-1">
          {groups.map(({ status, todos: list }) => (
            <StatusGroup key={status} status={status} todos={list} />
          ))}
        </div>
      )}
    </div>
  );
}

function StatusGroup({ status, todos }: { status: TodoStatus; todos: Todo[] }) {
  const { icon: Mark, label, className } = MARK[status];

  return (
    <Collapsible defaultOpen>
      <CollapsibleTrigger className="group/header flex w-full items-center gap-2 rounded-md bg-muted/50 px-2 py-1.5 text-sm outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50">
        <CaretRight
          aria-hidden="true"
          className="size-3 text-muted-foreground transition-transform duration-200 group-data-[panel-open]/header:rotate-90"
        />
        <Mark
          aria-hidden="true"
          className={cn("size-4", className)}
          weight={status === "done" ? "fill" : "regular"}
        />
        <span className="font-medium">{label}</span>
        <span className="text-muted-foreground tabular-nums">
          {todos.length}
        </span>
      </CollapsibleTrigger>

      <CollapsibleContent>
        <ul className="flex flex-col py-1">
          {todos.map((todo) => (
            <li key={todo.id}>
              <TodoRow todo={todo} />
            </li>
          ))}
        </ul>
      </CollapsibleContent>
    </Collapsible>
  );
}

function TodoRow({ todo }: { todo: Todo }) {
  const { icon: Mark, label, className } = MARK[todo.status];
  const done = todo.status === "done";

  return (
    <Link
      className={cn(
        "flex items-center gap-2.5 rounded-md py-1.5 pr-2 pl-7 text-sm hover:bg-foreground/4",
        done && "text-muted-foreground"
      )}
      params={{ id: todo.id }}
      to="/todos/$id"
    >
      <PriorityIcon
        className="size-4 text-muted-foreground"
        label={`${todo.priority} priority`}
        priority={todo.priority}
      />
      <span className="shrink-0 font-mono text-muted-foreground text-xs">
        {todo.display_id}
      </span>
      <Mark
        aria-label={label}
        className={cn("size-4 shrink-0", className)}
        role="img"
        weight={done ? "fill" : "regular"}
      />
      <span className="min-w-0 truncate">{todo.title}</span>
      {todo.blockers.length > 0 && (
        <span className="shrink-0 text-muted-foreground text-xs">
          blocked by {todo.blockers.length}
        </span>
      )}
      <span className="ml-auto hidden shrink-0 text-muted-foreground text-xs sm:block">
        {todo.repository ? projectName(todo.repository) : "every repository"}
      </span>
      <TimeAgo
        at={todo.created_at}
        className="w-16 shrink-0 text-end text-muted-foreground text-xs tabular-nums"
      />
    </Link>
  );
}

function TodosError(props: ErrorComponentProps) {
  return <OrchestratorError what="todos" {...props} />;
}
