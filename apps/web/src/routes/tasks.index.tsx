import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute } from "@tanstack/react-router";

import { OrchestratorError } from "@/components/orchestrator-error";
import { StatTiles } from "@/components/stat-tiles";
import { TaskList } from "@/components/task-list";
import { getStats, getTasks } from "@/lib/lgtm/server";

export const Route = createFileRoute("/tasks/")({
  loader: async () => {
    const [tasks, stats] = await Promise.all([getTasks(), getStats()]);
    return { stats, tasks: tasks.filter((task) => !task.archived) };
  },
  component: TasksPage,
  errorComponent: TasksError,
});

function TasksPage() {
  const { tasks, stats } = Route.useLoaderData();

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-7xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <div className="flex items-baseline gap-3">
        <h1 className="font-medium text-xl tracking-tight">Tasks</h1>
        <span className="text-muted-foreground text-sm tabular-nums">
          {tasks.length}
        </span>
      </div>
      <StatTiles stats={stats} />
      <TaskList tasks={tasks} />
    </div>
  );
}

function TasksError(props: ErrorComponentProps) {
  return <OrchestratorError what="tasks" {...props} />;
}
