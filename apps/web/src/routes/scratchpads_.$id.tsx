import type { Icon } from "@phosphor-icons/react";
import {
  Archive,
  ArrowCounterClockwise,
  CircleNotch,
  Trash,
} from "@phosphor-icons/react";
import type { ErrorComponentProps } from "@tanstack/react-router";
import {
  createFileRoute,
  useNavigate,
  useRouter,
} from "@tanstack/react-router";
import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";

import { projectName } from "@/components/app-sidebar";
import { EditorToc } from "@/components/editor-toc";
import type { EditorHeading } from "@/components/markdown-editor";
import { MarkdownEditor } from "@/components/markdown-editor";
import { OrchestratorError } from "@/components/orchestrator-error";
import { TimeAgo } from "@/components/time-ago";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  deleteScratchpad,
  getScratchpad,
  updateScratchpad,
} from "@/lib/lgtm/server";
import type { Scratchpad } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";
import { padTitle } from "@/routes/scratchpads";

export const Route = createFileRoute("/scratchpads_/$id")({
  loader: ({ params }) => getScratchpad({ data: params.id }),
  component: ScratchpadPage,
  errorComponent: ScratchpadError,
});

type Action = "archive" | "delete";

type SaveState = "idle" | "saving" | "saved";

/** A document that saves itself still has to say that it did; at rest it says
 *  nothing, so the line only speaks when something happened. */
const SAVE_LABEL: Record<SaveState, string> = {
  idle: "",
  saving: "Saving…",
  saved: "Saved",
};

/** Long enough to notice, short enough not to become furniture. */
const SAVED_MS = 2000;

/** Long enough to read "Confirm delete", short enough that a forgotten arm
 *  cannot still be live when the next person reaches the keyboard. */
const DISARM_MS = 4000;

function ScratchpadPage() {
  const pad = Route.useLoaderData();
  // Everything below — the derived title, the outline, the queued markdown — is
  // state about one document, so opening another one starts it over.
  return <ScratchpadDocument key={pad.id} pad={pad} />;
}

