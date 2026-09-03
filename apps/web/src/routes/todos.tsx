import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute } from "@tanstack/react-router";
import { ChevronIcon } from "@/components/icons";
import { OrchestratorError } from "@/components/orchestrator-error";
import { MARK } from "@/components/todo-chips";
import { TodoRow } from "@/components/todo-row";
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
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-6 px-4 py-6 sm:px-6 lg:px-8">
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
      <CollapsibleTrigger className="group/header flex w-full items-center gap-2 rounded-md bg-foreground/5 px-2 py-1.5 text-sm outline-none hover:bg-foreground/10 focus-visible:ring-2 focus-visible:ring-ring/50">
        <ChevronIcon
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

function TodosError(props: ErrorComponentProps) {
  return <OrchestratorError what="todos" {...props} />;
}
