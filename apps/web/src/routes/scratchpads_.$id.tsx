import { useEffect, useRef, useState } from 'react'
import { createFileRoute, useNavigate, useRouter } from '@tanstack/react-router'
import type { ErrorComponentProps } from '@tanstack/react-router'
import {
  Archive,
  ArrowCounterClockwise,
  Check,
  CircleNotch,
  PencilSimple,
  Trash,
} from '@phosphor-icons/react'
import type { Icon } from '@phosphor-icons/react'
import { toast } from 'sonner'

import { projectName } from '@/components/app-sidebar'
import { OrchestratorError } from '@/components/orchestrator-error'
import { relativeAge } from '@/components/task-list'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { padTitle } from '@/routes/scratchpads'
import { deleteScratchpad, getScratchpad, updateScratchpad } from '@/lib/lgtm/server'
import { cn } from '@/lib/utils'

export const Route = createFileRoute('/scratchpads_/$id')({
  loader: ({ params }) => getScratchpad({ data: params.id }),
  component: ScratchpadPage,
  errorComponent: ScratchpadError,
})

type Action = 'save' | 'archive' | 'delete'

/** Long enough to read "Confirm delete", short enough that a forgotten arm
 *  cannot still be live when the next person reaches the keyboard. */
const DISARM_MS = 4000

function ScratchpadPage() {
  const pad = Route.useLoaderData()
  const router = useRouter()
  const navigate = useNavigate()
  const [pending, setPending] = useState<Action | null>(null)
  const [armed, setArmed] = useState(false)
  // null means "not editing" — an empty draft is a distinct, valid state.
  const [draft, setDraft] = useState<string | null>(null)
  const deleteRef = useRef<HTMLButtonElement>(null)

  // Arming delete puts the page in a mode, and a mode nobody meant to enter has
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
  const title = padTitle(pad.content)

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

  async function save() {
    if (draft === null || draft === pad.content) return
    const saved = await run(
      'save',
      () => updateScratchpad({ data: { id: pad.id, content: draft } }),
      'Scratchpad saved',
    )
    if (saved) setDraft(null)
  }

  async function remove() {
    const deleted = await run(
      'delete',
      () => deleteScratchpad({ data: pad.id }),
      'Scratchpad deleted',
    )
    if (deleted) await navigate({ to: '/scratchpads' })
  }

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <article className="mx-auto flex w-full max-w-7xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <header className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-3">
          <h1 className="min-w-0 text-xl font-medium tracking-tight">{title}</h1>
          {pad.archived && <Badge variant="outline">archived</Badge>}
        </div>

        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-muted-foreground">
          {pad.repository !== null && <span>{projectName(pad.repository)}</span>}
          <time dateTime={new Date(pad.created_at).toISOString()} suppressHydrationWarning>
            created {relativeAge(pad.created_at)}
          </time>
          {pad.updated_at !== pad.created_at && (
            <time dateTime={new Date(pad.updated_at).toISOString()} suppressHydrationWarning>
              edited {relativeAge(pad.updated_at)}
            </time>
          )}
        </div>
      </header>

      {draft === null ? (
        <>
          <div className="flex flex-wrap items-center gap-2">
            <Button
              size="lg"
              variant="outline"
              disabled={busy}
              onClick={() => setDraft(pad.content)}
            >
              <PencilSimple data-icon="inline-start" />
              Edit
            </Button>
            <Button
              size="lg"
              variant="outline"
              disabled={busy}
              onClick={() =>
                run(
                  'archive',
                  () => updateScratchpad({ data: { id: pad.id, archived: !pad.archived } }),
                  pad.archived ? 'Scratchpad restored' : 'Scratchpad archived',
                )
              }
            >
              <ActionIcon
                icon={pad.archived ? ArrowCounterClockwise : Archive}
                busy={pending === 'archive'}
              />
              {pad.archived ? 'Unarchive' : 'Archive'}
            </Button>
            <Button
              ref={deleteRef}
              size="lg"
              variant="destructive"
              className={cn(
                // The variant's own `dark:` classes outrank an unprefixed
                // override, so the armed fill has to be stated for both themes.
                armed &&
                  'bg-destructive text-destructive-foreground hover:bg-destructive/90 dark:bg-destructive dark:hover:bg-destructive/90',
              )}
              disabled={busy}
              onClick={() => (armed ? remove() : setArmed(true))}
            >
              <ActionIcon icon={Trash} busy={pending === 'delete'} />
              {armed ? 'Confirm delete' : 'Delete'}
            </Button>
          </div>

          <div className="rounded-lg border bg-muted/30 p-4">
            {/* The raw markdown is what the agents read and write; rendering it
                is a decision to take once the pads are worth reading as pages.
                Wrapping keeps a long line inside the container, not the page. */}
            <pre className="font-mono text-sm leading-relaxed whitespace-pre-wrap [overflow-wrap:anywhere]">
              {pad.content === '' ? (
                <span className="text-muted-foreground">Empty. Edit to write something.</span>
              ) : (
                pad.content
              )}
            </pre>
          </div>
        </>
      ) : (
        <div className="flex flex-col gap-3">
          <Textarea
            autoFocus
            aria-label="Scratchpad content"
            className="min-h-96 font-mono"
            value={draft}
            disabled={busy}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Escape') setDraft(null)
              // A document editor without a keyboard save invites losing the
              // habit halfway through a thought.
              if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) save()
            }}
          />
          <div className="flex items-center gap-2">
            <Button size="lg" disabled={busy || draft === pad.content} onClick={save}>
              <ActionIcon icon={Check} busy={pending === 'save'} />
              Save
            </Button>
            <Button size="lg" variant="ghost" disabled={busy} onClick={() => setDraft(null)}>
              Cancel
            </Button>
          </div>
        </div>
      )}
    </article>
  )
}

/** Swapping the leading icon for the spinner, rather than adding one, keeps the
 *  button the same width while it works. */
function ActionIcon({ icon: Icon, busy }: { icon: Icon; busy: boolean }) {
  if (busy) {
    return <CircleNotch data-icon="inline-start" className="motion-safe:animate-spin" />
  }
  return <Icon data-icon="inline-start" />
}

function ScratchpadError(props: ErrorComponentProps) {
  return <OrchestratorError what="this scratchpad" {...props} />
}
