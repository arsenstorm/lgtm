import { useState } from 'react'
import type { FormEvent, ReactNode } from 'react'
import { Link, createFileRoute, useRouter } from '@tanstack/react-router'
import type { ErrorComponentProps } from '@tanstack/react-router'
import { CaretDown, PencilSimple } from '@phosphor-icons/react'
import { toast } from 'sonner'

import { projectName } from '@/components/app-sidebar'
import { OrchestratorError } from '@/components/orchestrator-error'
import { relativeAge } from '@/components/task-list'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { MARK } from '@/routes/todos'
import { commentOnTodo, getTodo, updateTodo } from '@/lib/lgtm/server'
import type { Todo, TodoComment, TodoPriority, TodoStatus } from '@/lib/lgtm/types'
import { cn } from '@/lib/utils'

export const Route = createFileRoute('/todos_/$id')({
  loader: ({ params }) => getTodo({ data: params.id }),
  component: TodoDetailPage,
  errorComponent: TodoDetailError,
})

const STATUS_OPTIONS: TodoStatus[] = ['open', 'in_progress', 'done']
const PRIORITY_OPTIONS: TodoPriority[] = ['low', 'medium', 'high']

function TodoDetailPage() {
  const { todo, comments } = Route.useLoaderData()
  const router = useRouter()
  const [pending, setPending] = useState(false)

  async function run(call: () => Promise<unknown>, message: string) {
    setPending(true)
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
      setPending(false)
    }
  }

  const patch = (patch: Parameters<typeof updateTodo>[0]['data']['patch'], message: string) =>
    run(() => updateTodo({ data: { id: todo.id, patch } }), message)

  const { icon: Mark, label, className } = MARK[todo.status]

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-7xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <header className="flex flex-col gap-5">
        <div className="flex items-start gap-3">
          <Mark
            aria-label={label}
            role="img"
            className={cn('size-5 h-lh shrink-0', className)}
            weight={todo.status === 'done' ? 'fill' : 'regular'}
          />
          <Editable
            what="title"
            value={todo.title}
            pending={pending}
            onSave={(title) => patch({ title }, 'Title updated')}
          >
            <h1 className="min-w-0 flex-1 text-xl font-medium tracking-tight text-pretty">
              {todo.title}
            </h1>
          </Editable>
        </div>

        <Meta todo={todo} />
      </header>

      <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
        <Picker
          label="Status"
          value={todo.status}
          options={STATUS_OPTIONS}
          format={(status) => MARK[status].label}
          disabled={pending}
          onPick={(status) =>
            patch({ status }, `Marked ${MARK[status].label.toLowerCase()}`)
          }
        />
        <Picker
          label="Priority"
          value={todo.priority}
          options={PRIORITY_OPTIONS}
          format={(priority) => priority}
          disabled={pending}
          onPick={(priority) => patch({ priority }, `Priority set to ${priority}`)}
        />
      </div>

      <section className="flex flex-col gap-2">
        <h2 className="text-sm font-medium text-muted-foreground">Description</h2>
        <Editable
          what="description"
          value={todo.description}
          multiline
          allowEmpty
          pending={pending}
          onSave={(description) => patch({ description }, 'Description updated')}
        >
          <p className="min-w-0 flex-1 text-sm whitespace-pre-wrap text-pretty">
            {todo.description || <span className="text-muted-foreground">No description.</span>}
          </p>
        </Editable>
      </section>

      <Comments
        comments={comments}
        pending={pending}
        onSend={(body) => run(() => commentOnTodo({ data: { id: todo.id, body } }), 'Comment added')}
      />
    </div>
  )
}

function Meta({ todo }: { todo: Todo }) {
  return (
    <dl className="grid grid-cols-2 gap-x-6 gap-y-4 border-t pt-5 sm:grid-cols-3 lg:grid-cols-4">
      <Fact term="Repository">
        {todo.repository ? projectName(todo.repository) : 'Every repository'}
      </Fact>
      <Fact term="Created">
        <time dateTime={new Date(todo.created_at).toISOString()} suppressHydrationWarning>
          {relativeAge(todo.created_at)}
        </time>
      </Fact>
      {todo.assignee ? (
        <Fact term="Assignee">
          <span className="font-mono text-xs [overflow-wrap:anywhere]">{todo.assignee}</span>
        </Fact>
      ) : null}
      {todo.task ? (
        <Fact term="Task">
          <Link
            to="/tasks/$id"
            params={{ id: todo.task }}
            className="font-mono text-xs underline-offset-4 [overflow-wrap:anywhere] hover:underline"
          >
            {todo.task}
          </Link>
        </Fact>
      ) : null}
      {todo.blockers.length > 0 ? (
        <Fact term="Blocked by">
          <span className="tabular-nums">{todo.blockers.length}</span>
        </Fact>
      ) : null}
    </dl>
  )
}

