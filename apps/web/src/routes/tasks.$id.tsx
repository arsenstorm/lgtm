import { createFileRoute } from '@tanstack/react-router'

import { TaskDetailView } from '@/components/task-detail'
import { getTask } from '@/lib/lgtm/server'

export const Route = createFileRoute('/tasks/$id')({
  loader: ({ params }) => getTask({ data: params.id }),
  component: TaskDetailPage,
})

function TaskDetailPage() {
  return <TaskDetailView detail={Route.useLoaderData()} />
}
