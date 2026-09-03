import { Link, useMatchRoute, useRouter } from "@tanstack/react-router";
import { useCallback, useState } from "react";
import { toast } from "sonner";

import {
  ChevronIcon,
  CircleWarningIcon,
  ComposeIcon,
  DotsIcon,
  FolderIcon,
} from "@/components/icons";
import { STATUS } from "@/components/task-list";
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
import {
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
} from "@/components/ui/sidebar";
import { updateProjectPrefix } from "@/lib/lgtm/server";
import type {
  Project as ProjectRecord,
  Task,
  TaskStatus,
} from "@/lib/lgtm/types";
import { cn, taskTitle } from "@/lib/utils";

export interface Project {
  name: string;
  repository: string;
  tasks: Task[];
}

const PREVIEW = 5;

// Only the statuses that need a person get a trailing mark; everything else is
// noise in a list you scan.
const ATTENTION: Partial<Record<TaskStatus, string>> = {
  awaiting_review: "text-amber-500",
  failed: "text-red-500",
  runner_lost: "text-red-500",
  timed_out: "text-red-500",
};

// Row furniture stays out of the way until the row is reached. A coarse
// pointer has no hover to reach it with, so there it is always on.
const REVEAL =
  "pointer-coarse:opacity-100 opacity-0 group-focus-within/menu-item:opacity-100 group-hover/menu-item:opacity-100";

