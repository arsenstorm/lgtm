import { CircleNotch, Notepad, Plus, Tag, X } from "@phosphor-icons/react";
import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { toast } from "sonner";

import { projectName } from "@/components/app-sidebar";
import { OrchestratorError } from "@/components/orchestrator-error";
import { TimeAgo } from "@/components/time-ago";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { createScratchpad, getScratchpads } from "@/lib/lgtm/server";
import type { Scratchpad } from "@/lib/lgtm/types";

export const Route = createFileRoute("/scratchpads")({
  loader: async () => ({ scratchpads: await getScratchpads() }),
  component: ScratchpadsPage,
  errorComponent: ScratchpadsError,
});

const TITLE_MAX = 80;

function cut(text: string): string {
  return text.length > TITLE_MAX ? `${text.slice(0, TITLE_MAX)}…` : text;
}

/** A scratchpad has no title field: like any markdown file, its first `# `
 *  heading names it, and failing that its first written line does. */
export function padTitle(content: string): string {
  const lines = content.split("\n");
  const heading = lines.find((line) => line.startsWith("# "));
  if (heading) {
    return cut(heading.slice(2).trim());
  }

  const first = lines.find((line) => line.trim() !== "");
  return first ? cut(first.trim()) : "Untitled";
}

const byEdited = (a: Scratchpad, b: Scratchpad) => b.updated_at - a.updated_at;

const TAG_CHIP =
  "inline-flex shrink-0 items-center gap-1 rounded-full border border-border px-2 py-0.5 text-muted-foreground text-xs";

/** How many tags a row shows before the rest collapse into a count. */
const TAGS_SHOWN = 3;

/** Todos and scratchpads edit tags the same way. This route file is the one
 *  place both detail pages already import from, and a shared components/ home
 *  is out of scope for this change. */
export function TagsRow({
  tags,
  disabled,
  onChange,
}: {
  disabled: boolean;
  onChange: (next: string[], message: string) => void;
  tags: string[];
}) {
  const [draft, setDraft] = useState<string | null>(null);

  function commit() {
    const tag = (draft ?? "").trim();
    setDraft(null);
    if (tag !== "" && !tags.includes(tag)) {
      onChange([...tags, tag], "Tag added");
    }
  }

  return (
    <div className="flex min-w-0 flex-wrap items-center gap-1.5">
      {tags.map((tag) => (
        <span className={TAG_CHIP} key={tag}>
          <Tag aria-hidden="true" className="size-3" />
          {tag}
          <button
            aria-label={`Remove ${tag}`}
            className="-mr-1 rounded-full p-0.5 transition-colors hover:text-foreground disabled:opacity-50"
            disabled={disabled}
            onClick={() =>
              onChange(
                tags.filter((other) => other !== tag),
                "Tag removed"
              )
            }
            type="button"
          >
            <X className="size-3" />
          </button>
        </span>
      ))}

      {draft === null ? (
        <Button
          disabled={disabled}
          onClick={() => setDraft("")}
          size="xs"
          variant="ghost"
        >
          <Plus data-icon="inline-start" />
          Add tag
        </Button>
      ) : (
        <Input
          aria-label="New tag"
          autoFocus
          className="h-6 w-32 text-xs md:text-xs"
          // Blur cancels: an abandoned field should not leave a half-typed tag
          // sitting in the row.
          onBlur={() => setDraft(null)}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              commit();
            } else if (event.key === "Escape") {
              setDraft(null);
            }
          }}
          placeholder="tag"
          value={draft}
        />
      )}
    </div>
  );
}

function ScratchpadsPage() {
  const { scratchpads } = Route.useLoaderData();
  const navigate = useNavigate();
  const [creating, setCreating] = useState(false);

  const live = scratchpads.filter((pad) => !pad.archived).sort(byEdited);
  const archived = scratchpads.filter((pad) => pad.archived).sort(byEdited);

  async function create() {
    setCreating(true);
    try {
      const pad = await createScratchpad({ data: { content: "" } });
      // The blank document opening is the success signal; a toast on top of it
      // would only say what the screen already shows.
      await navigate({ to: "/scratchpads/$id", params: { id: pad.id } });
    } catch (error) {
      // The orchestrator's refusal reason is the whole message; genericising it
      // would throw away the only thing that says what to do next.
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      setCreating(false);
    }
  }

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-7xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <div className="flex items-center gap-3">
        <h1 className="font-medium text-xl tracking-tight">Scratchpads</h1>
        <span className="text-muted-foreground text-sm tabular-nums">
          {live.length}
        </span>
        <Button
          className="ms-auto"
          disabled={creating}
          onClick={create}
          size="lg"
        >
          {creating ? (
            <CircleNotch
              className="motion-safe:animate-spin"
              data-icon="inline-start"
            />
          ) : (
            <Plus data-icon="inline-start" />
          )}
          New scratchpad
        </Button>
      </div>

      {scratchpads.length === 0 ? (
        <p className="text-muted-foreground text-sm">
          No scratchpads yet. New scratchpad starts one.
        </p>
      ) : (
        <>
          <ul className="-mx-2 divide-y divide-foreground/5" role="list">
            {live.map((pad) => (
              <li key={pad.id}>
                <ScratchpadRow pad={pad} />
              </li>
            ))}
          </ul>

          {archived.length > 0 && (
            <section className="flex flex-col gap-2">
              <h2 className="font-medium text-muted-foreground text-sm">
                Archived
              </h2>
              <ul className="-mx-2 divide-y divide-foreground/5" role="list">
                {archived.map((pad) => (
                  <li key={pad.id}>
                    <ScratchpadRow pad={pad} />
                  </li>
                ))}
              </ul>
            </section>
          )}
        </>
      )}
    </div>
  );
}

function ScratchpadRow({ pad }: { pad: Scratchpad }) {
  return (
    <Link
      className="flex items-center gap-3 rounded-md px-2 py-2.5 text-sm hover:bg-foreground/4"
      params={{ id: pad.id }}
      to="/scratchpads/$id"
    >
      <Notepad className="size-4 shrink-0 text-muted-foreground" />
      <span className="flex min-w-0 flex-1 items-center gap-2">
        <span className="min-w-0 truncate">{padTitle(pad.content)}</span>
        {pad.tags.slice(0, TAGS_SHOWN).map((tag) => (
          <span className={TAG_CHIP} key={tag}>
            {tag}
          </span>
        ))}
        {pad.tags.length > TAGS_SHOWN && (
          <span className="shrink-0 text-muted-foreground text-xs tabular-nums">
            +{pad.tags.length - TAGS_SHOWN}
          </span>
        )}
      </span>

      {pad.repository !== null && (
        <span className="w-32 shrink-0 truncate text-muted-foreground">
          {projectName(pad.repository)}
        </span>
      )}

      <TimeAgo
        at={pad.updated_at}
        className="shrink-0 text-end text-muted-foreground tabular-nums"
      />
    </Link>
  );
}

function ScratchpadsError(props: ErrorComponentProps) {
  return <OrchestratorError what="scratchpads" {...props} />;
}
