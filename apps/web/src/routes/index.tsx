import { createFileRoute } from '@tanstack/react-router'
import type { ErrorComponentProps } from '@tanstack/react-router'

import { StatTiles } from '@/components/stat-tiles'
import { TaskList } from '@/components/task-list'
import { getStats, getTasks } from '@/lib/lgtm/server'

export const Route = createFileRoute('/')({
  loader: async () => {
    const [tasks, stats] = await Promise.all([getTasks(), getStats()])
    return { tasks, stats }
  },
  component: TasksPage,
  errorComponent: TasksError,
})

function TasksPage() {
  const { tasks, stats } = Route.useLoaderData()

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-7xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <div className="flex items-baseline gap-3">
        <h1 className="text-xl font-medium tracking-tight">Tasks</h1>
        <span className="text-sm tabular-nums text-muted-foreground">{tasks.length}</span>
      </div>
      <StatTiles stats={stats} />
      <TaskList tasks={tasks} />
    </div>
  )
}

function TasksError({ error }: ErrorComponentProps) {
  return (
    <div className="m-4 flex flex-col gap-2 rounded-xl border border-red-600/25 bg-red-500/5 p-6 sm:m-6">
      <h1 className="text-base font-medium text-red-700 dark:text-red-400">
        Can&rsquo;t load tasks
      </h1>
      <p className="max-w-[68ch] text-base text-pretty text-muted-foreground sm:text-sm">
        The orchestrator did not answer. Check that <code className="text-sm">lgtm serve</code> is
        running and that <code className="text-sm">LGTM_ORCHESTRATOR</code> and{' '}
        <code className="text-sm">LGTM_TOKEN</code> in <code className="text-sm">.dev.vars</code>{' '}
        point at it.
      </p>
      <p className="font-mono text-sm break-words text-muted-foreground">{error.message}</p>
    </div>
  )
}
