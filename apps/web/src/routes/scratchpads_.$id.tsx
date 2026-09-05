import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import type { FocusEvent, KeyboardEvent } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";

import { EditorToc } from "@/components/editor-toc";
import {
  ArchiveIcon,
  ArrowBackIcon,
  ChevronIcon,
  DotsIcon,
  TrashIcon,
} from "@/components/icons";
import type { EditorHeading } from "@/components/markdown-editor";
import { MarkdownEditor } from "@/components/markdown-editor";
import { OrchestratorError } from "@/components/orchestrator-error";
import { TagsRow } from "@/components/tags-row";
import { TimeAgo } from "@/components/time-ago";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useAction } from "@/hooks/use-action";
import {
  deleteScratchpad,
  getProjects,
  getScratchpad,
  updateScratchpad,
} from "@/lib/lgtm/server";
import type { Project, Scratchpad } from "@/lib/lgtm/types";

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
function commitOrRestore(event: KeyboardEvent<HTMLTextAreaElement>) {
  if (event.key === "Enter") {
    // A title wraps but never holds a line break.
    event.preventDefault();
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
  const [armed, setArmed] = useState(false);
  const disarm = useCallback(() => setArmed(false), []);
  const onMenuOpenChange = useCallback(
    (open: boolean) => {
      if (!open) {
        disarm();
      }
    },
    [disarm]
  );
  const { busy, run } = useAction<Action>({ onStart: disarm });

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
      if (repository === (pad.repository ?? EVERY_REPOSITORY)) {
        return;
      }
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
    [run, pad.id, pad.repository, repositoryName]
  );

  const rename = useCallback(
    (event: FocusEvent<HTMLTextAreaElement>) => {
      const title = event.target.value.replace(/\s+/g, " ").trim();
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
    // own gutters. One grid holds the title and document on the left and the
    // actions and details on the right, so the right column starts at the top
    // however many lines the title takes. The second row is the flexible one,
    // so a rail taller than the document grows that row, never the title's.
    <article className="mx-auto grid w-full max-w-5xl gap-8 px-4 py-6 sm:px-6 lg:grid-cols-[minmax(0,1fr)_14rem] lg:grid-rows-[auto_1fr] lg:px-8">
      {/* The first line is as tall as PageHeading's row, so the title sits where
          every other page's does and the dots beside it centre on that line. */}
      <header className="flex min-w-0 items-start gap-3 py-1">
        {/* The heading is the input: click it, type, and leaving it saves.
            Emptied, it shows the saved title and leaving it changes nothing. */}
        <h1 className="min-w-0 max-w-3xl flex-1 font-medium text-xl tracking-tight">
          {/* A textarea, so a long title wraps like the document under it;
              the caret is its only focus mark. */}
          <textarea
            aria-label="Title"
            className="field-sizing-content w-full resize-none overflow-hidden bg-transparent outline-none"
            defaultValue={pad.title}
            disabled={busy}
            key={pad.title}
            onBlur={rename}
            onKeyDown={commitOrRestore}
            placeholder={pad.title}
            rows={1}
          />
        </h1>
        {pad.archived ? (
          <Badge className="mt-1" variant="outline">
            archived
          </Badge>
        ) : null}
      </header>

      {/* Between the title and the document in the DOM, so a narrow screen
          shows the actions and details before the document, not under it. */}
      <aside className="flex flex-col gap-6 self-start lg:sticky lg:top-6 lg:col-start-2 lg:row-span-2 lg:row-start-1 lg:w-56">
        <div className="flex h-9 items-center justify-end gap-2">
          <span
            aria-live="polite"
            className="min-w-14 text-end text-muted-foreground text-xs"
          >
            {SAVE_LABEL[saveState]}
          </span>
          <DropdownMenu onOpenChange={onMenuOpenChange}>
            <DropdownMenuTrigger
              render={
                <Button
                  aria-label="Scratchpad actions"
                  className="text-muted-foreground"
                  disabled={busy}
                  size="icon-sm"
                  variant="ghost"
                />
              }
            >
              <DotsIcon aria-hidden="true" />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-44 rounded-lg">
              <DropdownMenuItem className="gap-2 px-2 py-1.5" onClick={archive}>
                {pad.archived ? <ArrowBackIcon /> : <ArchiveIcon />}
                <span>{pad.archived ? "Unarchive" : "Archive"}</span>
              </DropdownMenuItem>
              {/* The first press arms and keeps the menu open; the second
                  deletes. Closing the menu any other way disarms. */}
              <DropdownMenuItem
                className="gap-2 px-2 py-1.5"
                closeOnClick={armed}
                onClick={onDelete}
                variant="destructive"
              >
                <TrashIcon />
                <span>{armed ? "Confirm delete" : "Delete"}</span>
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        {/* A property list: labels in one column, values in the other, every
            value plain text and the editable ones ghost controls. */}
        <dl className="grid grid-cols-[4.5rem_minmax(0,1fr)] items-center gap-x-2 gap-y-1 text-sm">
          <dt className="text-muted-foreground">Repository</dt>
          <dd className="min-w-0">
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <button
                    className="-mx-1.5 flex h-7 max-w-full items-center gap-1 rounded-md px-1.5 transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50 disabled:opacity-50 [&_svg]:size-3.5 [&_svg]:shrink-0"
                    disabled={busy}
                    type="button"
                  />
                }
              >
                <span className="truncate">
                  {repositoryName(pad.repository ?? EVERY_REPOSITORY)}
                </span>
                <ChevronIcon
                  className="text-muted-foreground"
                  direction="down"
                />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="w-56 rounded-lg">
                <DropdownMenuRadioGroup
                  onValueChange={setRepository}
                  value={pad.repository ?? EVERY_REPOSITORY}
                >
                  {[EVERY_REPOSITORY, ...repositories].map((option) => (
                    <DropdownMenuRadioItem
                      className="gap-2 px-2 py-1.5"
                      key={option}
                      value={option}
                    >
                      <span>{repositoryName(option)}</span>
                    </DropdownMenuRadioItem>
                  ))}
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>
          </dd>

          <dt className="text-muted-foreground">Created</dt>
          <dd className="flex h-7 items-center">
            <TimeAgo at={pad.created_at} />
          </dd>

          {pad.updated_at !== pad.created_at && (
            <>
              <dt className="text-muted-foreground">Edited</dt>
              <dd className="flex h-7 items-center">
                <TimeAgo at={pad.updated_at} />
              </dd>
            </>
          )}

          <dt className="self-start pt-1 text-muted-foreground">Tags</dt>
          <dd className="py-1">
            <TagsRow disabled={busy} onChange={setTags} tags={pad.tags} />
          </dd>
        </dl>

        <div className="hidden lg:block">
          <EditorToc containerRef={contentRef} headings={headings} />
        </div>
      </aside>

      {/* The rail scrolls the headings this element contains, so it has to wrap
          the rendered editor and nothing else. */}
      <div
        className="min-w-0 max-w-3xl lg:col-start-1 lg:row-start-2"
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
    </article>
  );
}

function ScratchpadError(props: ErrorComponentProps) {
  return <OrchestratorError what="this scratchpad" {...props} />;
}
