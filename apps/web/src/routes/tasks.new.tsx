import type { Icon } from "@phosphor-icons/react";
import {
  ArrowUp,
  CaretDown,
  CircleNotch,
  FolderSimple,
  GitBranch,
  HardDrives,
  Sparkle,
} from "@phosphor-icons/react";
import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { toast } from "sonner";

import { LgtmLogo } from "@/components/app-sidebar";
import { OrchestratorError } from "@/components/orchestrator-error";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Textarea } from "@/components/ui/textarea";
import {
  createTask,
  enhancePrompt,
  getProjects,
  getRunners,
} from "@/lib/lgtm/server";
import type { Executor } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/tasks/new")({
  loader: async () => {
    const [projects, runners] = await Promise.all([
      getProjects(),
      getRunners(),
    ]);
    return { projects, runners };
  },
  component: NewTaskPage,
  errorComponent: NewTaskError,
});

// Every control in the composer is bare: the only chrome is the text going
// from muted to foreground, so focus-visible has to carry the ring alone.
const CONTROL =
  "flex items-center gap-1.5 rounded-sm text-muted-foreground text-sm outline-none transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50";

const EXECUTORS: { value: Executor; label: string }[] = [
  { value: "claude", label: "Claude" },
  { value: "codex", label: "Codex" },
];

