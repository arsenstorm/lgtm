import type { ErrorComponentProps } from "@tanstack/react-router";
import {
  createFileRoute,
  Link,
  useLoaderData,
  useNavigate,
} from "@tanstack/react-router";
import type { FocusEvent, KeyboardEvent } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";

import { useRightPanel } from "@/components/app-shell";
import { EditorToc } from "@/components/editor-toc";
import {
  CheckIcon,
  ChevronIcon,
  CopyIcon,
  DotsIcon,
  TrashIcon,
} from "@/components/icons";
import type { EditorHeading } from "@/components/markdown-editor";
import { EditorSkeleton, MarkdownEditor } from "@/components/markdown-editor";
import { OrchestratorError } from "@/components/orchestrator-error";
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
import { Skeleton } from "@/components/ui/skeleton";
import { useAction } from "@/hooks/use-action";
import {
  approveSkill,
  deleteSkill,
  getSkill,
  updateSkill,
} from "@/lib/lgtm/server";
import type { Project, Skill } from "@/lib/lgtm/types";

export const Route = createFileRoute("/skills_/$id")({
  loader: ({ params }) => getSkill({ data: params.id }),
  // Tiptap cannot render on the server, so the document only ever paints on
  // the client; sending its skeleton in the HTML keeps the page's shape from
  // the first byte instead of leaving a hole until hydration.
  ssr: "data-only",
  pendingComponent: SkillSkeleton,
  staticData: {
    rightPanel: { content: <DetailsSkeleton />, title: "Details" },
  },
  component: SkillPage,
  errorComponent: SkillError,
});

type Action = "approve" | "delete" | "description" | "name" | "repository";

type SaveState = "idle" | "saving" | "saved";

/** The text after the frontmatter's closing `---`; the frontmatter itself is
 *  the orchestrator's to rewrite. */
function skillBody(content: string): string {
  const lines = content.split("\n");
  for (let i = 1; i < lines.length; i += 1) {
    if (lines[i].trimEnd() === "---") {
      return lines.slice(i + 1).join("\n");
    }
  }
  return content;
}

/** A document that saves itself still has to say that it did; at rest it says
 *  nothing, so the line only speaks when something happened. */
const SAVE_LABEL: Record<SaveState, string> = {
  idle: "",
  saving: "Saving…",
  saved: "Saved",
};

/** Enter commits the field by leaving it; Escape puts the saved value back
 *  first, so leaving then saves nothing. */
function commitOrRestore(event: KeyboardEvent<HTMLTextAreaElement>) {
  if (event.key === "Enter") {
    // The field wraps but never holds a line break.
    event.preventDefault();
    event.currentTarget.blur();
  } else if (event.key === "Escape") {
    event.currentTarget.value = event.currentTarget.defaultValue;
    event.currentTarget.blur();
  }
}

/** Long enough to notice, short enough not to become furniture. */
const SAVED_MS = 2000;

/** The labels are known before the document is; only the values wait. */
function DetailsSkeleton() {
  return (
    <dl className="grid grid-cols-[4.5rem_minmax(0,1fr)] items-center gap-x-2 gap-y-1 text-sm">
      <dt className="text-muted-foreground">Repository</dt>
      <dd className="flex h-7 items-center">
        <Skeleton className="h-4 w-16" />
      </dd>
      <dt className="text-muted-foreground">Created</dt>
      <dd className="flex h-7 items-center">
        <Skeleton className="h-4 w-12" />
      </dd>
    </dl>
  );
}

function SkillSkeleton() {
  return (
    <article className="mx-auto flex w-full max-w-5xl flex-col px-4 pt-6 pb-24 sm:px-6 lg:px-8">
      <header className="flex h-9 items-center">
        <Skeleton className="h-6 w-64" />
      </header>
      <EditorSkeleton className="mt-2" />
    </article>
  );
}

function SkillPage() {
  const skill = Route.useLoaderData();
  const { projects } = useLoaderData({ from: "__root__" });
  // Everything below — the outline, the queued markdown — is state about one
  // document, so opening another one starts it over.
  return <SkillDocument key={skill.id} projects={projects} skill={skill} />;
}

/** The picker's "no repository" choice; a real repository is never blank. */
const EVERY_REPOSITORY = "";

