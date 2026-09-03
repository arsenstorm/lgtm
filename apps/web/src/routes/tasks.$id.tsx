import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute, useRouter } from "@tanstack/react-router";
import { useEffect } from "react";

import { OrchestratorError } from "@/components/orchestrator-error";
import { TaskDetailView } from "@/components/task-detail";
import { getTask } from "@/lib/lgtm/server";

export const Route = createFileRoute("/tasks/$id")({
  loader: ({ params }) => getTask({ data: params.id }),
  component: TaskDetailPage,
  errorComponent: TaskDetailError,
});

const POLL_MS = 2500;

function TaskDetailPage() {
  const detail = Route.useLoaderData();
  const router = useRouter();
  const status = detail.task.status;
  const live = status === "queued" || status === "running";

  // The orchestrator has no push channel to the browser; while the agent works
  // the loader is re-run on a short interval so the transcript streams in.
  useEffect(() => {
    if (!live) {
      return;
    }
    const id = window.setInterval(() => router.invalidate(), POLL_MS);
    return () => window.clearInterval(id);
  }, [live, router]);

  return <TaskDetailView detail={detail} />;
}

function TaskDetailError(props: ErrorComponentProps) {
  return <OrchestratorError what="this task" {...props} />;
}
