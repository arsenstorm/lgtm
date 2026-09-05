import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import type { FocusEvent, KeyboardEvent } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";

import { ActionIcon } from "@/components/action-icon";
import { EditorToc } from "@/components/editor-toc";
import {
  ArchiveIcon,
  ArrowBackIcon,
  FolderIcon,
  TrashIcon,
} from "@/components/icons";
import type { EditorHeading } from "@/components/markdown-editor";
import { MarkdownEditor } from "@/components/markdown-editor";
import { OrchestratorError } from "@/components/orchestrator-error";
import { TagsRow } from "@/components/tags-row";
import { TimeAgo } from "@/components/time-ago";
import { Picker } from "@/components/todo-chips";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useAction } from "@/hooks/use-action";
import { ARMED_CLASS, useArmedConfirm } from "@/hooks/use-armed-confirm";
import {
  deleteScratchpad,
  getProjects,
  getScratchpad,
  updateScratchpad,
} from "@/lib/lgtm/server";
import type { Project, Scratchpad } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/scratchpads_/$id")({
  loader: async ({ params }) => {
    const [pad, projects] = await Promise.all([
      getScratchpad({ data: params.id }),
      getProjects(),
    ]);
    return { pad, projects };
  },
  component: ScratchpadPage,
  errorComponent: ScratchpadError,
});

type Action = "archive" | "delete" | "repository" | "tags" | "title";

type SaveState = "idle" | "saving" | "saved";

/** A document that saves itself still has to say that it did; at rest it says
 *  nothing, so the line only speaks when something happened. */
const SAVE_LABEL: Record<SaveState, string> = {
  idle: "",
  saving: "Saving…",
  saved: "Saved",
};

/** Enter commits the title by leaving the field; Escape puts the saved one
 *  back first, so leaving then saves nothing. */
function commitOrRestore(event: KeyboardEvent<HTMLInputElement>) {
  if (event.key === "Enter") {
    event.currentTarget.blur();
  } else if (event.key === "Escape") {
    event.currentTarget.value = event.currentTarget.defaultValue;
    event.currentTarget.blur();
  }
}

/** Long enough to notice, short enough not to become furniture. */
const SAVED_MS = 2000;

function ScratchpadPage() {
  const { pad, projects } = Route.useLoaderData();
  // Everything below — the derived title, the outline, the queued markdown — is
  // state about one document, so opening another one starts it over.
  return <ScratchpadDocument key={pad.id} pad={pad} projects={projects} />;
}

/** The picker's "no repository" choice; a real repository is never blank. */
const EVERY_REPOSITORY = "";

