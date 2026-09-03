import { Link, useMatchRoute } from '@tanstack/react-router'
import {
  RiAddCircleFill,
  RiArrowRightSLine,
  RiErrorWarningFill,
  RiGitRepositoryLine,
  RiListCheck2,
  RiLogoutBoxRLine,
  RiMoonLine,
  RiMore2Line,
  RiServerLine,
  RiSunLine,
} from '@remixicon/react'
import type { ComponentProps } from 'react'
import { useState } from 'react'
import { toast } from 'sonner'

import { STATUS } from '@/components/task-list'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
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
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
  useSidebar,
} from '@/components/ui/sidebar'
import type { Task, TaskStatus } from '@/lib/lgtm/types'
import { cn } from '@/lib/utils'

const NAV = [
  { to: '/', label: 'Tasks', icon: RiListCheck2, exact: true },
  { to: '/runners', label: 'Runners', icon: RiServerLine, exact: false },
] as const

const PREVIEW = 5

// Only the statuses that need a person get a trailing mark; everything else is
// noise in a list you scan.
const ATTENTION: Partial<Record<TaskStatus, string>> = {
  awaiting_review: 'text-amber-500',
  failed: 'text-red-500',
  timed_out: 'text-red-500',
  runner_lost: 'text-red-500',
}

const TRAILING_SLASHES = /\/+$/
const DOT_GIT = /\.git$/

/**
 * Mirrors the pre-paint script in `__root.tsx`: same `theme` key, same pair of
 * classes on <html>, same inline color-scheme. Drifting from it reintroduces
 * the flash that script exists to prevent.
 */
function toggleTheme() {
  const root = document.documentElement
  const next = root.classList.contains('dark') ? 'light' : 'dark'

  root.classList.remove('light', 'dark')
  root.classList.add(next)
  root.style.colorScheme = next

  try {
    window.localStorage.setItem('theme', next)
  } catch {
    // Storage can be unavailable (private mode, blocked cookies); the toggle
    // still works for this page, it just will not be remembered.
  }
}

function firstLine(prompt: string): string {
  const line = prompt.split('\n', 1)[0]?.trim()
  return line ? line : '(no prompt)'
}

/** `git@github.com:acme/web.git` and `/srv/repos/web/` both read as "web". */
function projectName(repository: string): string {
  const trimmed = repository.replace(TRAILING_SLASHES, '')
  const segment = trimmed.split('/').at(-1)?.replace(DOT_GIT, '')
  return segment ? segment : repository
}

interface Project {
  repository: string
  name: string
  tasks: Task[]
}

function groupByProject(tasks: Task[]): Project[] {
  const byRepository = new Map<string, Task[]>()
  for (const task of tasks) {
    const bucket = byRepository.get(task.spec.repository)
    if (bucket) {
      bucket.push(task)
    } else {
      byRepository.set(task.spec.repository, [task])
    }
  }

  return [...byRepository]
    .map(([repository, list]) => ({
      repository,
      name: projectName(repository),
      tasks: [...list].sort((a, b) => b.created_at - a.created_at),
    }))
    .sort((a, b) => b.tasks[0].created_at - a.tasks[0].created_at)
}

function LgtmLogo({ className }: { className?: string }) {
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
  )
}

