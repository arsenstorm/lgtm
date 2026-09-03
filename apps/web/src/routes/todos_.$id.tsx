import {
  ArrowDownLeft,
  ArrowUpRight,
  CaretDown,
  PencilSimple,
} from "@phosphor-icons/react";
import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute, Link, useRouter } from "@tanstack/react-router";
import type { FormEvent, ReactNode } from "react";
import { useCallback, useRef, useState } from "react";
import { toast } from "sonner";

import { projectName } from "@/components/app-sidebar";
import { MarkdownEditor } from "@/components/markdown-editor";
import { OrchestratorError } from "@/components/orchestrator-error";
import { PriorityIcon } from "@/components/priority-icon";
import { TimeAgo } from "@/components/time-ago";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  commentOnTodo,
  getTodo,
  getTodos,
  updateTodo,
} from "@/lib/lgtm/server";
import type {
  Todo,
  TodoComment,
  TodoPriority,
  TodoStatus,
} from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";
import { TagsRow } from "@/routes/scratchpads";
import { MARK } from "@/routes/todos";

export const Route = createFileRoute("/todos_/$id")({
  loader: async ({ params }) => {
    const [detail, all] = await Promise.all([
      getTodo({ data: params.id }),
      getTodos(),
    ]);
    // "Blocking" is the reverse edge of `blockers`, and the API has no index for
    // it, so the list is joined here — cheap at the sizes a todo list reaches.
    return {
      ...detail,
      blocking: all.filter((other) => other.blockers.includes(params.id))
        .length,
    };
  },
  component: TodoDetailPage,
  errorComponent: TodoDetailError,
});

const STATUS_OPTIONS: TodoStatus[] = ["open", "in_progress", "done"];
const PRIORITY_OPTIONS: TodoPriority[] = ["low", "medium", "high"];

const PRIORITY: Record<TodoPriority, { className: string; label: string }> = {
  high: {
    className: "border-red-600/30 text-red-700 dark:text-red-400",
    label: "High priority",
  },
  medium: {
    className: "border-amber-600/30 text-amber-700 dark:text-amber-400",
    label: "Medium priority",
  },
  low: {
    className: "border-border text-muted-foreground",
    label: "Low priority",
  },
};

const CHIP =
  "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs whitespace-nowrap [&_svg]:size-3.5 [&_svg]:shrink-0";

type SaveState = "idle" | "saving" | "saved";

function TodoDetailPage() {
  const { todo, comments, blocking } = Route.useLoaderData();
  const router = useRouter();
  const [pending, setPending] = useState(false);

  async function run(call: () => Promise<unknown>, message: string) {
    setPending(true);
    try {
      await call();
      toast.success(message);
      await router.invalidate();
      return true;
    } catch (error) {
      // The orchestrator's refusal reason is the whole message; genericising it
      // would throw away the only thing that says what to do next.
      toast.error(error instanceof Error ? error.message : String(error));
      return false;
    } finally {
      setPending(false);
    }
  }

  const patch = (
    fields: Parameters<typeof updateTodo>[0]["data"]["patch"],
    message: string
  ) => run(() => updateTodo({ data: { id: todo.id, patch: fields } }), message);

  const done = todo.status === "done";

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-4xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <header className="flex flex-col gap-4">
        <div className="flex min-w-0 flex-col gap-1">
          <span className="font-mono text-muted-foreground text-xs">
            {todo.display_id}
          </span>

          <EditableTitle
            onSave={(title) => patch({ title }, "Title updated")}
            pending={pending}
            value={todo.title}
          >
            <h1
              className={cn(
                "min-w-0 flex-1 text-pretty font-semibold text-2xl tracking-tight",
                done && "text-muted-foreground line-through"
              )}
            >
              {todo.title}
            </h1>
          </EditableTitle>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <StatusChip
            disabled={pending}
            onPick={(status) =>
              patch({ status }, `Marked ${MARK[status].label.toLowerCase()}`)
            }
            value={todo.status}
          />
          <PriorityChip
            disabled={pending}
            onPick={(priority) =>
              patch({ priority }, `Priority set to ${priority}`)
            }
            value={todo.priority}
          />
          {todo.blockers.length > 0 ? (
            <span className={cn(CHIP, "border-border text-muted-foreground")}>
              <ArrowDownLeft />
              Blocked by{" "}
              <span className="tabular-nums">{todo.blockers.length}</span>
            </span>
          ) : null}
          {blocking > 0 ? (
            <span className={cn(CHIP, "border-border text-muted-foreground")}>
              <ArrowUpRight />
              Blocking <span className="tabular-nums">{blocking}</span>
            </span>
          ) : null}
        </div>

        <TagsRow
          disabled={pending}
          onChange={(tags, message) => patch({ tags }, message)}
          tags={todo.tags}
        />

        <Meta todo={todo} />
      </header>

      <Description todo={todo} />

      <Activity
        comments={comments}
        createdAt={todo.created_at}
        onSend={(body) =>
          run(
            () => commentOnTodo({ data: { id: todo.id, body } }),
            "Comment added"
          )
        }
        pending={pending}
      />
    </div>
  );
}

