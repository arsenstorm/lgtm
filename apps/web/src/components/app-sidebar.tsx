import { PlusCircle } from "@phosphor-icons/react";
import { Link, useMatchRoute } from "@tanstack/react-router";
import type { ComponentProps } from "react";
import { useEffect, useState } from "react";

import { AccountMenu } from "@/components/account-menu";
import {
  ActivityIcon,
  AiDeveloperIcon,
  BrainSparkleIcon,
  DotsIcon,
  ListCheckboxIcon,
  MsgsIcon,
  NotesIcon,
  TasksIcon,
} from "@/components/icons";
import type { Project } from "@/components/project-item";
import { ProjectItem } from "@/components/project-item";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from "@/components/ui/sidebar";
import { getProjects } from "@/lib/lgtm/server";
import type { Project as ProjectRecord, Task } from "@/lib/lgtm/types";

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

const PROJECTS_OPEN_KEY = "lgtm-projects-open";

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
  const segment = trimmed.split("/").at(-1)?.replace(DOT_GIT, "");
  return segment ? segment : repository;
}

function groupByProject(tasks: Task[]): Project[] {
  const byRepository = new Map<string, Task[]>();
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
    .sort((a, b) => b.tasks[0].created_at - a.tasks[0].created_at);
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
  tasks,
  ...props
}: { tasks: Task[] } & ComponentProps<typeof Sidebar>) {
  const matchRoute = useMatchRoute();
  const projects = groupByProject(tasks);
  const [records, setRecords] = useState<ProjectRecord[]>([]);
  const [open, setOpen] = useState<OpenMap>({});

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

  function setProjectOpen(repository: string, isOpen: boolean) {
    const next = { ...open, [repository]: isOpen };
    setOpen(next);
    try {
      window.localStorage.setItem(PROJECTS_OPEN_KEY, JSON.stringify(next));
    } catch {
      // Same as the theme toggle: storage can be unavailable (private mode,
      // blocked cookies) and the panel still opens, just not next time.
    }
  }

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

      <SidebarContent>
        <SidebarGroup className="pt-0">
          <SidebarGroupContent className="flex flex-col gap-1">
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton
                  className="min-w-8 bg-primary text-primary-foreground duration-200 ease-linear hover:bg-primary/90 hover:text-primary-foreground active:bg-primary/90 active:text-primary-foreground"
                  render={<Link search={{ repo: undefined }} to="/" />}
                >
                  <PlusCircle aria-hidden="true" weight="fill" />
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

        <SidebarGroup>
          <SidebarGroupLabel>Projects</SidebarGroupLabel>
          <SidebarGroupContent>
            {projects.length === 0 ? (
              <p className="px-2 py-1 text-sidebar-foreground/70 text-sm">
                No tasks yet
              </p>
            ) : (
              <SidebarMenu className="gap-1">
                {projects.map((project) => (
                  <ProjectItem
                    key={project.repository}
                    onOpenChange={(isOpen) =>
                      setProjectOpen(project.repository, isOpen)
                    }
                    onRecordChange={(updated) =>
                      setRecords((current) =>
                        current.map((r) => (r.id === updated.id ? updated : r))
                      )
                    }
                    open={open[project.repository] ?? true}
                    project={project}
                    record={records.find(
                      (r) => r.repository === project.repository
                    )}
                  />
                ))}
              </SidebarMenu>
            )}
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>

      <SidebarFooter>
        <AccountMenu />
      </SidebarFooter>
    </Sidebar>
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

