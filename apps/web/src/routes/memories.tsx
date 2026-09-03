import { createFileRoute } from '@tanstack/react-router'
import type { ErrorComponentProps } from '@tanstack/react-router'

import { projectName } from '@/components/app-sidebar'
import { OrchestratorError } from '@/components/orchestrator-error'
import { relativeAge } from '@/components/task-list'
import { Badge } from '@/components/ui/badge'
import { getMemories } from '@/lib/lgtm/server'
import type { Memory } from '@/lib/lgtm/types'

export const Route = createFileRoute('/memories')({
  loader: async () => ({ memories: await getMemories() }),
  component: MemoriesPage,
  errorComponent: MemoriesError,
})

interface Group {
  key: string
  label: string
  memories: Memory[]
}

function group(memories: Memory[]): Group[] {
  const byRepository = new Map<string | null, Memory[]>()
  for (const memory of memories) {
    const bucket = byRepository.get(memory.repository)
    if (bucket) {
      bucket.push(memory)
    } else {
      byRepository.set(memory.repository, [memory])
    }
  }

  return [...byRepository]
    .map(([repository, list]) => ({
      key: repository ?? '',
      label: repository === null ? 'Every repository' : projectName(repository),
      memories: [...list].sort((a, b) => b.created_at - a.created_at),
    }))
    // Memories with no repository apply everywhere, so they lead.
    .sort((a, b) => Number(!!a.key) - Number(!!b.key) || a.label.localeCompare(b.label))
}

function MemoriesPage() {
  const { memories } = Route.useLoaderData()
  const groups = group(memories)

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-7xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <div className="flex items-baseline gap-3">
        <h1 className="text-xl font-medium tracking-tight">Memories</h1>
        <span className="text-sm tabular-nums text-muted-foreground">{memories.length}</span>
      </div>

      {groups.length === 0 ? (
        <p className="text-sm text-muted-foreground">No memories yet.</p>
      ) : (
        groups.map((entry) => (
          <section key={entry.key} className="flex flex-col gap-2">
            <h2 className="text-sm font-medium text-muted-foreground">{entry.label}</h2>
            <ul role="list" className="-mx-2 divide-y divide-foreground/5">
              {entry.memories.map((memory) => (
                <li key={memory.id}>
                  <MemoryRow memory={memory} />
                </li>
              ))}
            </ul>
          </section>
        ))
      )}
    </div>
  )
}

function MemoryRow({ memory }: { memory: Memory }) {
  return (
    <div className="flex items-start gap-3 px-2 py-2.5 text-sm">
      <p className="min-w-0 flex-1 text-pretty">{memory.content}</p>

      {/* Approved is the boring default; only a proposal needs saying. */}
      {memory.verification === 'agent_proposed' && (
        <Badge variant="outline" className="border-amber-600/30 text-amber-700 dark:text-amber-400">
          proposed
        </Badge>
      )}

      {memory.source === 'agent' && (
        <span className="shrink-0 text-xs text-muted-foreground">agent</span>
      )}

      <time
        dateTime={new Date(memory.created_at).toISOString()}
        suppressHydrationWarning
        className="w-16 shrink-0 text-end tabular-nums text-muted-foreground"
      >
        {relativeAge(memory.created_at)}
      </time>
    </div>
  )
}

function MemoriesError(props: ErrorComponentProps) {
  return <OrchestratorError what="memories" {...props} />
}
