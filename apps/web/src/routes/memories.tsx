import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute } from "@tanstack/react-router";
import { useState } from "react";

import { ActionIcon } from "@/components/action-icon";
import { projectName } from "@/components/app-sidebar";
import { CheckIcon, PencilIcon, TrashIcon } from "@/components/icons";
import { OrchestratorError } from "@/components/orchestrator-error";
import { PageHeading } from "@/components/page-heading";
import { TimeAgo } from "@/components/time-ago";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { useAction } from "@/hooks/use-action";
import { ARMED_CLASS, useArmedConfirm } from "@/hooks/use-armed-confirm";
import {
  approveMemory,
  deleteMemory,
  getMemories,
  updateMemory,
} from "@/lib/lgtm/server";
import type { Memory } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/memories")({
  loader: async () => ({ memories: await getMemories() }),
  component: MemoriesPage,
  errorComponent: MemoriesError,
});

interface Group {
  key: string;
  label: string;
  memories: Memory[];
}

function group(memories: Memory[]): Group[] {
  const byRepository = new Map<string | null, Memory[]>();
  for (const memory of memories) {
    const bucket = byRepository.get(memory.repository);
    if (bucket) {
      bucket.push(memory);
    } else {
      byRepository.set(memory.repository, [memory]);
    }
  }

  return (
    [...byRepository]
      .map(([repository, list]) => ({
        key: repository ?? "",
        label:
          repository === null ? "Every repository" : projectName(repository),
        memories: [...list].sort((a, b) => b.created_at - a.created_at),
      }))
      // Memories with no repository apply everywhere, so they lead.
      .sort(
        (a, b) =>
          Number(!!a.key) - Number(!!b.key) || a.label.localeCompare(b.label)
      )
  );
}

function MemoriesPage() {
  const { memories } = Route.useLoaderData();
  const groups = group(memories);

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <PageHeading meta={memories.length} title="Memories" />

      {groups.length === 0 ? (
        <p className="text-muted-foreground text-sm">No memories yet.</p>
      ) : (
        groups.map((entry) => (
          <section className="flex flex-col gap-2" key={entry.key}>
            <h2 className="truncate font-medium text-muted-foreground text-sm">
              {entry.label}
            </h2>
            <ul className="-mx-2 divide-y divide-foreground/5" role="list">
              {entry.memories.map((memory) => (
                <li key={memory.id}>
                  <MemoryRow memory={memory} />
                </li>
              ))}
            </ul>
          </section>
        ))
      )}
    </div>
  );
}

type Action = "save" | "delete" | "approve";

function MemoryRow({ memory }: { memory: Memory }) {
  // null means "not editing" — an empty draft is a distinct, valid state.
  const [draft, setDraft] = useState<string | null>(null);
  const { armed, arm, disarm, ref: deleteRef } = useArmedConfirm();
  const { pending, busy, run } = useAction<Action>({ onStart: disarm });

  const proposed = memory.verification === "agent_proposed";

  const edited = (draft ?? "").trim();

  async function save() {
    if (!edited || edited === memory.content) {
      return;
    }
    const saved = await run(
      "save",
      () => updateMemory({ data: { id: memory.id, content: edited } }),
      // The orchestrator treats an edit as sign-off, so say so.
      proposed ? "Memory updated and approved" : "Memory updated"
    );
    if (saved) {
      setDraft(null);
    }
  }

  if (draft !== null) {
    return (
      <div className="flex items-start gap-3 px-2 py-2.5 text-sm">
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <Textarea
            aria-label="Memory content"
            autoFocus
            disabled={busy}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                setDraft(null);
              }
            }}
            value={draft}
          />
          <div className="flex items-center gap-2">
            <Button
              disabled={busy || !edited || edited === memory.content}
              onClick={save}
              size="sm"
            >
              <ActionIcon busy={pending === "save"} icon={CheckIcon} />
              Save
            </Button>
            <Button
              disabled={busy}
              onClick={() => setDraft(null)}
              size="sm"
              variant="ghost"
            >
              Cancel
            </Button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="group/row flex items-start gap-3 px-2 py-2.5 text-sm">
      <p className="min-w-0 flex-1 text-pretty">{memory.content}</p>

      {/* Always in the flow so revealing them cannot reflow the row. */}
      <div className="flex shrink-0 items-center gap-0.5 opacity-0 pointer-coarse:opacity-100 transition-opacity group-focus-within/row:opacity-100 group-hover/row:opacity-100">
        {proposed && (
          <Button
            aria-label="Approve memory"
            className="text-muted-foreground"
            disabled={busy}
            onClick={() =>
              run(
                "approve",
                () => approveMemory({ data: memory.id }),
                "Memory approved"
              )
            }
            size="icon-sm"
            variant="ghost"
          >
            <ActionIcon busy={pending === "approve"} icon={CheckIcon} />
          </Button>
        )}
        <Button
          aria-label="Edit memory"
          className="text-muted-foreground"
          disabled={busy}
          onClick={() => setDraft(memory.content)}
          size="icon-sm"
          variant="ghost"
        >
          <PencilIcon />
        </Button>
        <Button
          aria-label={armed ? "Confirm delete memory" : "Delete memory"}
          className={cn(armed ? ARMED_CLASS : "text-muted-foreground")}
          disabled={busy}
          onClick={() =>
            armed
              ? run(
                  "delete",
                  () => deleteMemory({ data: memory.id }),
                  "Memory deleted"
                )
              : arm()
          }
          ref={deleteRef}
          size={armed ? "sm" : "icon-sm"}
          variant={armed ? "destructive" : "ghost"}
        >
          <ActionIcon busy={pending === "delete"} icon={TrashIcon} />
          {armed && "Confirm delete"}
        </Button>
      </div>

      {/* Approved is the boring default; only a proposal needs saying. */}
      {memory.verification === "agent_proposed" && (
        <Badge
          className="border-amber-600/30 text-amber-700 dark:text-amber-400"
          variant="outline"
        >
          proposed
        </Badge>
      )}

      {memory.source === "agent" && (
        <span className="shrink-0 text-muted-foreground text-xs">agent</span>
      )}

      <TimeAgo
        at={memory.created_at}
        className="w-16 shrink-0 truncate text-end text-muted-foreground tabular-nums"
      />
    </div>
  );
}

function MemoriesError(props: ErrorComponentProps) {
  return <OrchestratorError what="memories" {...props} />;
}
