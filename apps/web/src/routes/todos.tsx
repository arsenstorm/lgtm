import { Link, createFileRoute } from '@tanstack/react-router'
import type { ErrorComponentProps } from '@tanstack/react-router'
import { CheckCircle, Circle, CircleHalf } from '@phosphor-icons/react'
import type { Icon } from '@phosphor-icons/react'

import { projectName } from '@/components/app-sidebar'
import { OrchestratorError } from '@/components/orchestrator-error'
import { relativeAge } from '@/components/task-list'
import { Badge } from '@/components/ui/badge'
import { getTodos } from '@/lib/lgtm/server'
import type { Todo, TodoStatus } from '@/lib/lgtm/types'
import { cn } from '@/lib/utils'

export const Route = createFileRoute('/todos')({
  loader: async () => ({ todos: await getTodos() }),
  component: TodosPage,
  errorComponent: TodosError,
})

export const MARK: Record<TodoStatus, { icon: Icon; label: string; className: string }> = {
  open: { icon: Circle, label: 'Open', className: 'text-muted-foreground' },
  in_progress: { icon: CircleHalf, label: 'In progress', className: 'text-foreground' },
  done: { icon: CheckCircle, label: 'Done', className: 'text-emerald-700 dark:text-emerald-400' },
}

const PRIORITY_RANK = { high: 0, medium: 1, low: 2 }

interface Group {
  key: string
  label: string
  todos: Todo[]
}

function group(todos: Todo[]): Group[] {
  const byRepository = new Map<string | null, Todo[]>()
  for (const todo of todos) {
    const bucket = byRepository.get(todo.repository)
    if (bucket) {
      bucket.push(todo)
    } else {
      byRepository.set(todo.repository, [todo])
    }
  }

  return [...byRepository]
    .map(([repository, list]) => ({
      key: repository ?? '',
      label: repository === null ? 'Every repository' : projectName(repository),
      todos: [...list].sort(
        (a, b) =>
          Number(a.status === 'done') - Number(b.status === 'done') ||
          PRIORITY_RANK[a.priority] - PRIORITY_RANK[b.priority] ||
          b.created_at - a.created_at,
      ),
    }))
    // Repository-wide todos apply everywhere, so they lead.
    .sort((a, b) => Number(!!a.key) - Number(!!b.key) || a.label.localeCompare(b.label))
}

function TodosPage() {
  const { todos } = Route.useLoaderData()
  const groups = group(todos)
  const openCount = todos.filter((todo) => todo.status !== 'done').length

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-7xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <div className="flex items-baseline gap-3">
        <h1 className="text-xl font-medium tracking-tight">Todos</h1>
        <span className="text-sm tabular-nums text-muted-foreground">{openCount} open</span>
      </div>

      {groups.length === 0 ? (
        <p className="text-sm text-muted-foreground">No todos yet.</p>
      ) : (
        groups.map((entry) => (
          <section key={entry.key} className="flex flex-col gap-2">
            <h2 className="text-sm font-medium text-muted-foreground">{entry.label}</h2>
            <ul role="list" className="-mx-2 divide-y divide-foreground/5">
              {entry.todos.map((todo) => (
                <li key={todo.id}>
                  <TodoRow todo={todo} />
                </li>
              ))}
            </ul>
          </section>
        ))
      )}
    </div>
  )
}

function TodoRow({ todo }: { todo: Todo }) {
  const { icon: Mark, label, className } = MARK[todo.status]
  const done = todo.status === 'done'

  return (
    <Link
      to="/todos/$id"
      params={{ id: todo.id }}
      className={cn(
        'flex items-start gap-3 rounded-md px-2 py-2.5 text-sm hover:bg-foreground/4',
        done && 'text-muted-foreground',
      )}
    >
      <Mark
        aria-label={label}
        role="img"
        className={cn('size-4 h-lh shrink-0', className)}
        weight={done ? 'fill' : 'regular'}
      />

      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex min-w-0 items-center gap-2">
          <span className="min-w-0 truncate">{todo.title}</span>
          {todo.priority === 'high' && (
            <Badge
              variant="outline"
              className="border-amber-600/30 text-amber-700 dark:text-amber-400"
            >
              high
            </Badge>
          )}
        </div>
        {todo.description && (
          <p className="min-w-0 truncate text-xs text-muted-foreground">{todo.description}</p>
        )}
      </div>

      {todo.blockers.length > 0 && (
        <span className="shrink-0 text-xs text-muted-foreground">
          blocked by {todo.blockers.length}
        </span>
      )}

      <time
        dateTime={new Date(todo.created_at).toISOString()}
        suppressHydrationWarning
        className="w-16 shrink-0 text-end tabular-nums text-muted-foreground"
      >
        {relativeAge(todo.created_at)}
      </time>
    </Link>
  )
}

function TodosError(props: ErrorComponentProps) {
  return <OrchestratorError what="todos" {...props} />
}