function Meta({ todo }: { todo: Todo }) {
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-muted-foreground text-xs">
      <span className="truncate">
        {todo.repository ? projectName(todo.repository) : "Every repository"}
      </span>
      <span aria-hidden="true">·</span>
      <TimeAgo at={todo.created_at} />
      {todo.assignee ? (
        <>
          <span aria-hidden="true">·</span>
          <span className="truncate font-mono">{todo.assignee}</span>
        </>
      ) : null}
      {todo.task ? (
        <>
          <span aria-hidden="true">·</span>
          <Link
            className="truncate font-mono underline-offset-4 hover:underline"
            params={{ id: todo.task }}
            to="/tasks/$id"
          >
            {todo.task}
          </Link>
        </>
      ) : null}
    </div>
  );
}

function StatusChip({
  value,
  disabled,
  onPick,
}: {
  disabled: boolean;
  onPick: (value: TodoStatus) => void;
  value: TodoStatus;
}) {
  const { icon: Mark, label, className } = MARK[value];

  return (
    <Picker
      disabled={disabled}
      format={(status) => MARK[status].label}
      onPick={onPick}
      options={STATUS_OPTIONS}
      triggerClassName={cn("border-border", className)}
      value={value}
    >
      <Mark weight={value === "done" ? "fill" : "regular"} />
      {label}
    </Picker>
  );
}

function PriorityChip({
  value,
  disabled,
  onPick,
}: {
  disabled: boolean;
  onPick: (value: TodoPriority) => void;
  value: TodoPriority;
}) {
  const { className, label } = PRIORITY[value];

  return (
    <Picker
      disabled={disabled}
      format={(priority) => PRIORITY[priority].label}
      onPick={onPick}
      options={PRIORITY_OPTIONS}
      triggerClassName={className}
      value={value}
    >
      <PriorityIcon className="size-3.5" priority={value} />
      {label}
    </Picker>
  );
}