export function AppSidebar({
  tasks,
  ...props
}: { tasks: Task[] } & ComponentProps<typeof Sidebar>) {
  const matchRoute = useMatchRoute()
  const projects = groupByProject(tasks)

  return (
    <Sidebar collapsible="offcanvas" {...props}>
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton className="p-1.5!" render={<Link to="/" />}>
              <LgtmLogo className="size-5! shrink-0 text-foreground" />
              <span className="text-base font-semibold tracking-tight">LGTM</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupContent className="flex flex-col gap-2">
            <SidebarMenu>
              <SidebarMenuItem>
                {/* Placeholder: the web app has no creation flow yet, tasks are
                    queued from the CLI. */}
                <SidebarMenuButton
                  className="min-w-8 bg-primary text-primary-foreground duration-200 ease-linear hover:bg-primary/90 hover:text-primary-foreground active:bg-primary/90 active:text-primary-foreground"
                  onClick={() =>
                    toast.info('Task creation from the web is coming — use lgtm run for now')
                  }
                >
                  <RiAddCircleFill aria-hidden="true" />
                  <span>New task</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
            <SidebarMenu>
              {NAV.map(({ to, label, icon: Icon, exact }) => (
                <SidebarMenuItem key={to}>
                  <SidebarMenuButton
                    isActive={!!matchRoute({ to, fuzzy: !exact })}
                    render={<Link to={to} />}
                  >
                    <Icon aria-hidden="true" />
                    <span>{label}</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>

        <SidebarGroup>
          <SidebarGroupLabel>Projects</SidebarGroupLabel>
          <SidebarGroupContent>
            {projects.length === 0 ? (
              <p className="px-2 py-1 text-sm text-sidebar-foreground/70">No tasks yet</p>
            ) : (
              <SidebarMenu>
                {projects.map((project) => (
                  <ProjectItem key={project.repository} project={project} />
                ))}
              </SidebarMenu>
            )}
          </SidebarGroupContent>
        </SidebarGroup>

        <SidebarGroup className="mt-auto">
          <SidebarGroupContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton onClick={toggleTheme}>
                  {/* The theme is only known from the class the pre-paint script
                      wrote, so both states ship in the markup and CSS picks one.
                      Deriving it in React would mismatch on hydration and flash. */}
                  <RiMoonLine aria-hidden="true" className="dark:hidden" />
                  <RiSunLine aria-hidden="true" className="hidden dark:block" />
                  <span className="dark:hidden">Dark mode</span>
                  <span className="hidden dark:inline">Light mode</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>

      <SidebarFooter>
        <AccountMenu />
      </SidebarFooter>
    </Sidebar>
  )
}

function ProjectItem({ project }: { project: Project }) {
  const matchRoute = useMatchRoute()
  const [expanded, setExpanded] = useState(false)
  const shown = expanded ? project.tasks : project.tasks.slice(0, PREVIEW)
  const hidden = project.tasks.length - PREVIEW

  return (
    <Collapsible defaultOpen>
      <SidebarMenuItem>
        <CollapsibleTrigger render={<SidebarMenuButton />}>
          <RiGitRepositoryLine aria-hidden="true" />
          <span>{project.name}</span>
          <RiArrowRightSLine
            aria-hidden="true"
            className="ml-auto transition-transform duration-200 group-data-[panel-open]/menu-button:rotate-90"
          />
        </CollapsibleTrigger>

        <CollapsibleContent>
          <SidebarMenuSub>
            {shown.map((task) => {
              const { label } = STATUS[task.status]
              const attention = ATTENTION[task.status]

              return (
                <SidebarMenuSubItem key={task.id}>
                  <SidebarMenuSubButton
                    isActive={!!matchRoute({ to: '/tasks/$id', params: { id: task.id } })}
                    render={<Link params={{ id: task.id }} to="/tasks/$id" />}
                  >
                    <span className="min-w-0 flex-1 truncate">{firstLine(task.spec.prompt)}</span>
                    {attention && (
                      // SidebarMenuSubButton force-colours its direct `svg`
                      // children, so the icon keeps its tone only inside a span.
                      <span aria-label={label} className="ml-auto flex shrink-0" role="img">
                        <RiErrorWarningFill className={cn('size-3.5', attention)} />
                      </span>
                    )}
                  </SidebarMenuSubButton>
                </SidebarMenuSubItem>
              )
            })}

            {hidden > 0 && (
              <SidebarMenuSubItem>
                <SidebarMenuSubButton
                  className="text-sidebar-foreground/70"
                  onClick={() => setExpanded((open) => !open)}
                  render={<button type="button" />}
                >
                  <span>{expanded ? 'Show less' : `Show ${hidden} more`}</span>
                </SidebarMenuSubButton>
              </SidebarMenuSubItem>
            )}
          </SidebarMenuSub>
        </CollapsibleContent>
      </SidebarMenuItem>
    </Collapsible>
  )
}

// Placeholder identity: real auth (and a real signed-in user) lands later, so
// the name, email and initials are hard-coded and sign out is inert.
const USER = { name: 'Arsen Shkrumelyak', email: 'arsen@shkrumelyak.com', initials: 'AS' }

function AccountMenu() {
  const { isMobile } = useSidebar()

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <SidebarMenuButton
                size="lg"
                className="data-open:bg-sidebar-accent data-open:text-sidebar-accent-foreground"
              />
            }
          >
            <Identity />
            <RiMore2Line aria-hidden="true" className="ml-auto size-4" />
          </DropdownMenuTrigger>

          <DropdownMenuContent
            className="min-w-56 rounded-lg"
            side={isMobile ? 'bottom' : 'right'}
            align="end"
            sideOffset={4}
          >
            <DropdownMenuGroup>
              {/* Base UI: a menu label must sit inside a menu group. */}
              <DropdownMenuLabel className="p-0 font-normal">
                <div className="flex items-center gap-2 px-1 py-1.5 text-left text-sm">
                  <Identity />
                </div>
              </DropdownMenuLabel>
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            <DropdownMenuItem disabled>
              <RiLogoutBoxRLine aria-hidden="true" />
              Sign out
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  )
}

function Identity() {
  return (
    <>
      <Avatar size="sm" className="rounded-lg after:rounded-lg">
        <AvatarFallback className="rounded-lg">{USER.initials}</AvatarFallback>
      </Avatar>
      <span className="grid min-w-0 flex-1 text-left text-sm leading-tight">
        <span className="truncate font-medium">{USER.name}</span>
        <span className="truncate text-xs text-muted-foreground">{USER.email}</span>
      </span>
    </>
  )
}
