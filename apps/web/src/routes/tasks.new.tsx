import type { Icon } from "@phosphor-icons/react";
import {
  ArrowUp,
  Binoculars,
  Bug,
  CaretDown,
  CircleNotch,
  FolderSimple,
  GitBranch,
  HardDrives,
  ListChecks,
  Sparkle,
  Wrench,
} from "@phosphor-icons/react";
import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useRef, useState } from "react";
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

const SEEDS: { title: string; icon: Icon; tone: string; seed: string }[] = [
  {
    title: "Explore and understand code",
    icon: Binoculars,
    tone: "text-sky-600 dark:text-sky-400",
    seed: "Explore and explain how ",
  },
  {
    title: "Build a new feature, app, or tool",
    icon: Wrench,
    tone: "text-violet-600 dark:text-violet-400",
    seed: "Build ",
  },
  {
    title: "Review code and suggest changes",
    icon: ListChecks,
    tone: "text-emerald-600 dark:text-emerald-400",
    seed: "Review ",
  },
  {
    title: "Fix issues and failures",
    icon: Bug,
    tone: "text-orange-600 dark:text-orange-400",
    seed: "Fix ",
  },
];

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
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const project = repositories.find((p) => p.repository === repository);
  const busy = pending !== null;

  function seedDraft(seed: string) {
    setDraft((current) => (current.trim() === "" ? seed : current));
    textareaRef.current?.focus();
  }

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
      <div className="flex flex-1 flex-col items-center justify-center gap-8">
        <div className="flex flex-col items-center gap-4 text-center">
          <LgtmLogo className="size-8 text-muted-foreground" />
          <h1 className="font-semibold text-2xl tracking-tight sm:text-3xl">
            What should we build in{" "}
            <RepositoryPicker
              label={project ? project.name : "a repository"}
              onPick={setRepository}
              repositories={repositories}
              trigger={
                <button
                  className="rounded-sm underline decoration-muted-foreground/50 decoration-dashed underline-offset-4 outline-none hover:decoration-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
                  type="button"
                />
              }
            />
            ?
          </h1>
        </div>

        <div className="grid w-full grid-cols-2 gap-3 sm:grid-cols-4">
          {SEEDS.map(({ title, icon: SeedIcon, tone, seed }) => (
            <button
              className="flex flex-col gap-2 rounded-xl border p-3 text-left text-sm outline-none transition-colors hover:bg-foreground/4 focus-visible:ring-2 focus-visible:ring-ring/50"
              key={title}
              onClick={() => seedDraft(seed)}
              type="button"
            >
              <SeedIcon aria-hidden="true" className={cn("size-4", tone)} />
              <span className="text-pretty">{title}</span>
            </button>
          ))}
        </div>
      </div>

      <div className="sticky bottom-6 mt-8 flex flex-col rounded-2xl border bg-card shadow-sm focus-within:ring-2 focus-within:ring-ring/30">
        <div className="flex flex-wrap items-center gap-1 border-b px-2 py-1.5">
          <RepositoryPicker
            label={project ? project.name : "repository"}
            onPick={setRepository}
            repositories={repositories}
            trigger={<Chip icon={FolderSimple} />}
          />
          <DropdownMenu>
            <DropdownMenuTrigger render={<Chip icon={HardDrives} />}>
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
              className="h-6 w-28 rounded-md border bg-transparent px-1.5 font-mono text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
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
            <Chip icon={GitBranch} onClick={() => setEditingBranch(true)}>
              {branch}
            </Chip>
          )}
        </div>

        <Textarea
          aria-label="Task prompt"
          className="max-h-64 min-h-20 resize-none border-0 bg-transparent shadow-none focus-visible:ring-0"
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
              event.preventDefault();
              submit();
            }
          }}
          placeholder="Do anything"
          ref={textareaRef}
          value={draft}
        />

        <div className="flex items-center gap-2 px-2 pb-2">
          <Button
            className="text-muted-foreground"
            disabled={busy || draft.trim() === ""}
            onClick={enhance}
            size="sm"
            variant="ghost"
          >
            {pending === "enhance" ? (
              <CircleNotch className="animate-spin" />
            ) : (
              <Sparkle />
            )}
            Enhance
          </Button>
          <div className="ml-auto flex items-center gap-2">
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button
                    className="text-muted-foreground"
                    size="sm"
                    variant="ghost"
                  />
                }
              >
                {EXECUTORS.find((e) => e.value === executor)?.label}
                <CaretDown className="size-3" />
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
              className="size-8 rounded-full"
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

function Chip({
  icon: ChipIcon,
  onClick,
  children,
  ...props
}: {
  children?: React.ReactNode;
  icon: Icon;
  onClick?: () => void;
} & React.ComponentProps<"button">) {
  return (
    <button
      className="flex h-6 items-center gap-1.5 rounded-md px-1.5 text-muted-foreground text-xs outline-none transition-colors hover:bg-foreground/6 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
      onClick={onClick}
      type="button"
      {...props}
    >
      <ChipIcon aria-hidden="true" className="size-3.5" />
      {children}
    </button>
  );
}

function NewTaskError(props: ErrorComponentProps) {
  return <OrchestratorError what="the new task page" {...props} />;
}