function NewTaskPage() {
  const { projects, runners } = Route.useLoaderData();
  const navigate = useNavigate();

  // Repositories come from the projects the orchestrator already numbers;
  // the "general" (repository-less) bucket cannot host a task.
  const repositories = projects.filter((p) => p.repository !== null);
  const [repository, setRepository] = useState(
    repositories[0]?.repository ?? ""
  );
  const [branch, setBranch] = useState("main");
  const [editingBranch, setEditingBranch] = useState(false);
  const [runner, setRunner] = useState<string | null>(null);
  const [executor, setExecutor] = useState<Executor>("claude");
  const [draft, setDraft] = useState("");
  const [pending, setPending] = useState<"enhance" | "submit" | null>(null);

  const project = repositories.find((p) => p.repository === repository);
  const busy = pending !== null;

  async function enhance() {
    const prompt = draft.trim();
    if (prompt === "" || busy) {
      return;
    }
    setPending("enhance");
    try {
      const result = await enhancePrompt({
        data: { prompt, repository: repository || undefined },
      });
      setDraft(result.prompt);
    } catch (error) {
      // The orchestrator's reason (no runner, no executor) is the whole
      // message; the draft stays untouched either way.
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      setPending(null);
    }
  }

  async function submit() {
    const prompt = draft.trim();
    if (prompt === "" || repository === "" || busy) {
      return;
    }
    setPending("submit");
    try {
      const task = await createTask({
        data: {
          repository,
          base_branch: branch.trim() === "" ? "main" : branch.trim(),
          prompt,
          executor,
          runner,
        },
      });
      // The task page already narrates the whole lifecycle; nothing to stage here.
      navigate({ to: "/tasks/$id", params: { id: task.id } });
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
      setPending(null);
    }
  }

  return (
    <div className="mx-auto flex min-h-full w-full max-w-2xl flex-col px-4 py-6 sm:px-6">
      <div className="flex flex-1 flex-col items-center justify-center gap-4 text-center">
        <LgtmLogo className="size-8 text-muted-foreground" />
        <h1 className="font-semibold text-2xl tracking-tight sm:text-3xl">
          What should we build in{" "}
          <RepositoryPicker
            label={project ? project.name : "a repository"}
            onPick={setRepository}
            repositories={repositories}
            trigger={
              // The decoration keeps its muted colour; the name dims toward it
              // and the underline drops away on hover. Chromium animates
              // text-underline-offset, elsewhere the colour fade carries it.
              <button
                className="rounded-sm text-foreground underline decoration-muted-foreground/50 decoration-dashed underline-offset-4 outline-none transition-[color,text-underline-offset] duration-300 hover:text-muted-foreground hover:underline-offset-[7px] focus-visible:ring-2 focus-visible:ring-ring/50"
                type="button"
              />
            }
          />
          ?
        </h1>
      </div>

      <div className="sticky bottom-6 mt-8 flex flex-col">
        {/* The rear pill sits behind the composer and only shows its top edge:
            one step of luminance between page and card, no border. */}
        <RepositoryPicker
          label={project ? project.name : "repository"}
          onPick={setRepository}
          repositories={repositories}
          trigger={
            <Control
              className="mx-3.5 -mb-4 rounded-t-[18px] bg-foreground/3 px-4 pt-2 pb-6 text-left dark:bg-foreground/4"
              icon={FolderSimple}
            />
          }
        />

        {/* In light mode --card equals --background, so this hairline is the
            only thing separating the composer from the page. */}
        <div className="relative flex flex-col rounded-[18px] border border-foreground/6 bg-card transition-colors focus-within:border-foreground/15">
          <Textarea
            aria-label="Task prompt"
            className="max-h-64 min-h-20 resize-none border-0 bg-transparent px-3 py-3 shadow-none placeholder:text-muted-foreground/60 focus-visible:ring-0 dark:bg-transparent"
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                event.preventDefault();
                submit();
              }
            }}
            placeholder="Describe your task..."
            value={draft}
          />

          <div className="flex items-center gap-3 px-4 pb-4">
            <button
              className={cn(CONTROL, "disabled:opacity-40")}
              disabled={busy || draft.trim() === ""}
              onClick={enhance}
              type="button"
            >
              {pending === "enhance" ? (
                <CircleNotch className="size-4 animate-spin" />
              ) : (
                <Sparkle className="size-4" />
              )}
              Enhance
            </button>

            <div aria-hidden="true" className="h-4 w-px bg-border/60" />

            <DropdownMenu>
              <DropdownMenuTrigger render={<Control icon={HardDrives} />}>
                {runner ?? "Any runner"}
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start">
                <DropdownMenuItem onClick={() => setRunner(null)}>
                  Any runner
                </DropdownMenuItem>
                {runners.map((r) => (
                  <DropdownMenuItem
                    key={r.info.name}
                    onClick={() => setRunner(r.info.name)}
                  >
                    {r.info.name}
                    <span className="ml-auto text-muted-foreground text-xs">
                      {r.running.length}/{r.info.slots}
                    </span>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>

            {editingBranch ? (
              <input
                autoFocus
                className="w-28 rounded-sm bg-transparent font-mono text-foreground text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                defaultValue={branch}
                onBlur={() => setEditingBranch(false)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    setBranch(event.currentTarget.value);
                    setEditingBranch(false);
                  }
                  if (event.key === "Escape") {
                    setEditingBranch(false);
                  }
                }}
              />
            ) : (
              <Control icon={GitBranch} onClick={() => setEditingBranch(true)}>
                {branch}
              </Control>
            )}

            <div className="flex-1" />

            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <button
                    className="flex items-center gap-1 rounded-sm font-medium text-foreground text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                    type="button"
                  />
                }
              >
                {EXECUTORS.find((e) => e.value === executor)?.label}
                <CaretDown className="size-3 text-muted-foreground" />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                {EXECUTORS.map(({ value, label }) => (
                  <DropdownMenuItem
                    key={value}
                    onClick={() => setExecutor(value)}
                  >
                    {label}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>

            <Button
              aria-label="Queue task"
              className="ml-1 size-8 rounded-full bg-foreground text-background hover:bg-foreground/90 disabled:opacity-40"
              disabled={busy || draft.trim() === "" || repository === ""}
              onClick={submit}
              size="icon"
            >
              {pending === "submit" ? (
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

function RepositoryPicker({
  repositories,
  label,
  onPick,
  trigger,
}: {
  label: string;
  onPick: (repository: string) => void;
  repositories: { id: string; name: string; repository: string | null }[];
  trigger: React.ReactElement;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger render={trigger}>{label}</DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        {repositories.length === 0 ? (
          <DropdownMenuItem disabled>
            No repositories yet — run a task from the CLI first
          </DropdownMenuItem>
        ) : (
          repositories.map((p) => (
            <DropdownMenuItem
              key={p.id}
              onClick={() => p.repository && onPick(p.repository)}
            >
              {p.name}
            </DropdownMenuItem>
          ))
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function Control({
  icon: ControlIcon,
  className,
  children,
  ...props
}: {
  children?: React.ReactNode;
  icon: Icon;
} & React.ComponentProps<"button">) {
  return (
    <button className={cn(CONTROL, className)} type="button" {...props}>
      <ControlIcon aria-hidden="true" className="size-4 shrink-0" />
      {children}
    </button>
  );
}

function NewTaskError(props: ErrorComponentProps) {
  return <OrchestratorError what="the new task page" {...props} />;
}
