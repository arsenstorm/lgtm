import { Combobox } from "@base-ui/react/combobox";
import type { ErrorComponentProps } from "@tanstack/react-router";
import {
  createFileRoute,
  useNavigate,
  useRouter,
} from "@tanstack/react-router";
import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";

import { LgtmLogo } from "@/components/app-sidebar";
import {
  AiDeveloperIcon,
  ArrowUpIcon,
  CheckIcon,
  CodeBranchIcon,
  ComposeIcon,
  FolderIcon,
  LoaderIcon,
  MagicWandSparkleIcon,
  MsgsIcon,
  PlusIcon,
  SearchIcon,
} from "@/components/icons";
import {
  type LlmModel,
  MODELS,
  ModelSelector,
} from "@/components/model-selector";
import { OrchestratorError } from "@/components/orchestrator-error";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { useWorkspaceChat } from "@/components/workspace-chat";
import {
  createTask,
  enhancePrompt,
  getProjects,
  getRunners,
} from "@/lib/lgtm/server";
import type { ReasoningEffort } from "@/lib/lgtm/types";
import { BARE_CONTROL, cn } from "@/lib/utils";

export const Route = createFileRoute("/")({
  // ?repo= is the whole selection state: a refresh, a shared link and the back
  // button all restore the composer without a second copy of it in React.
  // ?draft= is how a chat hands a brief to task mode.
  validateSearch: (
    search: Record<string, unknown>
  ): { draft?: string; repo?: string } => ({
    repo: typeof search.repo === "string" ? search.repo : undefined,
    draft: typeof search.draft === "string" ? search.draft : undefined,
  }),
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

const MODEL_PREFERENCES_KEY = "lgtm-model-preferences";

const COPY = {
  chat: {
    label: "Message",
    placeholder: "Ask about the workspace, or describe work…",
    send: "Send",
  },
  task: {
    label: "Task prompt",
    placeholder: "Describe your task...",
    send: "Queue task",
  },
} as const;

function isReasoningEffort(value: unknown): value is ReasoningEffort {
  return value === "low" || value === "medium" || value === "high";
}

function readModelPreferences(): {
  configurations: Record<string, ReasoningEffort>;
  model: LlmModel;
} | null {
  try {
    const stored = window.localStorage.getItem(MODEL_PREFERENCES_KEY);
    const parsed = stored ? JSON.parse(stored) : null;
    const selected = MODELS.find(
      (candidate) => candidate.value === parsed?.model
    );
    if (!selected) {
      return null;
    }
    const configurations: Record<string, ReasoningEffort> = {};
    for (const candidate of MODELS) {
      const effort = parsed?.configurations?.[candidate.value];
      if (isReasoningEffort(effort)) {
        configurations[candidate.value] = effort;
      }
    }
    return { configurations, model: selected };
  } catch {
    return null;
  }
}

function saveModelPreferences(
  model: string,
  configurations: Record<string, ReasoningEffort>
) {
  try {
    window.localStorage.setItem(
      MODEL_PREFERENCES_KEY,
      JSON.stringify({ configurations, model })
    );
  } catch {
    // The choices still work for this page when storage is unavailable.
  }
}

function heading(mode: "chat" | "task", project: string | undefined): string {
  if (mode === "chat") {
    return project ? "What should we work on in" : "What should we work on?";
  }
  return project
    ? "What should we build in"
    : "Choose a project to get started";
}

function NewTaskPage() {
  const { projects, runners } = Route.useLoaderData();
  const { repo, draft: handed } = Route.useSearch();
  const navigate = useNavigate();
  const router = useRouter();

  // Repositories come from the projects the orchestrator already numbers;
  // the "general" (repository-less) bucket cannot host a task.
  const repositories = useMemo(
    () => projects.filter((candidate) => candidate.repository !== null),
    [projects]
  );
  const project = repositories.find((p) => p.repository === repo);
  const repository = project?.repository ?? "";
  const projectOptions = useMemo(
    () =>
      repositories.flatMap((candidate) =>
        candidate.repository
          ? [{ label: candidate.name, value: candidate.repository }]
          : []
      ),
    [repositories]
  );

  const [branch, setBranch] = useState("main");
  const [branches, setBranches] = useState(["main"]);
  const [runner, setRunner] = useState<string | null>(null);
  const [model, setModel] = useState<LlmModel>(MODELS[0]);
  const [modelConfigurations, setModelConfigurations] = useState<
    Record<string, ReasoningEffort>
  >({});
  const [draft, setDraft] = useState(handed ?? "");
  const [pending, setPending] = useState<"ask" | "enhance" | "submit" | null>(
    null
  );
  const chat = useWorkspaceChat({
    initialMode: handed ? "task" : "chat",
    pending,
    setPending,
  });
  const { mode } = chat;

  const busy = pending !== null;
  const runnerOptions = useMemo(
    () => [
      { label: "Auto", value: "" },
      ...runners
        .filter((candidate) =>
          candidate.info.executors.includes(model.executor)
        )
        .map((candidate) => ({
          detail: `${candidate.running.length}/${candidate.info.slots}`,
          label: candidate.info.name,
          value: candidate.info.name,
        })),
    ],
    [model.executor, runners]
  );

  useEffect(() => {
    const preferences = readModelPreferences();
    if (preferences) {
      setModel(preferences.model);
      setModelConfigurations(preferences.configurations);
    }
  }, []);

  const changeModelConfiguration = useCallback(
    (value: string, effort: ReasoningEffort) => {
      setModelConfigurations((current) => {
        const next = { ...current, [value]: effort };
        saveModelPreferences(model.value, next);
        return next;
      });
    },
    [model.value]
  );
  const changeModel = useCallback(
    (nextModel: LlmModel) => {
      setModel(nextModel);
      saveModelPreferences(nextModel.value, modelConfigurations);
      if (
        runner &&
        !runners
          .find((candidate) => candidate.info.name === runner)
          ?.info.executors.includes(nextModel.executor)
      ) {
        setRunner(null);
      }
    },
    [modelConfigurations, runner, runners]
  );
  const changeBranch = useCallback((nextBranch: string) => {
    setBranch(nextBranch);
    setBranches((current) =>
      current.includes(nextBranch) ? current : [...current, nextBranch]
    );
  }, []);

  // Picking a project rewrites this page's URL rather than navigating: the
  // draft in the textarea has to survive the change.
  const pick = useCallback(
    (next: string) =>
      navigate({ replace: true, search: { repo: next }, to: "/" }),
    [navigate]
  );
  const changeRunner = useCallback(
    (next: string) => setRunner(next === "" ? null : next),
    []
  );
  const changeDraft = useCallback(
    (event: React.ChangeEvent<HTMLTextAreaElement>) =>
      setDraft(event.currentTarget.value),
    []
  );

  const ask = useCallback(async () => {
    const asked = draft.trim();
    setDraft("");
    const opened = await chat.ask(asked);
    if (opened) {
      await navigate({ params: { id: opened.id }, to: "/chats/$id" });
      await router.invalidate();
    } else {
      setDraft(asked);
    }
  }, [chat, draft, navigate, router]);

  const enhance = useCallback(async () => {
    const prompt = draft.trim();
    if (prompt === "" || repository === "" || busy) {
      return;
    }
    setPending("enhance");
    try {
      const result = await enhancePrompt({
        data: { prompt, repository },
      });
      setDraft(result.prompt);
    } catch (error) {
      // The orchestrator's reason (no runner, no executor) is the whole
      // message; the draft stays untouched either way.
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      setPending(null);
    }
  }, [busy, draft, repository]);

  const submit = useCallback(async () => {
    const prompt = draft.trim();
    if (prompt === "" || repository === "" || busy) {
      return;
    }
    const selectedRunner = runners.find(
      (candidate) => candidate.info.name === runner
    );
    const compatibleRunner = selectedRunner?.info.executors.includes(
      model.executor
    )
      ? runner
      : null;
    setPending("submit");
    try {
      const task = await createTask({
        data: {
          repository,
          base_branch: branch.trim() === "" ? "main" : branch.trim(),
          prompt,
          executor: model.executor,
          model: model.model,
          reasoning_effort: modelConfigurations[model.value] ?? "medium",
          runner: compatibleRunner,
        },
      });
      // The task page already narrates the whole lifecycle; nothing to stage here.
      navigate({ to: "/tasks/$id", params: { id: task.id } });
    } catch (error) {
      toast.error(error instanceof Error ? error.message : String(error));
      setPending(null);
    }
  }, [
    branch,
    busy,
    draft,
    model.executor,
    model.model,
    model.value,
    modelConfigurations,
    navigate,
    repository,
    runner,
    runners,
  ]);
  // Enter sends a chat line the way it does in a task's follow-up box; queuing
  // a task keeps the deliberate Cmd+Enter it always had.
  const submitOnShortcut = useCallback(
    (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (event.key !== "Enter") {
        return;
      }
      if (mode === "chat" && !event.shiftKey) {
        event.preventDefault();
        ask();
      } else if (mode === "task" && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        submit();
      }
    },
    [ask, mode, submit]
  );

  const createTaskButton = (
    <button
      className={cn(BARE_CONTROL, "disabled:opacity-40")}
      disabled={busy}
      onClick={chat.createTask}
      type="button"
    >
      <ComposeIcon className="size-4" />
      Create task
    </button>
  );
  const canSend =
    !busy && draft.trim() !== "" && (mode === "chat" || repository !== "");
  const send = { chat: ask, task: submit }[mode];

  return (
    <div className="mx-auto flex min-h-full w-full max-w-2xl flex-col px-4 py-6 sm:px-6">
      <div className="flex flex-1 flex-col items-center justify-center gap-4 text-center">
        <LgtmLogo className="size-8 text-muted-foreground" />
        <h1 className="font-semibold text-2xl tracking-tight sm:text-3xl">
          {heading(mode, project?.name)}
          {project ? (
            <>
              {" "}
              <CompactSwitcher
                ariaLabel="Select project"
                emptyMessage="No matching projects"
                label={project.name}
                name="heading-project-search"
                onValueChange={pick}
                options={projectOptions}
                placeholder="Search projects…"
                trigger={
                  // The decoration keeps its muted colour; the name dims toward
                  // it and the underline drops away on hover. Chromium animates
                  // text-underline-offset, elsewhere the colour fade carries it.
                  <button
                    className="rounded-sm text-foreground underline decoration-muted-foreground/50 decoration-dashed underline-offset-4 outline-none transition-[color,text-underline-offset] duration-300 hover:text-muted-foreground hover:underline-offset-[7px] focus-visible:ring-2 focus-visible:ring-ring/50"
                    type="button"
                  />
                }
                value={repository}
              />
              ?
            </>
          ) : null}
        </h1>
      </div>

      <div className="sticky bottom-6 mt-auto flex flex-col pt-8">
        {/* The rear pill sits behind the composer and only shows its top edge:
            one step of luminance between page and card, no border. */}
        <div className="mx-3.5 -mb-4 flex flex-wrap items-center gap-3 rounded-t-[18px] bg-foreground/3 px-4 pt-2 pb-6 text-left dark:bg-foreground/4">
          <CompactSwitcher
            ariaLabel="Select project"
            emptyMessage="No matching projects"
            label={project ? project.name : "Choose project"}
            name="project-search"
            onValueChange={pick}
            options={projectOptions}
            placeholder="Search projects…"
            trigger={<Control icon={FolderIcon} />}
            value={repository}
          />

          {mode === "task" ? (
            <>
              <CompactSwitcher
                ariaLabel="Select runner"
                emptyMessage="No matching runners"
                label={runner ?? "Auto"}
                name="runner-search"
                onValueChange={changeRunner}
                options={runnerOptions}
                placeholder="Search runners…"
                trigger={<Control icon={AiDeveloperIcon} />}
                value={runner ?? ""}
              />

              <BranchSwitcher
                branches={branches}
                onValueChange={changeBranch}
                value={branch}
              />
            </>
          ) : null}
        </div>

        {/* In light mode --card equals --background, so this hairline is the
            only thing separating the composer from the page. */}
        <div className="relative flex flex-col rounded-[18px] border border-foreground/6 bg-card transition-colors focus-within:border-foreground/15">
          <Textarea
            aria-label={COPY[mode].label}
            className="max-h-64 min-h-20 resize-none border-0 bg-transparent px-3 py-3 shadow-none placeholder:text-muted-foreground/60 focus-visible:ring-0 dark:bg-transparent"
            onChange={changeDraft}
            onKeyDown={submitOnShortcut}
            placeholder={COPY[mode].placeholder}
            value={draft}
          />

          <div className="flex flex-wrap items-center gap-3 px-4 pb-4">
            {mode === "chat" ? (
              createTaskButton
            ) : (
              <>
                <button
                  className={BARE_CONTROL}
                  disabled={busy}
                  onClick={chat.backToChat}
                  type="button"
                >
                  <MsgsIcon className="size-4" />
                  Chat
                </button>
                <button
                  className={cn(BARE_CONTROL, "disabled:opacity-40")}
                  disabled={busy || draft.trim() === "" || repository === ""}
                  onClick={enhance}
                  type="button"
                >
                  {pending === "enhance" ? (
                    <LoaderIcon className="size-4 animate-spin" />
                  ) : (
                    <MagicWandSparkleIcon className="size-4" />
                  )}
                  Enhance
                </button>
              </>
            )}

            <div className="ml-auto flex min-w-0 items-center gap-2">
              {mode === "task" ? (
                <ModelSelector
                  configurations={modelConfigurations}
                  disabled={busy}
                  model={model}
                  onConfigurationChange={changeModelConfiguration}
                  onModelChange={changeModel}
                />
              ) : null}

              <Button
                aria-label={COPY[mode].send}
                className="size-8 shrink-0 rounded-full bg-foreground text-background hover:bg-foreground/90 disabled:opacity-40"
                disabled={!canSend}
                onClick={send}
                size="icon"
              >
                {pending === "ask" || pending === "submit" ? (
                  <LoaderIcon className="animate-spin" />
                ) : (
                  <ArrowUpIcon />
                )}
              </Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function BranchSwitcher({
  branches,
  onValueChange,
  value,
}: {
  branches: string[];
  onValueChange: (branch: string) => void;
  value: string;
}) {
  const [inputValue, setInputValue] = useState("");
  const [open, setOpen] = useState(false);
  const customBranch = inputValue.trim();
  const canUseCustomBranch =
    customBranch !== "" && !branches.includes(customBranch);
  const sortedBranches = useMemo(
    () => [...branches].sort((a, b) => a.localeCompare(b)),
    [branches]
  );

  const selectBranch = useCallback(
    (branch: string) => {
      onValueChange(branch);
      setInputValue("");
      setOpen(false);
    },
    [onValueChange]
  );
  const handleValueChange = useCallback(
    (branch: string | null) => {
      if (branch) {
        selectBranch(branch);
      }
    },
    [selectBranch]
  );
  const handleOpenChange = useCallback((nextOpen: boolean) => {
    setInputValue("");
    setOpen(nextOpen);
  }, []);
  const selectCustomBranch = useCallback(
    () => selectBranch(customBranch),
    [customBranch, selectBranch]
  );

  return (
    <Combobox.Root<string>
      autoHighlight
      inputValue={inputValue}
      items={sortedBranches}
      onInputValueChange={setInputValue}
      onOpenChange={handleOpenChange}
      onValueChange={handleValueChange}
      open={open}
      value={value}
    >
      <Combobox.Trigger
        aria-label="Select base branch"
        render={<Control icon={CodeBranchIcon} />}
      >
        {value}
      </Combobox.Trigger>
      <Combobox.Portal>
        <Combobox.Positioner
          align="center"
          className="z-50"
          side="top"
          sideOffset={6}
        >
          <Combobox.Popup className="w-[min(16rem,var(--available-width))] overflow-hidden rounded-lg bg-popover text-popover-foreground shadow-md outline-none ring-1 ring-foreground/10 dark:shadow-none">
            <Combobox.InputGroup className="flex items-center gap-2 border-foreground/10 border-b px-3">
              <SearchIcon
                aria-hidden="true"
                className="size-5 shrink-0 text-muted-foreground sm:size-4"
              />
              <Combobox.Input
                aria-label="Search branches"
                autoComplete="off"
                autoFocus
                className="h-11 min-w-0 flex-1 bg-transparent text-base outline-none placeholder:text-muted-foreground sm:h-9 sm:text-sm"
                name="branch-search"
                placeholder="Search branches…"
                spellCheck={false}
              />
            </Combobox.InputGroup>
            <Combobox.List className="max-h-44 overflow-y-auto">
              {(branch: string) => (
                <Combobox.Item
                  className="m-1 flex cursor-default items-center gap-2 rounded-md px-2 py-2 text-base outline-none data-highlighted:bg-accent sm:py-1.5 sm:text-sm"
                  key={branch}
                  value={branch}
                >
                  <span className="min-w-0 flex-1 truncate">{branch}</span>
                  {branch === value ? (
                    <CheckIcon
                      aria-hidden="true"
                      className="size-5 shrink-0 sm:size-4"
                    />
                  ) : null}
                </Combobox.Item>
              )}
            </Combobox.List>
            {canUseCustomBranch ? (
              <div className="border-foreground/10 border-t p-1">
                <button
                  className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-base outline-none hover:bg-accent focus-visible:bg-accent sm:py-1.5 sm:text-sm"
                  onClick={selectCustomBranch}
                  type="button"
                >
                  <PlusIcon
                    aria-hidden="true"
                    className="size-5 shrink-0 text-muted-foreground sm:size-4"
                  />
                  <span className="min-w-0 truncate">
                    Use “{customBranch}” as base
                  </span>
                </button>
              </div>
            ) : (
              <Combobox.Empty>
                <p className="px-3 py-2 text-base text-muted-foreground sm:text-sm">
                  No matching branches
                </p>
              </Combobox.Empty>
            )}
          </Combobox.Popup>
        </Combobox.Positioner>
      </Combobox.Portal>
    </Combobox.Root>
  );
}

interface SwitcherOption {
  detail?: string;
  label: string;
  value: string;
}

function isSameOption(item: SwitcherOption, value: SwitcherOption): boolean {
  return item.value === value.value;
}

function CompactSwitcher({
  ariaLabel,
  emptyMessage,
  label,
  name,
  onValueChange,
  options,
  placeholder,
  trigger,
  value,
}: {
  ariaLabel: string;
  emptyMessage: string;
  label: string;
  name: string;
  onValueChange: (value: string) => void;
  options: SwitcherOption[];
  placeholder: string;
  trigger: React.ReactElement;
  value: string;
}) {
  const [inputValue, setInputValue] = useState("");
  const [open, setOpen] = useState(false);
  const selected = options.find((option) => option.value === value) ?? null;
  const handleOpenChange = useCallback((nextOpen: boolean) => {
    setInputValue("");
    setOpen(nextOpen);
  }, []);
  const handleValueChange = useCallback(
    (option: SwitcherOption | null) => {
      if (option) {
        onValueChange(option.value);
        setOpen(false);
      }
    },
    [onValueChange]
  );

  return (
    <Combobox.Root<SwitcherOption>
      autoHighlight
      inputValue={inputValue}
      isItemEqualToValue={isSameOption}
      items={options}
      onInputValueChange={setInputValue}
      onOpenChange={handleOpenChange}
      onValueChange={handleValueChange}
      open={open}
      value={selected}
    >
      <Combobox.Trigger aria-label={ariaLabel} render={trigger}>
        {label}
      </Combobox.Trigger>
      <Combobox.Portal>
        <Combobox.Positioner
          align="center"
          className="z-50"
          side="top"
          sideOffset={6}
        >
          <Combobox.Popup className="w-[min(16rem,var(--available-width))] overflow-hidden rounded-lg bg-popover text-popover-foreground shadow-md outline-none ring-1 ring-foreground/10 dark:shadow-none">
            <Combobox.InputGroup className="flex items-center gap-2 border-foreground/10 border-b px-3">
              <SearchIcon
                aria-hidden="true"
                className="size-5 shrink-0 text-muted-foreground sm:size-4"
              />
              <Combobox.Input
                aria-label={placeholder}
                autoComplete="off"
                autoFocus
                className="h-11 min-w-0 flex-1 bg-transparent text-base outline-none placeholder:text-muted-foreground sm:h-9 sm:text-sm"
                name={name}
                placeholder={placeholder}
                spellCheck={false}
              />
            </Combobox.InputGroup>
            <Combobox.List className="max-h-44 overflow-y-auto">
              {(option: SwitcherOption) => (
                <Combobox.Item
                  className="m-1 flex cursor-default items-center gap-2 rounded-md px-2 py-2 text-base outline-none data-highlighted:bg-accent sm:py-1.5 sm:text-sm"
                  key={option.value}
                  value={option}
                >
                  <span className="min-w-0 flex-1 truncate">
                    {option.label}
                  </span>
                  {option.detail ? (
                    <div className="shrink-0 text-muted-foreground text-sm tabular-nums sm:text-xs">
                      {option.detail}
                    </div>
                  ) : null}
                  {option.value === value ? (
                    <CheckIcon
                      aria-hidden="true"
                      className="size-5 shrink-0 sm:size-4"
                    />
                  ) : null}
                </Combobox.Item>
              )}
            </Combobox.List>
            <Combobox.Empty>
              <p className="px-3 py-2 text-base text-muted-foreground sm:text-sm">
                {emptyMessage}
              </p>
            </Combobox.Empty>
          </Combobox.Popup>
        </Combobox.Positioner>
      </Combobox.Portal>
    </Combobox.Root>
  );
}

function Control({
  icon: ControlIcon,
  className,
  children,
  ...props
}: {
  children?: React.ReactNode;
  icon: React.ComponentType<{
    className?: string;
    "aria-hidden"?: boolean | "true" | "false";
  }>;
} & React.ComponentProps<"button">) {
  return (
    <button
      className={cn(BARE_CONTROL, "min-w-0 max-w-full", className)}
      type="button"
      {...props}
    >
      <ControlIcon aria-hidden="true" className="size-4 shrink-0" />
      <span className="truncate">{children}</span>
    </button>
  );
}

function NewTaskError(props: ErrorComponentProps) {
  return <OrchestratorError what="the new task page" {...props} />;
}
