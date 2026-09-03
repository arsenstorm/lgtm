import { useState } from 'react'
import { Link, createFileRoute, useNavigate } from '@tanstack/react-router'
import type { ErrorComponentProps } from '@tanstack/react-router'
import { CircleNotch, Notepad, Plus } from '@phosphor-icons/react'
import { toast } from 'sonner'

import { projectName } from '@/components/app-sidebar'
import { OrchestratorError } from '@/components/orchestrator-error'
import { TimeAgo } from '@/components/time-ago'
import { Button } from '@/components/ui/button'
import { createScratchpad, getScratchpads } from '@/lib/lgtm/server'
import type { Scratchpad } from '@/lib/lgtm/types'

export const Route = createFileRoute('/scratchpads')({
  loader: async () => ({ scratchpads: await getScratchpads() }),
  component: ScratchpadsPage,
  errorComponent: ScratchpadsError,
})

const TITLE_MAX = 80

function cut(text: string): string {
  return text.length > TITLE_MAX ? `${text.slice(0, TITLE_MAX)}…` : text
}

/** A scratchpad has no title field: like any markdown file, its first `# `
 *  heading names it, and failing that its first written line does. */
export function padTitle(content: string): string {
  const lines = content.split('\n')
  const heading = lines.find((line) => line.startsWith('# '))
  if (heading) return cut(heading.slice(2).trim())

  const first = lines.find((line) => line.trim() !== '')
  return first ? cut(first.trim()) : 'Untitled'
}

const byEdited = (a: Scratchpad, b: Scratchpad) => b.updated_at - a.updated_at

function ScratchpadsPage() {
  const { scratchpads } = Route.useLoaderData()
  const navigate = useNavigate()
  const [creating, setCreating] = useState(false)

  const live = scratchpads.filter((pad) => !pad.archived).sort(byEdited)
  const archived = scratchpads.filter((pad) => pad.archived).sort(byEdited)

  async function create() {
    setCreating(true)
    try {
      const pad = await createScratchpad({ data: { content: '' } })
      // The blank document opening is the success signal; a toast on top of it
      // would only say what the screen already shows.
      await navigate({ to: '/scratchpads/$id', params: { id: pad.id } })
    } catch (error) {
      // The orchestrator's refusal reason is the whole message; genericising it
      // would throw away the only thing that says what to do next.
      toast.error(error instanceof Error ? error.message : String(error))
    } finally {
      setCreating(false)
    }
  }

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-7xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <div className="flex items-center gap-3">
        <h1 className="text-xl font-medium tracking-tight">Scratchpads</h1>
        <span className="text-sm tabular-nums text-muted-foreground">{live.length}</span>
        <Button size="lg" className="ms-auto" disabled={creating} onClick={create}>
          {creating ? (
            <CircleNotch data-icon="inline-start" className="motion-safe:animate-spin" />
          ) : (
            <Plus data-icon="inline-start" />
          )}
          New scratchpad
        </Button>
      </div>

      {scratchpads.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          No scratchpads yet. New scratchpad starts one.
        </p>
      ) : (
        <>
          <ul role="list" className="-mx-2 divide-y divide-foreground/5">
            {live.map((pad) => (
              <li key={pad.id}>
                <ScratchpadRow pad={pad} />
              </li>
            ))}
          </ul>

          {archived.length > 0 && (
            <section className="flex flex-col gap-2">
              <h2 className="text-sm font-medium text-muted-foreground">Archived</h2>
              <ul role="list" className="-mx-2 divide-y divide-foreground/5">
                {archived.map((pad) => (
                  <li key={pad.id}>
                    <ScratchpadRow pad={pad} />
                  </li>
                ))}
              </ul>
            </section>
          )}
        </>
      )}
    </div>
  )
}

function ScratchpadRow({ pad }: { pad: Scratchpad }) {
  return (
    <Link
      to="/scratchpads/$id"
      params={{ id: pad.id }}
      className="flex items-center gap-3 rounded-md px-2 py-2.5 text-sm hover:bg-foreground/4"
    >
      <Notepad className="size-4 shrink-0 text-muted-foreground" />
      <span className="min-w-0 flex-1 truncate">{padTitle(pad.content)}</span>

      {pad.repository !== null && (
        <span className="w-32 shrink-0 truncate text-muted-foreground">
          {projectName(pad.repository)}
        </span>
      )}

      <TimeAgo
        at={pad.updated_at}
        className="shrink-0 text-end text-muted-foreground tabular-nums"
      />
    </Link>
  )
}

function ScratchpadsError(props: ErrorComponentProps) {
  return <OrchestratorError what="scratchpads" {...props} />
}
