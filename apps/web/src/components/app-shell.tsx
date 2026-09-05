import type { AnyRouter } from "@tanstack/react-router";
import { useMatches } from "@tanstack/react-router";
import type { CSSProperties, ReactNode } from "react";
import {
  createContext,
  useContext,
  useEffect,
  useLayoutEffect,
  useState,
} from "react";

import { AppSidebar } from "@/components/app-sidebar";
import { SiteHeader } from "@/components/site-header";
import { TaskSummaryPanel } from "@/components/task-summary-panel";
import {
  Sidebar,
  SidebarContent,
  SidebarHeader,
  SidebarProvider,
  useSidebar,
} from "@/components/ui/sidebar";
import type { Chat, Project, Task, TaskDetail } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

const TASK_PANEL_KEY = "lgtm-task-panel-open";

interface RightPanel {
  content: ReactNode;
  title: string;
}

const RightPanelContext = createContext<(panel: RightPanel | null) => void>(
  () => undefined
);

/** A page with a right-hand panel hands the shell its content here. The
 *  shell owns the column, the header toggle, and whether it is open, so every
 *  page's panel behaves the same way. Runs in a layout effect so the panel
 *  never paints a render behind the page. */
export function useRightPanel(panel: RightPanel) {
  const set = useContext(RightPanelContext);
  useLayoutEffect(() => {
    set(panel);
  });
  useLayoutEffect(() => () => set(null), [set]);
}

function useTaskPanelOpen() {
  const [open, setOpen] = useState(true);

  useEffect(() => {
    try {
      const stored = window.localStorage.getItem(TASK_PANEL_KEY);
      if (stored !== null) {
        setOpen(stored === "true");
      }
    } catch {
      // Unavailable storage leaves the task panel open by default.
    }
  }, []);

  function updateOpen(next: boolean) {
    setOpen(next);
    try {
      window.localStorage.setItem(TASK_PANEL_KEY, String(next));
    } catch {
      // The control still works when the preference cannot be persisted.
    }
  }

  return { open, setOpen: updateOpen };
}

export function AppShell({
  chats,
  projects,
  tasks,
  children,
}: {
  chats: Chat[];
  projects: Project[];
  tasks: Task[];
  children: React.ReactNode;
}) {
  return (
    // The page body never scrolls: the shell is pinned to the viewport and the
    // content column is the only scroll container.
    <SidebarProvider
      className="isolate h-dvh overflow-hidden"
      style={
        {
          "--header-height": "calc(var(--spacing) * 12)",
          "--sidebar-width": "calc(var(--spacing) * 72)",
        } as CSSProperties
      }
    >
      <AppSidebar
        chats={chats}
        records={projects}
        tasks={tasks}
        variant="inset"
      />
      <AppFrame chats={chats} tasks={tasks}>
        {children}
      </AppFrame>
    </SidebarProvider>
  );
}

function AppFrame({
  chats,
  tasks,
  children,
}: {
  chats: Chat[];
  tasks: Task[];
  children: React.ReactNode;
}) {
  const leftSidebar = useSidebar();
  const taskDetail = useMatches<AnyRouter, TaskDetail | undefined>({
    select: (matches) =>
      matches.find((match) => match.routeId === "/tasks/$id")?.loaderData as
        | TaskDetail
        | undefined,
  });
  const taskPanel = useTaskPanelOpen();
  const [pagePanel, setPagePanel] = useState<RightPanel | null>(null);
  const panel: RightPanel | null = taskDetail
    ? {
        content: <TaskSummaryPanel detail={taskDetail} />,
        title: "Task details",
      }
    : pagePanel;
  const leftShown = leftSidebar.isMobile
    ? leftSidebar.openMobile
    : leftSidebar.open;

  return (
    <SidebarProvider
      className="h-full min-h-0 min-w-0 flex-1 overflow-hidden"
      cookieName={null}
      keyboardShortcut={null}
      onOpenChange={taskPanel.setOpen}
      open={panel ? taskPanel.open : false}
      style={
        {
          "--sidebar-width": "calc(var(--spacing) * 96)",
        } as CSSProperties
      }
    >
      <main
        className={cn(
          "relative flex min-w-0 flex-1 flex-col bg-background md:m-2 md:ml-0 md:rounded-xl md:shadow-sm",
          panel && taskPanel.open && "md:mr-0"
        )}
      >
        <SiteHeader
          chats={chats}
          hasPanel={panel !== null}
          leftSidebar={{
            shown: leftShown,
            toggle: leftSidebar.toggleSidebar,
          }}
          task={taskDetail?.task}
          tasks={tasks}
        />
        <div className="scrollbar-gutter-stable min-w-0 flex-1 overflow-y-auto overflow-x-hidden overscroll-y-contain">
          <RightPanelContext.Provider value={setPagePanel}>
            {children}
          </RightPanelContext.Provider>
        </div>
      </main>
      {panel ? (
        <Sidebar collapsible="offcanvas" side="right" variant="inset">
          <SidebarHeader className="px-4 pt-4 pb-2">
            <h2 className="font-medium text-sm tracking-tight">
              {panel.title}
            </h2>
          </SidebarHeader>
          <SidebarContent className="px-4 pb-4">{panel.content}</SidebarContent>
        </Sidebar>
      ) : null}
    </SidebarProvider>
  );
}
