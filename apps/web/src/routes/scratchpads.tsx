import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { toast } from "sonner";

import { LoaderIcon, PlusIcon } from "@/components/icons";
import { ListGroup } from "@/components/list-group";
import { OrchestratorError } from "@/components/orchestrator-error";
import { PageHeading } from "@/components/page-heading";
import { TAG_CHIP } from "@/components/tags-row";
import { TimeAgo } from "@/components/time-ago";
import { Button } from "@/components/ui/button";
import { createScratchpad, getScratchpads } from "@/lib/lgtm/server";
import type { Scratchpad } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";
import { groupByRepository } from "@/routes/memories";

export const Route = createFileRoute("/scratchpads")({
  loader: async () => ({ scratchpads: await getScratchpads() }),
  component: ScratchpadsPage,
  errorComponent: ScratchpadsError,
});

/** "09-05-09-35 Scratchpad": the moment it was made, in the browser's clock,
 *  so a page of fresh documents still reads in order. */
function defaultTitle(now = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(now.getMonth() + 1)}-${pad(now.getDate())}-${pad(now.getHours())}-${pad(now.getMinutes())} Scratchpad`;
}

const byEdited = (a: Scratchpad, b: Scratchpad) => b.updated_at - a.updated_at;

/** How many tags a row shows before the rest collapse into a count. */
const TAGS_SHOWN = 3;

function ScratchpadsPage() {
  const { scratchpads } = Route.useLoaderData();
  const navigate = useNavigate();
  const [creating, setCreating] = useState(false);

  const live = scratchpads.filter((pad) => !pad.archived);
  const archived = scratchpads.filter((pad) => pad.archived).sort(byEdited);
  const groups = groupByRepository(live, byEdited);

  async function create() {
    setCreating(true);
    try {
      const pad = await createScratchpad({
        data: { content: "", title: defaultTitle() },
      });
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
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-6 px-4 py-6 sm:px-6 lg:px-8">
      <PageHeading meta={live.length} title="Scratchpads">
        <Button disabled={creating} onClick={create} size="lg">
          {creating ? (
            <LoaderIcon
              className="motion-safe:animate-spin"
              data-icon="inline-start"
            />
          ) : (
            <PlusIcon data-icon="inline-start" />
          )}
          New scratchpad
        </Button>
      </PageHeading>

      {scratchpads.length === 0 ? (
        <p className="text-muted-foreground text-sm">
          No scratchpads yet. New scratchpad starts one.
        </p>
      ) : (
        <div className="flex flex-col gap-1">
          {groups.map((entry) => (
            <ScratchpadGroup
              key={entry.key}
              label={entry.label}
              pads={entry.items}
            />
          ))}
          {archived.length > 0 && (
            <ScratchpadGroup label="Archived" pads={archived} />
          )}
        </div>
      )}
    </div>
  );
}

function ScratchpadGroup({
  label,
  pads,
}: {
  label: string;
  pads: Scratchpad[];
}) {
  return (
    <ListGroup count={pads.length} label={label}>
      <ul className="flex flex-col py-1">
        {pads.map((pad) => (
          <li key={pad.id}>
            <ScratchpadRow pad={pad} />
          </li>
        ))}
      </ul>
    </ListGroup>
  );
}

function ScratchpadRow({ pad }: { pad: Scratchpad }) {
  return (
    <Link
      className="flex items-center gap-3 rounded-md py-2.5 pr-2 pl-7 text-sm hover:bg-foreground/4"
      params={{ id: pad.id }}
      to="/scratchpads/$id"
    >
      <span className="flex min-w-0 flex-1 items-center gap-2">
        <span className="min-w-0 truncate">{pad.title}</span>
        {pad.tags.slice(0, TAGS_SHOWN).map((tag) => (
          <span className={cn(TAG_CHIP, "max-w-40")} key={tag}>
            <span className="truncate">{tag}</span>
          </span>
        ))}
        {pad.tags.length > TAGS_SHOWN && (
          <span className="shrink-0 text-muted-foreground text-xs tabular-nums">
            +{pad.tags.length - TAGS_SHOWN}
          </span>
        )}
      </span>

      <TimeAgo
        at={pad.updated_at}
        className="w-16 shrink-0 truncate text-end text-muted-foreground tabular-nums"
      />
    </Link>
  );
}

function ScratchpadsError(props: ErrorComponentProps) {
  return <OrchestratorError what="scratchpads" {...props} />;
}
