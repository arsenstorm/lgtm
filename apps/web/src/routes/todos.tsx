import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute } from "@tanstack/react-router";
import { projectName } from "@/components/app-sidebar";
import { ListGroup } from "@/components/list-group";
import { OrchestratorError } from "@/components/orchestrator-error";
import { PageHeading } from "@/components/page-heading";
import { TodoRow } from "@/components/todo-row";
import { getTodos } from "@/lib/lgtm/server";
import type { Todo, TodoStatus } from "@/lib/lgtm/types";

export const Route = createFileRoute("/todos")({
  loader: async () => ({ todos: await getTodos() }),
  component: TodosPage,
  errorComponent: TodosError,
});

const PRIORITY_RANK = { high: 0, medium: 1, low: 2 };

// Work in flight leads, the backlog follows, finished work sinks.
const STATUS_RANK: Record<TodoStatus, number> = {
  in_progress: 0,
  open: 1,
  done: 2,
};

function byProject(todos: Todo[]) {
  const groups = new Map<string, Todo[]>();
  for (const todo of todos) {
    const name = todo.repository ? projectName(todo.repository) : "general";
    groups.set(name, [...(groups.get(name) ?? []), todo]);
  }
  return [...groups]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([name, list]) => ({
      name,
      todos: list.sort(
        (a, b) =>
          STATUS_RANK[a.status] - STATUS_RANK[b.status] ||
          PRIORITY_RANK[a.priority] - PRIORITY_RANK[b.priority] ||
          b.created_at - a.created_at
      ),
    }));
}

function TodosPage() {
  const { todos } = Route.useLoaderData();
  const openCount = todos.filter((todo) => todo.status !== "done").length;
  const groups = byProject(todos);

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-6 px-4 py-6 sm:px-6 lg:px-8">
      <PageHeading meta={`${openCount} open`} title="Todos" />

      {groups.length === 0 ? (
        <p className="text-muted-foreground text-sm">No todos yet.</p>
      ) : (
        <div className="flex flex-col gap-1">
          {groups.map(({ name, todos: list }) => (
            <ListGroup count={list.length} key={name} label={name}>
              <ul className="flex flex-col py-1">
                {list.map((todo) => (
                  <li key={todo.id}>
                    <TodoRow project={false} todo={todo} />
                  </li>
                ))}
              </ul>
            </ListGroup>
          ))}
        </div>
      )}
    </div>
  );
}

function TodosError(props: ErrorComponentProps) {
  return <OrchestratorError what="todos" {...props} />;
}
