import { Link, useMatchRoute } from "@tanstack/react-router";
import type { ComponentProps, FormEvent } from "react";
import { useCallback, useEffect, useId, useState } from "react";
import { toast } from "sonner";

import { AccountMenu } from "@/components/account-menu";
import { ChatItem } from "@/components/chat-item";
import {
  ActivityIcon,
  AiDeveloperIcon,
  BrainSparkleIcon,
  ChevronIcon,
  ComposeIcon,
  DotsIcon,
  ListCheckboxIcon,
  MsgsIcon,
  NotesIcon,
  PlusIcon,
  TasksIcon,
} from "@/components/icons";
import type { Project } from "@/components/project-item";
import { ProjectItem } from "@/components/project-item";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupAction,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from "@/components/ui/sidebar";
import { createProject, getProjects } from "@/lib/lgtm/server";
import type { Chat, Project as ProjectRecord, Task } from "@/lib/lgtm/types";

const NAV = [
  { exact: true, icon: TasksIcon, label: "Tasks", to: "/tasks" },
  { exact: false, icon: AiDeveloperIcon, label: "Runners", to: "/runners" },
  { exact: false, icon: ListCheckboxIcon, label: "Todos", to: "/todos" },
  { exact: false, icon: BrainSparkleIcon, label: "Memories", to: "/memories" },
  { exact: false, icon: NotesIcon, label: "Scratchpads", to: "/scratchpads" },
] as const;

// The second tier: worth reaching, not worth a permanent row.
const MORE = [
  { icon: MsgsIcon, label: "Sessions", to: "/sessions" },
  { icon: ActivityIcon, label: "Activity", to: "/activity" },
] as const;

const TRAILING_SLASHES = /\/+$/;
const DOT_GIT = /\.git$/;
const REPOSITORY_SEPARATOR = /[/:]/;

const PROJECTS_OPEN_KEY = "lgtm-projects-open";
// ponytail: the newest eight; a full list gets its own page when threads
// pile up.
const RECENT_CHATS = 8;

// Group headings share the projects' open map; no repository url starts with
// a colon, so these keys cannot collide with one.
const PROJECTS_GROUP = ":projects";
const CHATS_GROUP = ":chats";

// The caret is row furniture: out of the way until the heading is reached,
// always on where there is no hover to reach it with.
const GROUP_CARET =
  "ml-1 text-muted-foreground opacity-0 transition-[opacity,transform] duration-200 pointer-coarse:opacity-100 group-focus-within/group:opacity-100 group-hover/group:opacity-100 group-data-[panel-open]/label:rotate-90";

type OpenMap = Record<string, boolean>;

function readOpenMap(): OpenMap | null {
  try {
    const stored = window.localStorage.getItem(PROJECTS_OPEN_KEY);
    const parsed = stored ? JSON.parse(stored) : null;
    return parsed && typeof parsed === "object" ? (parsed as OpenMap) : null;
  } catch {
    // Unreadable or unavailable storage is not worth a failure: every project
    // just starts open, the way the server rendered it.
    return null;
  }
}

/** `git@github.com:acme/web.git` and `/srv/repos/web/` both read as "web". */
export function projectName(repository: string): string {
  const trimmed = repository.replace(TRAILING_SLASHES, "");
  const segment = trimmed
    .split(REPOSITORY_SEPARATOR)
    .at(-1)
    ?.replace(DOT_GIT, "");
  return segment ? segment : repository;
}

function groupByProject(tasks: Task[], records: ProjectRecord[]): Project[] {
  const byRepository = new Map<string, Task[]>();
  for (const record of records) {
    if (record.repository) {
      byRepository.set(record.repository, []);
    }
  }
  for (const task of tasks) {
    const bucket = byRepository.get(task.spec.repository);
    if (bucket) {
      bucket.push(task);
    } else {
      byRepository.set(task.spec.repository, [task]);
    }
  }

  return [...byRepository]
    .map(([repository, list]) => ({
      name: projectName(repository),
      repository,
      tasks: [...list].sort((a, b) => b.created_at - a.created_at),
    }))
    .sort((a, b) => {
      const recent =
        (b.tasks[0]?.created_at ?? 0) - (a.tasks[0]?.created_at ?? 0);
      return recent === 0 ? a.name.localeCompare(b.name) : recent;
    });
}

