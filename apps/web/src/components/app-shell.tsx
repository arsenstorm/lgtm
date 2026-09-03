import type { CSSProperties } from 'react'

import { AppSidebar } from '@/components/app-sidebar'
import { SiteHeader } from '@/components/site-header'
import { SidebarInset, SidebarProvider } from '@/components/ui/sidebar'
import type { Task } from '@/lib/lgtm/types'

export function AppShell({ tasks, children }: { tasks: Task[]; children: React.ReactNode }) {
  return (
    // The page body never scrolls: the shell is pinned to the viewport and the
    // content column is the only scroll container.
    <SidebarProvider
      className="isolate h-dvh overflow-hidden"
      style={
        {
          '--sidebar-width': 'calc(var(--spacing) * 72)',
          '--header-height': 'calc(var(--spacing) * 12)',
        } as CSSProperties
      }
    >
      <AppSidebar variant="inset" tasks={tasks} />
      <SidebarInset className="min-h-0">
        <SiteHeader />
        <div className="min-w-0 flex-1 overflow-y-auto overscroll-contain scrollbar-gutter-stable">{children}</div>
      </SidebarInset>
    </SidebarProvider>
  )
}
