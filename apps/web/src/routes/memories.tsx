import { useEffect, useRef, useState } from 'react'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import type { ErrorComponentProps } from '@tanstack/react-router'
import { Check, CircleNotch, PencilSimple, Trash } from '@phosphor-icons/react'
import type { Icon } from '@phosphor-icons/react'
import { toast } from 'sonner'

import { projectName } from '@/components/app-sidebar'
import { OrchestratorError } from '@/components/orchestrator-error'
import { relativeAge } from '@/components/task-list'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { approveMemory, deleteMemory, getMemories, updateMemory } from '@/lib/lgtm/server'
import type { Memory } from '@/lib/lgtm/types'
import { cn } from '@/lib/utils'

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

type Action = 'save' | 'delete' | 'approve'

/** Long enough to read "Confirm delete", short enough that a forgotten arm
 *  cannot still be live when the next person reaches the keyboard. */
const DISARM_MS = 4000

function MemoryRow({ memory }: { memory: Memory }) {
  const router = useRouter()
  const [pending, setPending] = useState<Action | null>(null)
  const [armed, setArmed] = useState(false)
  // null means "not editing" — an empty draft is a distinct, valid state.
  const [draft, setDraft] = useState<string | null>(null)
  const deleteRef = useRef<HTMLButtonElement>(null)

  // Arming delete puts the row in a mode, and a mode nobody meant to enter has
  // to expire on its own: a pointer anywhere else, Escape, or the timeout.
  useEffect(() => {
    if (!armed) return

    const disarm = () => setArmed(false)
    const onPointerDown = (event: PointerEvent) => {
      if (!deleteRef.current?.contains(event.target as Node)) disarm()
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') disarm()
    }

    const timer = window.setTimeout(disarm, DISARM_MS)
    document.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      window.clearTimeout(timer)
      document.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [armed])

  const busy = pending !== null
  const proposed = memory.verification === 'agent_proposed'

  async function run(action: Action, call: () => Promise<unknown>, message: string) {
    setPending(action)
    setArmed(false)
    try {
      await call()
      toast.success(message)
      await router.invalidate()
      return true
    } catch (error) {
      // The orchestrator's refusal reason is the whole message; genericising it
      // would throw away the only thing that says what to do next.
      toast.error(error instanceof Error ? error.message : String(error))
      return false
    } finally {
      setPending(null)
    }
  }

  const edited = (draft ?? '').trim()

  async function save() {
    if (!edited || edited === memory.content) return
    const saved = await run(
      'save',
      () => updateMemory({ data: { id: memory.id, content: edited } }),
      // The orchestrator treats an edit as sign-off, so say so.
      proposed ? 'Memory updated and approved' : 'Memory updated',
    )
    if (saved) setDraft(null)
  }

  if (draft !== null) {
    return (
      <div className="flex items-start gap-3 px-2 py-2.5 text-sm">
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <Textarea
            autoFocus
            aria-label="Memory content"
            value={draft}
            disabled={busy}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Escape') setDraft(null)
            }}
          />
          <div className="flex items-center gap-2">
            <Button size="sm" disabled={busy || !edited || edited === memory.content} onClick={save}>
              <ActionIcon icon={Check} busy={pending === 'save'} />
              Save
            </Button>
            <Button size="sm" variant="ghost" disabled={busy} onClick={() => setDraft(null)}>
              Cancel
            </Button>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="group/row flex items-start gap-3 px-2 py-2.5 text-sm">
      <p className="min-w-0 flex-1 text-pretty">{memory.content}</p>

      {/* Always in the flow so revealing them cannot reflow the row. */}
      <div className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-focus-within/row:opacity-100 group-hover/row:opacity-100 pointer-coarse:opacity-100">
        {proposed && (
          <Button
            size="icon-sm"
            variant="ghost"
            className="text-muted-foreground"
            aria-label="Approve memory"
            disabled={busy}
            onClick={() =>
              run('approve', () => approveMemory({ data: memory.id }), 'Memory approved')
            }
          >
            <ActionIcon icon={Check} busy={pending === 'approve'} />
          </Button>
        )}
        <Button
          size="icon-sm"
          variant="ghost"
          className="text-muted-foreground"
          aria-label="Edit memory"
          disabled={busy}
          onClick={() => setDraft(memory.content)}
        >
          <PencilSimple />
        </Button>
        <Button
          ref={deleteRef}
          size={armed ? 'sm' : 'icon-sm'}
          variant={armed ? 'destructive' : 'ghost'}
          className={cn(
            armed
              ? // The variant's own `dark:` classes outrank an unprefixed
                // override, so the armed fill has to be stated for both themes.
                'bg-destructive text-destructive-foreground hover:bg-destructive/90 dark:bg-destructive dark:hover:bg-destructive/90'
              : 'text-muted-foreground',
          )}
          aria-label={armed ? 'Confirm delete memory' : 'Delete memory'}
          disabled={busy}
          onClick={() =>
            armed
              ? run('delete', () => deleteMemory({ data: memory.id }), 'Memory deleted')
              : setArmed(true)
          }
        >
          <ActionIcon icon={Trash} busy={pending === 'delete'} />
          {armed && 'Confirm delete'}
        </Button>
      </div>

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

/** Swapping the icon for the spinner, rather than adding one, keeps the button
 *  the same width while it works. */
function ActionIcon({ icon: Icon, busy }: { icon: Icon; busy: boolean }) {
  if (busy) {
    return <CircleNotch data-icon="inline-start" className="motion-safe:animate-spin" />
  }
  return <Icon data-icon="inline-start" />
}

function MemoriesError(props: ErrorComponentProps) {
  return <OrchestratorError what="memories" {...props} />
}
