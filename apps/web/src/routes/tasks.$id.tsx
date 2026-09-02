import { createFileRoute } from '@tanstack/react-router'
import type { ErrorComponentProps } from '@tanstack/react-router'

import { OrchestratorError } from '@/components/orchestrator-error'
import { TaskDetailView } from '@/components/task-detail'
import { getTask } from '@/lib/lgtm/server'

export const Route = createFileRoute('/tasks/$id')({
  loader: ({ params }) => getTask({ data: params.id }),
  component: TaskDetailPage,
  errorComponent: TaskDetailError,
})

function TaskDetailPage() {
  return <TaskDetailView detail={Route.useLoaderData()} />
}

function TaskDetailError(props: ErrorComponentProps) {
  return <OrchestratorError what="this task" {...props} />
}
