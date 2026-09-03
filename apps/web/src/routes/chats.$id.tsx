import { ArrowUp, CircleNotch } from "@phosphor-icons/react";
import type { ErrorComponentProps } from "@tanstack/react-router";
import {
  createFileRoute,
  useNavigate,
  useRouter,
} from "@tanstack/react-router";
import type { ChangeEvent, KeyboardEvent } from "react";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";

import { TasksIcon } from "@/components/icons";
import { OrchestratorError } from "@/components/orchestrator-error";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { transcript, WorkspaceChat } from "@/components/workspace-chat";
import {
  askChat,
  enhancePrompt,
  getChat,
  getRunners,
  getTasks,
  getTodos,
} from "@/lib/lgtm/server";
import { BARE_CONTROL, cn } from "@/lib/utils";

export const Route = createFileRoute("/chats/$id")({
  loader: async ({ params }) => {
    const [chat, runners, tasks, todos] = await Promise.all([
      getChat({ data: params.id }),
      getRunners(),
      getTasks(),
      getTodos(),
    ]);
    return { chat, runners, tasks, todos };
  },
  component: ChatPage,
  errorComponent: ChatError,
});

const POLL_MS = 2500;

function ChatPage() {
  const { chat, runners, tasks, todos } = Route.useLoaderData();
  const router = useRouter();
  const navigate = useNavigate();
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState<"send" | "brief" | null>(null);
  // The answer lands in the background; while the last word is the person's
  // the loader is re-run on a short interval, the way the task page streams.
  const pending = chat.turns.at(-1)?.role === "person";

  useEffect(() => {
    if (!pending) {
      return;
    }
    const id = window.setInterval(() => router.invalidate(), POLL_MS);
    return () => window.clearInterval(id);
  }, [pending, router]);

  const send = useCallback(async () => {
    const asked = draft.trim();
    if (asked === "" || busy !== null || pending) {
      return;
    }
    setDraft("");
    setBusy("send");
    try {
      await askChat({ data: { id: chat.id, question: asked } });
      await router.invalidate();
    } catch (error) {
      // The orchestrator's reason is the whole message; the question goes
      // back in the box.
      toast.error(error instanceof Error ? error.message : String(error));
      setDraft(asked);
    } finally {
      setBusy(null);
    }
  }, [busy, chat.id, draft, pending, router]);

  // The step from talking to doing: a brief written from the transcript, or
  // the last thing the person said when nothing could write one, handed to
  // the composer in task mode.
  const createTask = useCallback(async () => {
    if (busy !== null) {
      return;
    }
    setBusy("brief");
    let brief =
      chat.turns.filter((turn) => turn.role === "person").at(-1)?.text ?? "";
    try {
      const result = await enhancePrompt({
        data: {
          prompt: `Conversation with the workspace agent:\n\n${transcript(chat.turns)}`,
        },
      });
      brief = result.prompt;
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(null);
    }
    await navigate({ search: { draft: brief, repo: undefined }, to: "/" });
  }, [busy, chat.turns, navigate]);

  const changeDraft = useCallback(
    (event: ChangeEvent<HTMLTextAreaElement>) =>
      setDraft(event.currentTarget.value),
    []
  );
  const sendOnEnter = useCallback(
    (event: KeyboardEvent<HTMLTextAreaElement>) => {
      if (event.key === "Enter" && !event.shiftKey) {
        event.preventDefault();
        send();
      }
    },
    [send]
  );

  const createTaskButton = (
    <button
      className={cn(BARE_CONTROL, "disabled:opacity-40")}
      disabled={busy !== null}
      onClick={createTask}
      type="button"
    >
      {busy === "brief" ? (
        <CircleNotch className="size-4 animate-spin" />
      ) : (
        <TasksIcon className="size-4" />
      )}
      Create task
    </button>
  );

  return (
    <div className="mx-auto flex min-h-full w-full max-w-2xl flex-col px-4 py-6 sm:px-6">
      <WorkspaceChat
        action={createTaskButton}
        pending={pending}
        references={{ runners, tasks, todos }}
        turns={chat.turns}
      />

      <div className="sticky bottom-6 mt-auto flex flex-col pt-8">
        {/* In light mode --card equals --background, so this hairline is the
            only thing separating the composer from the page. */}
        <div className="relative flex flex-col rounded-[18px] border border-foreground/6 bg-card transition-colors focus-within:border-foreground/15">
          <Textarea
            aria-label="Message"
            className="max-h-64 min-h-20 resize-none border-0 bg-transparent px-3 py-3 shadow-none placeholder:text-muted-foreground/60 focus-visible:ring-0 dark:bg-transparent"
            onChange={changeDraft}
            onKeyDown={sendOnEnter}
            placeholder="Ask a follow-up…"
            value={draft}
          />
          <div className="flex flex-wrap items-center gap-3 px-4 pb-4">
            {createTaskButton}
            <Button
              aria-label="Send"
              className="ml-auto size-8 shrink-0 rounded-full bg-foreground text-background hover:bg-foreground/90 disabled:opacity-40"
              disabled={busy !== null || pending || draft.trim() === ""}
              onClick={send}
              size="icon"
            >
              {busy === "send" ? (
                <CircleNotch className="animate-spin" />
              ) : (
                <ArrowUp />
              )}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

function ChatError(props: ErrorComponentProps) {
  return <OrchestratorError what="this chat" {...props} />;
}
