import { useEffect, useRef, useState } from 'react'
import type { FormEvent, ReactNode } from 'react'
import { useRouter } from '@tanstack/react-router'
import {
  RiCheckLine,
  RiDeleteBinLine,
  RiLoader4Line,
  RiRestartLine,
  RiSendPlaneLine,
} from '@remixicon/react'
import type { RemixiconComponentType } from '@remixicon/react'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { approveTask, rejectTask, retryTask, sendFollowUp } from '@/lib/lgtm/server'
import type { Task, TaskStatus } from '@/lib/lgtm/types'
import { cn } from '@/lib/utils'

type Action = 'approve' | 'reject' | 'retry' | 'follow-up'

const REVIEWABLE: TaskStatus[] = ['awaiting_review', 'conflicted']
const RETRYABLE: TaskStatus[] = ['failed', 'timed_out', 'runner_lost', 'cancelled']

/** Long enough to read "Confirm reject", short enough that a forgotten arm
 *  cannot still be live when the next person reaches the keyboard. */
const DISARM_MS = 4000

export function TaskActions({ task }: { task: Task }) {
  const router = useRouter()
  const [pending, setPending] = useState<Action | null>(null)
  const [armed, setArmed] = useState(false)
  const [followUp, setFollowUp] = useState('')
  const rejectRef = useRef<HTMLButtonElement>(null)

  // Arming reject puts the page in a mode, and a mode nobody meant to enter has
  // to expire on its own: a pointer anywhere else, Escape, or the timeout.
  useEffect(() => {
    if (!armed) return

    const disarm = () => setArmed(false)
    const onPointerDown = (event: PointerEvent) => {
      if (!rejectRef.current?.contains(event.target as Node)) disarm()
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

  async function run(action: Action, call: () => Promise<Task>, message: string) {
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

  if (RETRYABLE.includes(task.status)) {
    return (
      <Panel>
        <div className="flex flex-wrap items-center gap-3">
          <Button
            size="lg"
            className="relative"
            disabled={busy}
            onClick={() => run('retry', () => retryTask({ data: task.id }), 'Task requeued')}
          >
            <ActionIcon icon={RiRestartLine} busy={pending === 'retry'} />
            Retry
            <TouchTarget />
          </Button>
        </div>
        <Hint>Queues the task again on the same runner and executor, as a new paid run.</Hint>
      </Panel>
    )
  }

  if (!REVIEWABLE.includes(task.status)) return null

  // On a conflict the agent, not the reviewer, is the one who can move the task
  // forward — so the follow-up leads and approve steps back to a quiet button.
  const conflicted = task.status === 'conflicted'

  async function submitFollowUp(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const text = followUp.trim()
    if (!text || busy) return
    const sent = await run(
      'follow-up',
      () => sendFollowUp({ data: { id: task.id, text } }),
      'Follow-up sent',
    )
    if (sent) setFollowUp('')
  }

  const decide = (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-3">
        <Button
          size="lg"
          variant={conflicted ? 'outline' : 'default'}
          className="relative"
          disabled={busy}
          onClick={() =>
            run('approve', () => approveTask({ data: task.id }), 'Task approved — branch pushed')
          }
        >
          <ActionIcon icon={RiCheckLine} busy={pending === 'approve'} />
          Approve
          <TouchTarget />
        </Button>
        <Button
          ref={rejectRef}
          size="lg"
          variant="destructive"
          className={cn(
            'relative',
            // The variant's own `dark:` classes outrank an unprefixed override,
            // so the armed fill has to be stated for both themes.
            armed &&
              'bg-destructive text-destructive-foreground hover:bg-destructive/90 dark:bg-destructive dark:hover:bg-destructive/90',
          )}
          disabled={busy}
          onClick={() =>
            armed
              ? run('reject', () => rejectTask({ data: task.id }), 'Task rejected — worktree discarded')
              : setArmed(true)
          }
        >
          <ActionIcon icon={RiDeleteBinLine} busy={pending === 'reject'} />
          {armed ? 'Confirm reject' : 'Reject'}
          <TouchTarget />
        </Button>
      </div>
      <Hint live>
        {/* Both strings are kept under one line at 60ch so arming the button
            does not reflow the form below it. */}
        {armed
          ? 'Deletes the worktree and branch. This cannot be undone.'
          : 'Approve pushes the branch. Reject discards the work.'}
      </Hint>
    </div>
  )

  const respond = (
    <form className="flex flex-col gap-2" onSubmit={submitFollowUp}>
      <div className="flex gap-3">
        <Input
          name="follow-up"
          aria-label="Follow-up instructions for the agent"
          placeholder={
            conflicted ? 'Tell the agent how to resolve the conflict…' : 'Ask for a change…'
          }
          className="h-9 max-w-md"
          value={followUp}
          disabled={busy}
          onChange={(event) => setFollowUp(event.target.value)}
        />
        <Button
          type="submit"
          size="lg"
          variant={conflicted ? 'default' : 'outline'}
          className="relative"
          disabled={busy || followUp.trim() === ''}
        >
          <ActionIcon icon={RiSendPlaneLine} busy={pending === 'follow-up'} />
          Send follow-up
          <TouchTarget />
        </Button>
      </div>
      <Hint>Resumes the agent with these instructions, as a new paid run.</Hint>
    </form>
  )

  return (
    <Panel>
      {conflicted ? respond : decide}
      <div className="border-t" />
      {conflicted ? decide : respond}
    </Panel>
  )
}

function Panel({ children }: { children: ReactNode }) {
  return (
    <section
      aria-label="Task actions"
      className="flex flex-col gap-4 rounded-lg border bg-muted/30 p-4"
    >
      {children}
    </section>
  )
}

function Hint({ children, live }: { children: ReactNode; live?: boolean }) {
  return (
    <p
      aria-live={live ? 'polite' : undefined}
      className="max-w-[60ch] text-xs text-muted-foreground text-pretty"
    >
      {children}
    </p>
  )
}

/** Swapping the leading icon for the spinner, rather than adding one, keeps the
 *  button the same width while it works. */
function ActionIcon({ icon: Icon, busy }: { icon: RemixiconComponentType; busy: boolean }) {
  if (busy) {
    return <RiLoader4Line data-icon="inline-start" className="motion-safe:animate-spin" />
  }
  return <Icon data-icon="inline-start" />
}

/** A 36px control is under the touch minimum; this grows the tap area on coarse
 *  pointers only. Sized to the row gap so neighbouring targets never overlap. */
function TouchTarget() {
  return (
    <span
      aria-hidden="true"
      className="absolute top-1/2 left-1/2 size-[max(100%,3rem)] -translate-1/2 pointer-fine:hidden"
    />
  )
}