function Picker<T extends string>({
  value,
  options,
  format,
  disabled,
  onPick,
  triggerClassName,
  children,
}: {
  children: ReactNode;
  disabled: boolean;
  format: (value: T) => string;
  onPick: (value: T) => void;
  options: T[];
  triggerClassName: string;
  value: T;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        aria-label={format(value)}
        className={cn(
          CHIP,
          "transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50",
          triggerClassName
        )}
        disabled={disabled}
      >
        {children}
        <CaretDown className="text-muted-foreground" />
      </DropdownMenuTrigger>
      <DropdownMenuContent className="min-w-40">
        <DropdownMenuRadioGroup
          onValueChange={(next) => {
            if (next !== value) {
              onPick(next as T);
            }
          }}
          value={value}
        >
          {options.map((option) => (
            <DropdownMenuRadioItem key={option} value={option}>
              {format(option)}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function Description({ todo }: { todo: Todo }) {
  const [state, setState] = useState<SaveState>("idle");
  const latest = useRef(todo.description);
  const inFlight = useRef<Promise<void> | null>(null);

  const save = useCallback(
    (markdown: string) => {
      latest.current = markdown;
      // Latest-wins: an edit that lands mid-request rides the running save's
      // tail instead of racing it, so the server never takes an older draft
      // after a newer one.
      // biome-ignore lint/suspicious/noUnnecessaryConditions: the ref is mutated at runtime, which biome's inference does not see
      if (inFlight.current) {
        return;
      }
      setState("saving");

      const send = async (md: string): Promise<void> => {
        await updateTodo({ data: { id: todo.id, patch: { description: md } } });
        if (latest.current !== md) {
          await send(latest.current);
        }
      };

      inFlight.current = send(markdown)
        .then(() => setState("saved"))
        .catch((error: unknown) => {
          setState("idle");
          toast.error(error instanceof Error ? error.message : String(error));
        })
        .finally(() => {
          inFlight.current = null;
        });
    },
    [todo.id]
  );

  return (
    <section className="flex min-w-0 flex-col gap-2">
      <div className="flex items-center gap-3">
        <h2 className="font-medium text-muted-foreground text-sm">
          Description
        </h2>
        {/* Autosave without a Save button still has to say it happened. */}
        {state === "idle" ? null : (
          <span className="text-muted-foreground text-xs">
            {state === "saving" ? "Saving…" : "Saved"}
          </span>
        )}
      </div>
      <MarkdownEditor
        className="min-w-0"
        key={todo.id}
        onMarkdown={save}
        placeholder="Add a description…"
        value={todo.description}
      />
    </section>
  );
}

/** `draft === null` is the read mode, so opening the editor and seeding it from
 *  the current value cannot drift apart. */
function EditableTitle({
  value,
  pending,
  onSave,
  children,
}: {
  children: ReactNode;
  onSave: (next: string) => Promise<boolean>;
  pending: boolean;
  value: string;
}) {
  const [draft, setDraft] = useState<string | null>(null);

  if (draft === null) {
    return (
      <div className="flex min-w-0 items-start gap-2">
        {children}
        <Button
          aria-label="Edit title"
          className="mt-1 shrink-0"
          onClick={() => setDraft(value)}
          size="icon-sm"
          variant="ghost"
        >
          <PencilSimple />
        </Button>
      </div>
    );
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const next = draft ?? "";
    if (pending || next.trim() === "") {
      return;
    }
    if (await onSave(next)) {
      setDraft(null);
    }
  }

  return (
    <form className="flex min-w-0 flex-col gap-2" onSubmit={submit}>
      <Input
        aria-label="title"
        autoFocus
        disabled={pending}
        onChange={(event) => setDraft(event.target.value)}
        value={draft}
      />
      <div className="flex gap-2">
        <Button
          disabled={pending || draft.trim() === ""}
          size="sm"
          type="submit"
        >
          Save
        </Button>
        <Button
          onClick={() => setDraft(null)}
          size="sm"
          type="button"
          variant="ghost"
        >
          Cancel
        </Button>
      </div>
    </form>
  );
}

function Activity({
  comments,
  createdAt,
  pending,
  onSend,
}: {
  comments: TodoComment[];
  createdAt: number;
  onSend: (body: string) => Promise<boolean>;
  pending: boolean;
}) {
  const [body, setBody] = useState("");

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const text = body.trim();
    if (!text || pending) {
      return;
    }
    if (await onSend(text)) {
      setBody("");
    }
  }

  return (
    <section className="flex min-w-0 flex-col gap-4">
      <h2 className="font-medium text-muted-foreground text-sm">Activity</h2>

      <ol className="flex min-w-0 flex-col gap-4">
        {/* Creation is the only recorded event: the orchestrator keeps no
            history of edits yet. */}
        <li className="flex items-center gap-2 text-sm">
          <span
            aria-hidden="true"
            className="size-1.5 shrink-0 rounded-full bg-border"
          />
          <span className="text-muted-foreground">Created todo</span>
          <TimeAgo at={createdAt} className="text-muted-foreground text-xs" />
        </li>

        {comments.map((comment) => (
          <li className="flex min-w-0 gap-2" key={comment.id}>
            <Avatar size="sm">
              <AvatarFallback>{authorInitials(comment.author)}</AvatarFallback>
            </Avatar>
            <div className="flex min-w-0 flex-col gap-1">
              <div className="flex flex-wrap items-baseline gap-2">
                <span
                  className={cn(
                    "font-medium text-xs",
                    comment.author && "font-mono [overflow-wrap:anywhere]"
                  )}
                >
                  {comment.author ?? "automation"}
                </span>
                <TimeAgo
                  at={comment.created_at}
                  className="text-muted-foreground text-xs"
                />
              </div>
              <p className="whitespace-pre-wrap text-sm [overflow-wrap:anywhere]">
                {comment.body}
              </p>
            </div>
          </li>
        ))}
      </ol>

      <form className="flex min-w-0 gap-2" onSubmit={submit}>
        <Avatar size="sm">
          <AvatarFallback>AS</AvatarFallback>
        </Avatar>
        <div className="flex min-w-0 flex-1 flex-col items-start gap-2">
          <Textarea
            aria-label="New comment"
            disabled={pending}
            onChange={(event) => setBody(event.target.value)}
            placeholder="Leave a comment…"
            value={body}
          />
          <Button
            disabled={pending || body.trim() === ""}
            size="sm"
            type="submit"
          >
            Comment
          </Button>
        </div>
      </form>
    </section>
  );
}

const authorInitials = (author: string | null) =>
  author ? author.slice(0, 2).toUpperCase() : "A";

function TodoDetailError(props: ErrorComponentProps) {
  return <OrchestratorError what="this todo" {...props} />;
}
