import { Link, createFileRoute } from '@tanstack/react-router'
import type { ErrorComponentProps } from '@tanstack/react-router'

import { OrchestratorError } from '@/components/orchestrator-error'
import { STATUS, relativeAge } from '@/components/task-list'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { getTasks } from '@/lib/lgtm/server'
import type { Task } from '@/lib/lgtm/types'

export const Route = createFileRoute('/scratchpads')({
  loader: async () => ({ tasks: await getTasks() }),
  component: ScratchpadsPage,
  errorComponent: ScratchpadsError,
})

function firstLine(prompt: string): string {
  const line = prompt.split('\n', 1)[0]?.trim()
  return line ? line : '(no prompt)'
}

function ScratchpadsPage() {
  const { tasks } = Route.useLoaderData()
  const withNotes = tasks
    .filter((task) => task.scratchpad.trim() !== '')
    .sort((a, b) => b.created_at - a.created_at)

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-7xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <div className="flex items-baseline gap-3">
        <h1 className="text-xl font-medium tracking-tight">Scratchpads</h1>
        <span className="text-sm tabular-nums text-muted-foreground">{withNotes.length}</span>
      </div>

      {withNotes.length === 0 ? (
        <p className="text-sm text-muted-foreground">No scratchpads yet.</p>
      ) : (
        <ul role="list" className="flex flex-col gap-4">
          {withNotes.map((task) => (
            <li key={task.id}>
              <ScratchpadCard task={task} />
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

function ScratchpadCard({ task }: { task: Task }) {
  const { label } = STATUS[task.status]

  return (
    <Card>
      <CardHeader>
        <CardTitle className="min-w-0">
          <Link
            to="/tasks/$id"
            params={{ id: task.id }}
            className="block truncate underline-offset-4 hover:underline"
          >
            {firstLine(task.spec.prompt)}
          </Link>
        </CardTitle>
        <div className="text-sm text-muted-foreground">
          {label} ·{' '}
          <time dateTime={new Date(task.created_at).toISOString()} suppressHydrationWarning>
            {relativeAge(task.created_at)}
          </time>
        </div>
      </CardHeader>

      <CardContent>
        {/* The pad scrolls inside the card; a wide line must never widen the page. */}
        <pre className="max-h-80 overflow-auto rounded-md bg-foreground/4 p-3 font-mono text-xs">
          {task.scratchpad}
        </pre>
      </CardContent>
    </Card>
  )
}

function ScratchpadsError(props: ErrorComponentProps) {
  return <OrchestratorError what="scratchpads" {...props} />
}
