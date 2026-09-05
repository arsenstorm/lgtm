import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute, Link } from "@tanstack/react-router";
import type { FormEvent, ReactNode } from "react";
import { useCallback, useRef, useState } from "react";
import { toast } from "sonner";

import { projectName } from "@/components/app-sidebar";
import {
  ArrowDownLeftIcon,
  ArrowUpRightIcon,
  PencilIcon,
} from "@/components/icons";
import { MarkdownEditor } from "@/components/markdown-editor";
import { OrchestratorError } from "@/components/orchestrator-error";
import { TagsRow } from "@/components/tags-row";
import { TimeAgo } from "@/components/time-ago";
import { TodoActivity } from "@/components/todo-activity";
import { CHIP, MARK, PriorityChip, StatusChip } from "@/components/todo-chips";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useAction } from "@/hooks/use-action";
import {
  commentOnTodo,
  getTodo,
  getTodos,
  updateTodo,
} from "@/lib/lgtm/server";
import type { Todo } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

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

type SaveState = "idle" | "saving" | "saved";

function TodoDetailPage() {
  const { todo, comments, blocking } = Route.useLoaderData();
  const { busy: pending, run } = useAction();

  const patch = (
    fields: Parameters<typeof updateTodo>[0]["data"]["patch"],
    message: string
  ) =>
    run(
      "patch",
      () => updateTodo({ data: { id: todo.id, patch: fields } }),
      message
    );

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
              <ArrowDownLeftIcon />
              Blocked by{" "}
              <span className="tabular-nums">{todo.blockers.length}</span>
            </span>
          ) : null}
          {blocking > 0 ? (
            <span className={cn(CHIP, "border-border text-muted-foreground")}>
              <ArrowUpRightIcon />
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

      <TodoActivity
        comments={comments}
        createdAt={todo.created_at}
        onSend={(body) =>
          run(
            "comment",
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
          <PencilIcon />
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

function TodoDetailError(props: ErrorComponentProps) {
  return <OrchestratorError what="this todo" {...props} />;
}