function Fact({ term, children }: { term: string; children: ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <dt className="text-xs text-muted-foreground">{term}</dt>
      <dd className="text-sm font-medium">{children}</dd>
    </div>
  )
}

function Picker<T extends string>({
  label,
  value,
  options,
  format,
  disabled,
  onPick,
}: {
  label: string
  value: T
  options: T[]
  format: (value: T) => string
  disabled: boolean
  onPick: (value: T) => void
}) {
  return (
    <div className="flex items-center gap-2">
      <span className="text-xs text-muted-foreground">{label}</span>
      <DropdownMenu>
        <DropdownMenuTrigger
          disabled={disabled}
          render={<Button size="sm" variant="outline" aria-label={label} />}
        >
          {format(value)}
          <CaretDown data-icon="inline-end" />
        </DropdownMenuTrigger>
        <DropdownMenuContent className="min-w-36">
          <DropdownMenuRadioGroup
            value={value}
            onValueChange={(next) => {
              if (next !== value) onPick(next as T)
            }}
          >
            {options.map((option) => (
              <DropdownMenuRadioItem key={option} value={option}>
                {format(option)}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  )
}

/** `draft === null` is the read mode, so opening the editor and seeding it from
 *  the current value cannot drift apart. */
function Editable({
  what,
  value,
  multiline,
  allowEmpty,
  pending,
  onSave,
  children,
}: {
  what: string
  value: string
  multiline?: boolean
  allowEmpty?: boolean
  pending: boolean
  onSave: (next: string) => Promise<boolean>
  children: ReactNode
}) {
  const [draft, setDraft] = useState<string | null>(null)

  if (draft === null) {
    return (
      <div className="flex min-w-0 flex-1 items-start gap-2">
        {children}
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={`Edit ${what}`}
          className="shrink-0"
          onClick={() => setDraft(value)}
        >
          <PencilSimple />
        </Button>
      </div>
    )
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const next = draft ?? ''
    if (pending || (!allowEmpty && next.trim() === '')) return
    if (await onSave(next)) setDraft(null)
  }

  return (
    <form className="flex min-w-0 flex-1 flex-col gap-2" onSubmit={submit}>
      {multiline ? (
        <Textarea
          autoFocus
          aria-label={what}
          value={draft}
          disabled={pending}
          onChange={(event) => setDraft(event.target.value)}
        />
      ) : (
        <Input
          autoFocus
          aria-label={what}
          value={draft}
          disabled={pending}
          onChange={(event) => setDraft(event.target.value)}
        />
      )}
      <div className="flex gap-2">
        <Button
          type="submit"
          size="sm"
          disabled={pending || (!allowEmpty && draft.trim() === '')}
        >
          Save
        </Button>
        <Button type="button" size="sm" variant="ghost" onClick={() => setDraft(null)}>
          Cancel
        </Button>
      </div>
    </form>
  )
}

function Comments({
  comments,
  pending,
  onSend,
}: {
  comments: TodoComment[]
  pending: boolean
  onSend: (body: string) => Promise<boolean>
}) {
  const [body, setBody] = useState('')

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const text = body.trim()
    if (!text || pending) return
    if (await onSend(text)) setBody('')
  }

  return (
    <section className="flex flex-col gap-4">
      <h2 className="text-sm font-medium text-muted-foreground">Comments</h2>

      {comments.length === 0 ? (
        <p className="text-sm text-muted-foreground">No comments yet.</p>
      ) : (
        <ul role="list" className="flex flex-col gap-4">
          {comments.map((comment) => (
            <li key={comment.id} className="flex min-w-0 flex-col gap-1">
              <div className="flex flex-wrap items-baseline gap-2">
                {comment.author ? (
                  <span className="font-mono text-xs [overflow-wrap:anywhere]">
                    {comment.author}
                  </span>
                ) : (
                  <span className="text-xs font-medium">automation</span>
                )}
                <time
                  dateTime={new Date(comment.created_at).toISOString()}
                  suppressHydrationWarning
                  className="text-xs tabular-nums text-muted-foreground"
                >
                  {relativeAge(comment.created_at)}
                </time>
              </div>
              <p className="text-sm whitespace-pre-wrap [overflow-wrap:anywhere]">{comment.body}</p>
            </li>
          ))}
        </ul>
      )}

      <form className="flex max-w-2xl flex-col items-start gap-2" onSubmit={submit}>
        <Textarea
          aria-label="New comment"
          placeholder="Leave a comment…"
          value={body}
          disabled={pending}
          onChange={(event) => setBody(event.target.value)}
        />
        <Button type="submit" size="sm" disabled={pending || body.trim() === ''}>
          Comment
        </Button>
      </form>
    </section>
  )
}

function TodoDetailError(props: ErrorComponentProps) {
  return <OrchestratorError what="this todo" {...props} />
}