function ScratchpadDocument({
  pad,
  projects,
}: {
  pad: Scratchpad;
  projects: Project[];
}) {
  const navigate = useNavigate();
  const [headings, setHeadings] = useState<EditorHeading[]>([]);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const contentRef = useRef<HTMLDivElement>(null);
  const queued = useRef<string | null>(null);
  const inFlight = useRef<Promise<unknown> | null>(null);
  const { armed, arm, disarm, ref: deleteRef } = useArmedConfirm();
  const { pending, busy, run } = useAction<Action>({ onStart: disarm });

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
      // The page does not invalidate the route here: nothing on screen reads the
      // refetched content, and the refetch would only race the next keystroke.
      save(markdown);
    },
    [save]
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

  const repositories = projects.flatMap((project) =>
    project.repository === null ? [] : [project.repository]
  );
  const repositoryName = useCallback(
    (repository: string) =>
      projects.find((project) => project.repository === repository)?.name ??
      "Every repository",
    [projects]
  );
  const setRepository = useCallback(
    (repository: string) => {
      run(
        "repository",
        () =>
          updateScratchpad({
            data: {
              id: pad.id,
              repository: repository === EVERY_REPOSITORY ? null : repository,
            },
          }),
        repository === EVERY_REPOSITORY
          ? "Scratchpad applies to every repository"
          : `Scratchpad moved to ${repositoryName(repository)}`
      );
    },
    [run, pad.id, repositoryName]
  );

  const rename = useCallback(
    (event: FocusEvent<HTMLInputElement>) => {
      const title = event.target.value.trim();
      if (title === "" || title === pad.title) {
        event.target.value = pad.title;
        return;
      }
      run(
        "title",
        () => updateScratchpad({ data: { id: pad.id, title } }),
        "Scratchpad renamed"
      );
    },
    [run, pad.id, pad.title]
  );

  const setTags = useCallback(
    (tags: string[], message: string) => {
      run(
        "tags",
        () => updateScratchpad({ data: { id: pad.id, tags } }),
        message
      );
    },
    [run, pad.id]
  );

  const onDelete = useCallback(async () => {
    if (!armed) {
      arm();
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
  }, [armed, arm, navigate, run, pad.id]);

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <article className="mx-auto flex w-full max-w-5xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <header className="flex flex-wrap items-start gap-x-4 gap-y-3">
        <div className="flex min-w-0 flex-1 flex-wrap items-center gap-3">
          {/* The heading is the input: click it, type, and leaving it saves. */}
          <h1 className="min-w-0 flex-1 font-medium text-xl tracking-tight">
            <input
              aria-label="Title"
              className="w-full min-w-0 rounded-sm bg-transparent outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
              defaultValue={pad.title}
              disabled={busy}
              key={pad.title}
              onBlur={rename}
              onKeyDown={commitOrRestore}
              type="text"
            />
          </h1>
          {pad.archived ? <Badge variant="outline">archived</Badge> : null}
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
              icon={pad.archived ? ArrowBackIcon : ArchiveIcon}
            />
            {pad.archived ? "Unarchive" : "Archive"}
          </Button>
          <Button
            className={cn(armed && ARMED_CLASS)}
            disabled={busy}
            onClick={onDelete}
            ref={deleteRef}
            size="lg"
            variant="destructive"
          >
            <ActionIcon busy={pending === "delete"} icon={TrashIcon} />
            {armed ? "Confirm delete" : "Delete"}
          </Button>
        </div>
      </header>

      <div className="grid gap-8 lg:grid-cols-[minmax(0,1fr)_14rem]">
        {/* First in the DOM so a narrow screen shows the details before the
            document, not under it. */}
        <aside className="flex flex-col gap-6 self-start lg:sticky lg:top-6 lg:col-start-2 lg:row-start-1 lg:w-56">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-2 text-sm lg:flex-col lg:items-start">
            <Picker
              disabled={busy}
              format={repositoryName}
              onPick={setRepository}
              options={[EVERY_REPOSITORY, ...repositories]}
              triggerClassName="border-border"
              value={pad.repository ?? EVERY_REPOSITORY}
            >
              <FolderIcon />
              <span className="truncate">
                {repositoryName(pad.repository ?? EVERY_REPOSITORY)}
              </span>
            </Picker>
            <span className="text-muted-foreground">
              created <TimeAgo at={pad.created_at} />
            </span>
            {pad.updated_at !== pad.created_at && (
              <span className="text-muted-foreground">
                edited <TimeAgo at={pad.updated_at} />
              </span>
            )}
            <TagsRow disabled={busy} onChange={setTags} tags={pad.tags} />
          </div>

          <div className="hidden lg:block">
            <EditorToc containerRef={contentRef} headings={headings} />
          </div>
        </aside>

        {/* The rail scrolls the headings this element contains, so it has to wrap
            the rendered editor and nothing else. */}
        <div
          className="min-w-0 max-w-3xl lg:col-start-1 lg:row-start-1"
          ref={contentRef}
        >
          <MarkdownEditor
            autoFocus={pad.content === ""}
            onHeadings={setHeadings}
            onMarkdown={onMarkdown}
            placeholder="Start writing…"
            value={pad.content}
          />
        </div>
      </div>
    </article>
  );
}

function ScratchpadError(props: ErrorComponentProps) {
  return <OrchestratorError what="this scratchpad" {...props} />;
}
