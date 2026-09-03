import { Link } from '@tanstack/react-router'
import {
  ChatCircleText,
  CheckCircle,
  CircleNotch,
  Clock,
  Eye,
  GitBranch,
  GitMerge,
  MinusCircle,
  Plugs,
  Prohibit,
  Timer,
  XCircle,
} from '@phosphor-icons/react'
import type { Icon } from '@phosphor-icons/react'

import type { Task, TaskStatus } from '@/lib/lgtm/types'
import { cn } from '@/lib/utils'

/**
 * Tone groups the twelve statuses into the four things an operator actually
 * does next: wait, act, celebrate, investigate. Colour alone can't carry that,
 * so every status also keeps a distinct glyph and its spelled-out label.
 */
type Tone = 'idle' | 'live' | 'attention' | 'done' | 'broken'

export const STATUS: Record<
  TaskStatus,
  { label: string; icon: Icon; tone: Tone }
> = {
  queued: { label: 'Queued', icon: Clock, tone: 'idle' },
  running: { label: 'Running', icon: CircleNotch, tone: 'live' },
  awaiting_review: { label: 'Awaiting review', icon: Eye, tone: 'attention' },
  changes_requested: { label: 'Changes requested', icon: ChatCircleText, tone: 'attention' },
  conflicted: { label: 'Conflicted', icon: GitBranch, tone: 'attention' },
  approved: { label: 'Approved', icon: CheckCircle, tone: 'done' },
  merged: { label: 'Merged', icon: GitMerge, tone: 'done' },
  rejected: { label: 'Rejected', icon: Prohibit, tone: 'idle' },
  failed: { label: 'Failed', icon: XCircle, tone: 'broken' },
  timed_out: { label: 'Timed out', icon: Timer, tone: 'broken' },
  runner_lost: { label: 'Runner lost', icon: Plugs, tone: 'broken' },
  cancelled: { label: 'Cancelled', icon: MinusCircle, tone: 'idle' },
}

const TONE_TEXT: Record<Tone, string> = {
  idle: 'text-muted-foreground',
  live: 'text-blue-700 dark:text-blue-400',
  attention: 'text-amber-700 dark:text-amber-400',
  done: 'text-emerald-700 dark:text-emerald-400',
  broken: 'text-red-700 dark:text-red-400',
}

const UNITS: [ms: number, suffix: string][] = [
  [86_400_000, 'd'],
  [3_600_000, 'h'],
  [60_000, 'm'],
  [1_000, 's'],
]

/** Coarse duration — "4h", "12d". Used for ages, windows and medians alike. */
export function shortSpan(ms: number): string {
  const abs = Math.max(0, ms)
  for (const [size, suffix] of UNITS) {
    if (abs >= size) return `${Math.floor(abs / size)}${suffix}`
  }
  return '0s'
}

export function relativeAge(atMs: number): string {
  return `${shortSpan(Date.now() - atMs)} ago`
}

function firstLine(prompt: string): string {
  const line = prompt.split('\n', 1)[0]?.trim()
  return line ? line : '(no prompt)'
}

export function TaskList({ tasks }: { tasks: Task[] }) {
  if (tasks.length === 0) {
    return (
      <div className="flex min-h-64 flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-foreground/15 p-8 text-center">
        <h2 className="text-base font-medium">No tasks yet</h2>
        <p className="max-w-[52ch] text-base text-pretty text-muted-foreground sm:text-sm">
          Tasks appear here the moment the orchestrator accepts one. Queue the first from your
          terminal:
        </p>
        <code className="mt-1 text-sm">lgtm task &quot;fix the flaky login test&quot;</code>
      </div>
    )
  }

  return (
    <ul role="list" className="-mx-2 divide-y divide-foreground/5">
      {tasks.map((task) => (
        <TaskRow key={task.id} task={task} />
      ))}
    </ul>
  )
}

function TaskRow({ task }: { task: Task }) {
  const { label, icon: Icon, tone } = STATUS[task.status]
  const needsHuman = tone === 'attention'

  return (
    <li>
      <Link
        to="/tasks/$id"
        params={{ id: task.id }}
        className={cn(
          'relative flex flex-wrap items-center gap-x-3 gap-y-1.5 rounded-md px-2 py-2.5 text-sm hover:bg-foreground/4 sm:flex-nowrap sm:gap-x-4',
          // A bar, not just a hue: rows a human is blocking on stay findable
          // at a glance and in a screenshot printed in greyscale.
          needsHuman &&
            'before:absolute before:inset-y-1 before:start-0 before:w-0.5 before:rounded-full before:bg-amber-500 before:content-[""]',
        )}
      >
        <span
          className={cn(
            'flex w-40 shrink-0 items-center gap-1.5 font-medium whitespace-nowrap',
            TONE_TEXT[tone],
          )}
        >
          <Icon
            className={cn(
              'size-4 h-lh shrink-0',
              task.status === 'running' && 'motion-safe:animate-spin [animation-duration:1.8s]',
            )}
            aria-hidden="true"
          />
          {label}
        </span>

        <span className="w-16 shrink-0 font-mono tabular-nums text-muted-foreground">
          {task.id.slice(0, 8)}
        </span>

        <p className="order-last min-w-0 basis-full truncate text-base text-foreground sm:order-none sm:basis-auto sm:flex-1 sm:text-sm">
          {firstLine(task.spec.prompt)}
        </p>

        <span className="w-24 shrink-0 truncate text-muted-foreground">{task.runner ?? '—'}</span>

        <time
          dateTime={new Date(task.created_at).toISOString()}
          suppressHydrationWarning
          className="grow text-end tabular-nums text-muted-foreground sm:w-16 sm:grow-0"
        >
          {relativeAge(task.created_at)}
        </time>
      </Link>
    </li>
  )
}