export function ProjectItem({
  onOpenChange,
  onRecordChange,
  open,
  project,
  record,
}: {
  onOpenChange: (repository: string, open: boolean) => void;
  onRecordChange: (record: ProjectRecord) => void;
  open: boolean;
  project: Project;
  record?: ProjectRecord;
}) {
  const matchRoute = useMatchRoute();
  const router = useRouter();
  const [expanded, setExpanded] = useState(false);
  const shown = expanded ? project.tasks : project.tasks.slice(0, PREVIEW);
  const hidden = project.tasks.length - PREVIEW;
  const handleOpenChange = useCallback(
    (isOpen: boolean) => onOpenChange(project.repository, isOpen),
    [onOpenChange, project.repository]
  );

  function copyRepository() {
    navigator.clipboard
      .writeText(project.repository)
      .then(() => toast.success("Repository URL copied"))
      .catch(() => toast.error("Could not copy the repository URL"));
  }

  async function changePrefix() {
    if (!record) {
      return;
    }
    // ponytail: window.prompt is the deliberate cheap path; a real dialog
    // arrives with the settings surface, which does not exist yet.
    // biome-ignore lint/suspicious/noAlert: cheap path, see above
    const entered = window.prompt(`Prefix for ${project.name}`, record.prefix);
    const prefix = entered?.trim();
    if (!prefix || prefix === record.prefix) {
      return;
    }
    try {
      const updated = await updateProjectPrefix({
        data: { id: record.id, prefix },
      });
      onRecordChange(updated);
      toast.success(`Prefix is now ${updated.prefix} — todo ids follow it`);
      await router.invalidate();
    } catch (error) {
      // A 409 names the project already holding the prefix; that name is the
      // only thing that says what to do next, so it travels verbatim.
      toast.error(error instanceof Error ? error.message : String(error));
    }
  }

  return (
    <Collapsible onOpenChange={handleOpenChange} open={open}>
      <SidebarMenuItem>
        {/* The right padding keeps the name's truncation clear of the actions
            that sit over this row. */}
        <CollapsibleTrigger render={<SidebarMenuButton className="pr-14" />}>
          <FolderIcon aria-hidden="true" open={open} />
          <span className="min-w-0 truncate">{project.name}</span>
          <ChevronIcon
            aria-hidden="true"
            className={cn(
              REVEAL,
              "text-muted-foreground transition-[opacity,transform,color] duration-200 hover:text-foreground group-data-[panel-open]/menu-button:rotate-90"
            )}
          />
        </CollapsibleTrigger>

        {/* A click anywhere inside the trigger toggles the collapsible, so the
            actions cannot be nested in it — they overlay the row as a sibling. */}
        <div
          className={cn(
            REVEAL,
            // The menu is portalled out of the row, so hover ends the moment it
            // opens — without this its own trigger would vanish under it.
            "absolute top-1 right-1 flex items-center gap-0.5 transition-opacity has-[[aria-expanded=true]]:opacity-100"
          )}
        >
          <DropdownMenu>
            <DropdownMenuTrigger
              render={
                <Button
                  aria-label={`Manage ${project.name}`}
                  className="text-muted-foreground"
                  size="icon-xs"
                  variant="ghost"
                />
              }
            >
              <DotsIcon aria-hidden="true" className="size-4" />
            </DropdownMenuTrigger>

            <DropdownMenuContent
              align="start"
              className="w-48 rounded-lg"
              side="right"
              sideOffset={4}
            >
              <DropdownMenuItem
                className="gap-2 px-2 py-1.5"
                render={<Link search={{ repo: project.repository }} to="/" />}
              >
                New task
              </DropdownMenuItem>
              <DropdownMenuItem
                className="gap-2 px-2 py-1.5"
                onClick={copyRepository}
              >
                Copy repository URL
              </DropdownMenuItem>
              {/* No record means no todo ever numbered this repository, so
                  there is no prefix to change. */}
              {record && (
                <DropdownMenuItem
                  className="gap-2 px-2 py-1.5"
                  onClick={changePrefix}
                >
                  Change prefix…
                </DropdownMenuItem>
              )}
            </DropdownMenuContent>
          </DropdownMenu>

          <Button
            aria-label={`New task in ${project.name}`}
            className="text-muted-foreground"
            nativeButton={false}
            render={<Link search={{ repo: project.repository }} to="/" />}
            size="icon-xs"
            variant="ghost"
          >
            <ComposeIcon aria-hidden="true" className="size-4" />
          </Button>
        </div>

        <CollapsibleContent>
          <SidebarMenuSub className="mr-0 pr-0">
            {project.tasks.length === 0 ? (
              <SidebarMenuSubItem>
                <span className="block px-2 py-1 text-muted-foreground text-xs italic">
                  No tasks yet
                </span>
              </SidebarMenuSubItem>
            ) : null}
            {shown.map((task) => {
              const { label } = STATUS[task.status];
              const attention = ATTENTION[task.status];

              return (
                <SidebarMenuSubItem key={task.id}>
                  <SidebarMenuSubButton
                    isActive={
                      !!matchRoute({
                        params: { id: task.id },
                        to: "/tasks/$id",
                      })
                    }
                    render={<Link params={{ id: task.id }} to="/tasks/$id" />}
                  >
                    <span className="min-w-0 flex-1 overflow-hidden whitespace-nowrap [mask-image:linear-gradient(to_right,black_calc(100%-1.25rem),transparent)]">
                      {taskTitle(task)}
                    </span>
                    {attention && (
                      // SidebarMenuSubButton force-colours its direct `svg`
                      // children, so the icon keeps its tone only inside a span.
                      <span
                        aria-label={label}
                        className="ml-auto flex shrink-0"
                        role="img"
                      >
                        <CircleWarningIcon
                          className={cn("size-3.5", attention)}
                        />
                      </span>
                    )}
                  </SidebarMenuSubButton>
                </SidebarMenuSubItem>
              );
            })}

            {hidden > 0 && (
              <SidebarMenuSubItem>
                <SidebarMenuSubButton
                  className="text-sidebar-foreground/70"
                  onClick={() => setExpanded((current) => !current)}
                  render={<button type="button" />}
                >
                  <span>{expanded ? "Show less" : `Show ${hidden} more`}</span>
                </SidebarMenuSubButton>
              </SidebarMenuSubItem>
            )}
          </SidebarMenuSub>
        </CollapsibleContent>
      </SidebarMenuItem>
    </Collapsible>
  );
}