function SkillDocument({
  projects,
  skill,
}: {
  projects: Project[];
  skill: Skill;
}) {
  const navigate = useNavigate();
  const body = skillBody(skill.content);
  // The frontmatter is whatever the body is not; the orchestrator rewrites it,
  // so the page only ever carries it around.
  const frontmatter = skill.content.slice(
    0,
    skill.content.length - body.length
  );
  const [headings, setHeadings] = useState<EditorHeading[]>([]);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const contentRef = useRef<HTMLDivElement>(null);
  const latest = useRef(body);
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
          const next = queued.current;
          queued.current = null;
          inFlight.current = updateSkill({
            data: { body: next, id: skill.id },
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
    [skill.id]
  );

  const onMarkdown = useCallback(
    (markdown: string) => {
      // The page does not invalidate the route here: nothing on screen reads the
      // refetched content, and the refetch would only race the next keystroke.
      latest.current = markdown;
      save(markdown);
    },
    [save]
  );

  const copySkill = useCallback(() => {
    navigator.clipboard.writeText(`${frontmatter}${latest.current}`).then(
      () => toast.success("Copied SKILL.md"),
      (error: unknown) =>
        toast.error(error instanceof Error ? error.message : String(error))
    );
  }, [frontmatter]);

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
      if (repository === (skill.repository ?? EVERY_REPOSITORY)) {
        return;
      }
      run(
        "repository",
        () =>
          updateSkill({
            data: {
              id: skill.id,
              repository: repository === EVERY_REPOSITORY ? null : repository,
            },
          }),
        repository === EVERY_REPOSITORY
          ? "Skill applies to every repository"
          : `Skill moved to ${repositoryName(repository)}`
      );
    },
    [run, skill.id, skill.repository, repositoryName]
  );

  const rename = useCallback(
    (event: FocusEvent<HTMLTextAreaElement>) => {
      const name = event.target.value.replace(/\s+/g, " ").trim();
      if (name === "" || name === skill.name) {
        event.target.value = skill.name;
        return;
      }
      // The orchestrator refuses a name the spec disallows; its reason is the
      // toast, and the field's key puts the saved name back.
      run(
        "name",
        () => updateSkill({ data: { id: skill.id, name } }),
        "Skill renamed"
      );
    },
    [run, skill.id, skill.name]
  );

  const describe = useCallback(
    (event: FocusEvent<HTMLTextAreaElement>) => {
      const description = event.target.value.replace(/\s+/g, " ").trim();
      if (description === "" || description === skill.description) {
        event.target.value = skill.description;
        return;
      }
      run(
        "description",
        () => updateSkill({ data: { description, id: skill.id } }),
        "Description updated"
      );
    },
    [run, skill.description, skill.id]
  );

  const approve = useCallback(
    () =>
      run("approve", () => approveSkill({ data: skill.id }), "Skill approved"),
    [run, skill.id]
  );

  const onDelete = useCallback(async () => {
    if (!armed) {
      setArmed(true);
      return;
    }
    const deleted = await run(
      "delete",
      () => deleteSkill({ data: skill.id }),
      "Skill deleted"
    );
    if (deleted) {
      await navigate({ to: "/skills" });
    }
  }, [armed, navigate, run, skill.id]);

  const proposed = skill.verification === "agent_proposed";

  useRightPanel({
    title: "Details",
    content: (
      <div className="flex flex-col gap-6">
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
                  {repositoryName(skill.repository ?? EVERY_REPOSITORY)}
                </span>
                <ChevronIcon
                  className="text-muted-foreground"
                  direction="down"
                />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="w-56 rounded-lg">
                <DropdownMenuRadioGroup
                  onValueChange={setRepository}
                  value={skill.repository ?? EVERY_REPOSITORY}
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
            <TimeAgo at={skill.created_at} />
          </dd>

          {skill.updated_at !== skill.created_at && (
            <>
              <dt className="text-muted-foreground">Edited</dt>
              <dd className="flex h-7 items-center">
                <TimeAgo at={skill.updated_at} />
              </dd>
            </>
          )}

          {skill.origin !== null && (
            <>
              <dt className="self-start pt-1 text-muted-foreground">Origin</dt>
              <dd className="min-w-0 py-1">
                <span className="break-all font-mono text-xs">
                  {skill.origin}
                </span>
              </dd>
            </>
          )}

          {skill.files.length > 0 && (
            <>
              <dt className="self-start pt-1 text-muted-foreground">Files</dt>
              <dd className="min-w-0 py-1">
                <ul>
                  {skill.files.map((file) => (
                    <li className="truncate font-mono text-xs" key={file.path}>
                      {file.path}
                      {file.binary && (
                        <span className="text-muted-foreground"> binary</span>
                      )}
                    </li>
                  ))}
                </ul>
              </dd>
            </>
          )}

          {skill.proposed_by !== null && (
            <>
              <dt className="text-muted-foreground">Proposed by</dt>
              <dd className="flex h-7 min-w-0 items-center">
                <Link
                  className="truncate font-mono text-xs hover:underline"
                  params={{ id: skill.proposed_by }}
                  to="/tasks/$id"
                >
                  {skill.proposed_by}
                </Link>
              </dd>
            </>
          )}
        </dl>
        <EditorToc containerRef={contentRef} headings={headings} />
      </div>
    ),
  });

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    // Room under the document lets its last section reach the top when the
    // outline jumps to it.
    <article className="mx-auto flex w-full max-w-5xl flex-col px-4 pt-6 pb-24 sm:px-6 lg:px-8">
      {/* The first line is as tall as PageHeading's row, so the name sits where
          every other page's title does and the dots beside it centre on that
          line; the description hangs under it in the same column. */}
      <header className="flex items-start gap-3">
        <div className="flex min-w-0 flex-1 flex-col gap-1 py-1">
          <div className="flex min-w-0 items-start gap-3">
            {/* The heading is the input: click it, type, and leaving it saves.
                Emptied, it shows the saved name and leaving it changes nothing. */}
            <h1 className="min-w-0 flex-1 font-medium text-xl tracking-tight">
              {/* A textarea, so a long name wraps like the document under it;
                  the caret is its only focus mark. */}
              <textarea
                aria-label="Title"
                className="field-sizing-content w-full resize-none overflow-hidden bg-transparent outline-none"
                defaultValue={skill.name}
                disabled={busy}
                key={skill.name}
                onBlur={rename}
                onKeyDown={commitOrRestore}
                placeholder={skill.name}
                rows={1}
              />
            </h1>
            {proposed ? (
              <Badge
                className="mt-1 border-amber-600/30 text-amber-700 dark:text-amber-400"
                variant="outline"
              >
                proposed
              </Badge>
            ) : null}
          </div>
          {/* The description is what an agent reads to decide whether to open
              the skill at all, so it sits with the name, not in the panel. */}
          <textarea
            aria-label="Description"
            className="field-sizing-content w-full resize-none overflow-hidden bg-transparent pb-3 text-muted-foreground text-sm outline-none"
            defaultValue={skill.description}
            disabled={busy}
            key={skill.description}
            onBlur={describe}
            onKeyDown={commitOrRestore}
            rows={1}
          />
        </div>

        <div className="flex h-9 shrink-0 items-center gap-2">
          <span
            aria-live="polite"
            className="min-w-14 text-end text-muted-foreground text-xs"
          >
            {SAVE_LABEL[saveState]}
          </span>
          <Button
            aria-label="Copy SKILL.md"
            className="text-muted-foreground"
            onClick={copySkill}
            size="icon-sm"
            variant="ghost"
          >
            <CopyIcon aria-hidden="true" />
          </Button>
          <DropdownMenu onOpenChange={onMenuOpenChange}>
            <DropdownMenuTrigger
              render={
                <Button
                  aria-label="Skill actions"
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
              {proposed && (
                <DropdownMenuItem
                  className="gap-2 px-2 py-1.5"
                  onClick={approve}
                >
                  <CheckIcon />
                  <span>Approve</span>
                </DropdownMenuItem>
              )}
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
      </header>

      {/* The panel's outline scrolls the headings this element contains, so it
          has to wrap the rendered editor and nothing else. */}
      <div className="min-w-0" ref={contentRef}>
        <MarkdownEditor
          autoFocus={body.trim() === ""}
          onHeadings={setHeadings}
          onMarkdown={onMarkdown}
          placeholder="Write the steps an agent should follow…"
          value={body}
        />
      </div>
    </article>
  );
}

function SkillError(props: ErrorComponentProps) {
  return <OrchestratorError what="this skill" {...props} />;
}