export function LgtmLogo({ className }: { className?: string }) {
  return (
    <svg
      aria-hidden="true"
      className={className}
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={1.5}
      viewBox="0 0 18 18"
      xmlns="http://www.w3.org/2000/svg"
    >
      <circle cx="9" cy="9" r="2.25" />
      <path d="m11.25,1.75c0,1.2426-1.0074,2.25-2.25,2.25s-2.25-1.0074-2.25-2.25" />
      <path d="m16.25,11.25c-1.2426,0-2.25-1.0074-2.25-2.25s1.0074-2.25,2.25-2.25" />
      <path d="m6.75,16.25c0-1.2426,1.0074-2.25,2.25-2.25,1.2426,0,2.25,1.0074,2.25,2.25" />
      <path d="m1.75,6.75c1.2426,0,2.25,1.0074,2.25,2.25s-1.0074,2.25-2.25,2.25" />
    </svg>
  );
}

export function AppSidebar({
  chats,
  tasks,
  ...props
}: { chats: Chat[]; tasks: Task[] } & ComponentProps<typeof Sidebar>) {
  const matchRoute = useMatchRoute();
  const [records, setRecords] = useState<ProjectRecord[]>([]);
  const [open, setOpen] = useState<OpenMap>({});
  const [addingProject, setAddingProject] = useState(false);
  const [creatingProject, setCreatingProject] = useState(false);
  const projectInputId = useId();
  const projects = groupByProject(tasks, records);
  const shownChats = chats
    .filter((chat) => !chat.archived)
    .slice(0, RECENT_CHATS);

  // Project records carry the prefix and id the manage menu needs, but they are
  // loaded by the root route, which this component does not own. Fetching them
  // here after paint keeps `__root.tsx` untouched, and menu metadata arriving a
  // beat late costs nothing.
  useEffect(() => {
    getProjects()
      .then(setRecords)
      .catch(() => {
        // Without records the menu simply hides "Change prefix…".
      });
  }, []);

  // Reading localStorage during render would disagree with the all-open markup
  // the server sent and mismatch on hydration.
  useEffect(() => {
    const stored = readOpenMap();
    if (stored) {
      setOpen(stored);
    }
  }, []);

  const setProjectOpen = useCallback(
    (repository: string, isOpen: boolean) =>
      setOpen((current) => {
        const next = { ...current, [repository]: isOpen };
        try {
          window.localStorage.setItem(PROJECTS_OPEN_KEY, JSON.stringify(next));
        } catch {
          // Same as the theme toggle: storage can be unavailable (private mode,
          // blocked cookies) and the panel still opens, just not next time.
        }
        return next;
      }),
    []
  );

  const setProjectsGroupOpen = useCallback(
    (isOpen: boolean) => setProjectOpen(PROJECTS_GROUP, isOpen),
    [setProjectOpen]
  );
  const setChatsGroupOpen = useCallback(
    (isOpen: boolean) => setProjectOpen(CHATS_GROUP, isOpen),
    [setProjectOpen]
  );

  const showAddProject = useCallback(() => setAddingProject(true), []);
  const hideAddProject = useCallback(() => setAddingProject(false), []);
  const updateRecord = useCallback((updated: ProjectRecord) => {
    setRecords((current) =>
      current.map((record) => (record.id === updated.id ? updated : record))
    );
  }, []);

  const addProject = useCallback(
    async (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      if (creatingProject) {
        return;
      }
      const data = new FormData(event.currentTarget);
      const repository = String(data.get("repository") ?? "").trim();
      if (!repository) {
        return;
      }
      setCreatingProject(true);
      try {
        const project = await createProject({ data: repository });
        setRecords((current) => [
          project,
          ...current.filter((record) => record.id !== project.id),
        ]);
        setOpen((current) => ({ ...current, [repository]: true }));
        setAddingProject(false);
        toast.success(`${project.name} added`);
      } catch (error) {
        toast.error(error instanceof Error ? error.message : String(error));
      } finally {
        setCreatingProject(false);
      }
    },
    [creatingProject]
  );

  return (
    <Sidebar collapsible="offcanvas" {...props}>
      <SidebarHeader className="pb-2">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              className="p-1.5!"
              render={<Link search={{ repo: undefined }} to="/" />}
            >
              <LgtmLogo className="size-5! shrink-0 text-foreground" />
              <span className="font-semibold text-base tracking-tight">
                LGTM
              </span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      <SidebarContent className="scrollbar-gutter-stable">
        <SidebarGroup className="pt-0">
          <SidebarGroupContent className="flex flex-col gap-1">
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton
                  className="min-w-8 bg-primary text-primary-foreground duration-200 ease-linear hover:bg-primary/90 hover:text-primary-foreground active:bg-primary/90 active:text-primary-foreground"
                  render={<Link search={{ repo: undefined }} to="/" />}
                >
                  <ComposeIcon aria-hidden="true" />
                  <span>New task</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
            <SidebarMenu className="gap-1">
              {NAV.map(({ to, label, icon: Icon, exact }) => (
                <SidebarMenuItem key={to}>
                  <SidebarMenuButton
                    isActive={!!matchRoute({ fuzzy: !exact, to })}
                    render={<Link to={to} />}
                  >
                    <Icon aria-hidden="true" />
                    <span>{label}</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
              <MoreItem />
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>

        <Collapsible
          onOpenChange={setProjectsGroupOpen}
          open={open[PROJECTS_GROUP] ?? true}
          render={<SidebarGroup className="group/group" />}
        >
          <GroupHeading>Projects</GroupHeading>
          <SidebarGroupAction
            aria-label="Add project"
            className="opacity-0 pointer-coarse:opacity-100 transition-opacity group-focus-within/group:opacity-100 group-hover/group:opacity-100"
            onClick={showAddProject}
            type="button"
          >
            <PlusIcon aria-hidden="true" />
          </SidebarGroupAction>
          <CollapsibleContent render={<SidebarGroupContent />}>
            {addingProject ? (
              <form className="mb-2 px-2" onSubmit={addProject}>
                <label className="sr-only" htmlFor={projectInputId}>
                  Git repository URL
                </label>
                <Input
                  autoComplete="url"
                  autoFocus
                  disabled={creatingProject}
                  id={projectInputId}
                  name="repository"
                  placeholder="Repository URL"
                  required
                  type="text"
                />
                <div className="mt-1.5 flex justify-end gap-1.5">
                  <Button
                    disabled={creatingProject}
                    onClick={hideAddProject}
                    size="xs"
                    type="button"
                    variant="ghost"
                  >
                    Cancel
                  </Button>
                  <Button disabled={creatingProject} size="xs" type="submit">
                    {creatingProject ? "Adding…" : "Add project"}
                  </Button>
                </div>
              </form>
            ) : null}
            {projects.length === 0 ? (
              <p className="px-2 py-1 text-sidebar-foreground/70 text-sm">
                No projects yet
              </p>
            ) : (
              <SidebarMenu className="gap-1">
                {projects.map((project) => (
                  <ProjectItem
                    key={project.repository}
                    onOpenChange={setProjectOpen}
                    onRecordChange={updateRecord}
                    open={open[project.repository] ?? true}
                    project={project}
                    record={records.find(
                      (r) => r.repository === project.repository
                    )}
                  />
                ))}
              </SidebarMenu>
            )}
          </CollapsibleContent>
        </Collapsible>

        <Collapsible
          onOpenChange={setChatsGroupOpen}
          open={open[CHATS_GROUP] ?? true}
          render={<SidebarGroup className="group/group" />}
        >
          <GroupHeading>Your chats</GroupHeading>
          <CollapsibleContent render={<SidebarGroupContent />}>
            {shownChats.length === 0 ? (
              <p className="px-2 py-1 text-sidebar-foreground/70 text-sm">
                No chats yet
              </p>
            ) : (
              <SidebarMenu className="gap-1">
                {shownChats.map((chat) => (
                  <ChatItem chat={chat} key={chat.id} />
                ))}
              </SidebarMenu>
            )}
          </CollapsibleContent>
        </Collapsible>
      </SidebarContent>

      <SidebarFooter>
        <AccountMenu />
      </SidebarFooter>
    </Sidebar>
  );
}

function GroupHeading({ children }: { children: string }) {
  return (
    <SidebarGroupLabel
      className="group/label w-full cursor-pointer hover:text-sidebar-foreground"
      render={<CollapsibleTrigger />}
    >
      <span>{children}</span>
      <ChevronIcon aria-hidden="true" className={GROUP_CARET} />
    </SidebarGroupLabel>
  );
}

function MoreItem() {
  const { isMobile } = useSidebar();

  return (
    <SidebarMenuItem>
      <DropdownMenu>
        <DropdownMenuTrigger
          render={<SidebarMenuButton className="text-sidebar-foreground/70" />}
        >
          <DotsIcon aria-hidden="true" />
          <span>More</span>
        </DropdownMenuTrigger>

        <DropdownMenuContent
          align={isMobile ? "end" : "start"}
          className="w-32 rounded-lg"
          side={isMobile ? "bottom" : "right"}
          sideOffset={4}
        >
          {MORE.map(({ to, label, icon: Icon }) => (
            <DropdownMenuItem
              className="gap-2 px-2 py-1.5"
              key={to}
              render={<Link to={to} />}
            >
              <Icon aria-hidden="true" />
              <span>{label}</span>
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SidebarMenuItem>
  );
}
