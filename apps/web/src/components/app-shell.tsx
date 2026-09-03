import type { CSSProperties } from "react";

import { AppSidebar } from "@/components/app-sidebar";
import { SiteHeader } from "@/components/site-header";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import type { Task } from "@/lib/lgtm/types";

export function AppShell({
  tasks,
  children,
}: {
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
      <AppSidebar tasks={tasks} variant="inset" />
      <SidebarInset className="min-h-0">
        <SiteHeader tasks={tasks} />
        <div className="scrollbar-gutter-stable min-w-0 flex-1 overflow-y-auto overscroll-contain">
          {children}
        </div>
      </SidebarInset>
    </SidebarProvider>
  );
}
