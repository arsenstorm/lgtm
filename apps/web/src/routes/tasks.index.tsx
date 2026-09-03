import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute } from "@tanstack/react-router";

import { OrchestratorError } from "@/components/orchestrator-error";
import { TaskTriage } from "@/components/task-triage";
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
    <div className="mx-auto flex w-full max-w-5xl flex-col px-4 py-6 sm:px-6 lg:px-8">
      <TaskTriage stats={stats} tasks={tasks} />
    </div>
  );
}

function TasksError(props: ErrorComponentProps) {
  return <OrchestratorError what="tasks" {...props} />;
}