function ScratchpadDocument({ pad }: { pad: Scratchpad }) {
  const router = useRouter();
  const navigate = useNavigate();
  const [pending, setPending] = useState<Action | null>(null);
  const [armed, setArmed] = useState(false);
  const [title, setTitle] = useState(() => padTitle(pad.content));
  const [headings, setHeadings] = useState<EditorHeading[]>([]);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const deleteRef = useRef<HTMLButtonElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const queued = useRef<string | null>(null);
  const inFlight = useRef<Promise<unknown> | null>(null);

  // Arming delete puts the page in a mode, and a mode nobody meant to enter has
  // to expire on its own: a pointer anywhere else, Escape, or the timeout.
  useEffect(() => {
    if (!armed) {
      return;
    }

    const disarm = () => setArmed(false);
    const onPointerDown = (event: PointerEvent) => {
      if (!deleteRef.current?.contains(event.target as Node)) {
        disarm();
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        disarm();
      }
    };

    const timer = window.setTimeout(disarm, DISARM_MS);
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [armed]);

  useEffect(() => {
    if (saveState !== "saved") {
      return;
    }
    const timer = window.setTimeout(() => setSaveState("idle"), SAVED_MS);
    return () => window.clearTimeout(timer);
  }, [saveState]);

  const save = useCallback(
    async (markdown: string) => {
      // Latest wins: a save already in flight parks the newer markdown and
      // replays it when the request lands, instead of racing it. Two writes
      // arriving out of order would resurrect a paragraph the writer deleted.
      queued.current = markdown;
      if (inFlight.current !== null) {
        return;
      }
      setSaveState("saving");
      try {
        while (queued.current !== null) {
          const content = queued.current;
          queued.current = null;
          inFlight.current = updateScratchpad({
            data: { id: pad.id, content },
          });
          // biome-ignore lint/performance/noAwaitInLoops: sequential is the point — ordered writes to one document, not independent requests
          await inFlight.current;
        }
        setSaveState("saved");
      } catch (error) {
        // Autosave is silent when it works; when it fails, silence would let
        // someone keep typing into a document that stopped being written down.
        toast.error(error instanceof Error ? error.message : String(error));
        setSaveState("idle");
      } finally {
        inFlight.current = null;
      }
    },
    [pad.id]
  );

  const onMarkdown = useCallback(
    (markdown: string) => {
      setTitle(padTitle(markdown));
      // The page does not invalidate the route here: nothing on screen reads the
      // refetched content, and the refetch would only race the next keystroke.
      save(markdown);
    },
    [save]
  );

  const busy = pending !== null;

  const run = useCallback(
    async (action: Action, call: () => Promise<unknown>, message: string) => {
      setPending(action);
      setArmed(false);
      try {
        await call();
        toast.success(message);
        await router.invalidate();
        return true;
      } catch (error) {
        // The orchestrator's refusal reason is the whole message; genericising
        // it would throw away the only thing that says what to do next.
        toast.error(error instanceof Error ? error.message : String(error));
        return false;
      } finally {
        setPending(null);
      }
    },
    [router]
  );

  const archive = useCallback(
    () =>
      run(
        "archive",
        () =>
          updateScratchpad({ data: { id: pad.id, archived: !pad.archived } }),
        pad.archived ? "Scratchpad restored" : "Scratchpad archived"
      ),
    [run, pad.archived, pad.id]
  );

  const onDelete = useCallback(async () => {
    if (!armed) {
      setArmed(true);
      return;
    }
    const deleted = await run(
      "delete",
      () => deleteScratchpad({ data: pad.id }),
      "Scratchpad deleted"
    );
    if (deleted) {
      await navigate({ to: "/scratchpads" });
    }
  }, [armed, navigate, run, pad.id]);

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <article className="mx-auto flex w-full max-w-7xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <header className="flex flex-wrap items-start gap-x-4 gap-y-3">
        <div className="flex min-w-0 flex-1 flex-col gap-3">
          <div className="flex min-w-0 flex-wrap items-center gap-3">
            <h1 className="min-w-0 truncate font-medium text-xl tracking-tight">
              {title}
            </h1>
            {pad.archived ? <Badge variant="outline">archived</Badge> : null}
          </div>

          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-muted-foreground text-sm">
            {pad.repository !== null && (
              <span>{projectName(pad.repository)}</span>
            )}
            <span>
              created <TimeAgo at={pad.created_at} />
            </span>
            {pad.updated_at !== pad.created_at && (
              <span>
                edited <TimeAgo at={pad.updated_at} />
              </span>
            )}
          </div>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          <span
            aria-live="polite"
            className="min-w-14 text-end text-muted-foreground text-xs"
          >
            {SAVE_LABEL[saveState]}
          </span>
          <Button disabled={busy} onClick={archive} size="lg" variant="outline">
            <ActionIcon
              busy={pending === "archive"}
              icon={pad.archived ? ArrowCounterClockwise : Archive}
            />
            {pad.archived ? "Unarchive" : "Archive"}
          </Button>
          <Button
            className={cn(
              // The variant's own `dark:` classes outrank an unprefixed
              // override, so the armed fill has to be stated for both themes.
              armed &&
                "bg-destructive text-destructive-foreground hover:bg-destructive/90 dark:bg-destructive dark:hover:bg-destructive/90"
            )}
            disabled={busy}
            onClick={onDelete}
            ref={deleteRef}
            size="lg"
            variant="destructive"
          >
            <ActionIcon busy={pending === "delete"} icon={Trash} />
            {armed ? "Confirm delete" : "Delete"}
          </Button>
        </div>
      </header>

      <div className="grid gap-8 lg:grid-cols-[minmax(0,1fr)_14rem]">
        {/* The rail scrolls the headings this element contains, so it has to wrap
            the rendered editor and nothing else. */}
        <div className="min-w-0 max-w-3xl" ref={contentRef}>
          <MarkdownEditor
            autoFocus={pad.content === ""}
            onHeadings={setHeadings}
            onMarkdown={onMarkdown}
            placeholder="Start writing — the first heading becomes the title."
            value={pad.content}
          />
        </div>

        <aside className="hidden w-56 self-start lg:sticky lg:top-6 lg:block">
          <EditorToc containerRef={contentRef} headings={headings} />
        </aside>
      </div>
    </article>
  );
}

/** Swapping the leading icon for the spinner, rather than adding one, keeps the
 *  button the same width while it works. */
function ActionIcon({
  icon: IconComponent,
  busy,
}: {
  icon: Icon;
  busy: boolean;
}) {
  if (busy) {
    return (
      <CircleNotch
        className="motion-safe:animate-spin"
        data-icon="inline-start"
      />
    );
  }
  return <IconComponent data-icon="inline-start" />;
}

function ScratchpadError(props: ErrorComponentProps) {
  return <OrchestratorError what="this scratchpad" {...props} />;
}
