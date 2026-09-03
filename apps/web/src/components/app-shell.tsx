import { AppSidebar } from '@/components/app-sidebar'
import { SidebarInset, SidebarProvider, SidebarTrigger } from '@/components/ui/sidebar'
import type { Task } from '@/lib/lgtm/types'

export function AppShell({ tasks, children }: { tasks: Task[]; children: React.ReactNode }) {
  return (
    // The page body never scrolls: the shell is pinned to the viewport and the
    // content column is the only scroll container.
    <SidebarProvider className="isolate h-dvh overflow-hidden">
      <AppSidebar tasks={tasks} />
      <SidebarInset className="min-h-0">
        <div className="flex h-12 shrink-0 items-center border-b border-border px-2 md:hidden">
          <SidebarTrigger />
        </div>
        {/* SidebarInset is already the <main> landmark, so the scroll container
            below stays a plain element. */}
        <div className="min-w-0 flex-1 overflow-y-auto overscroll-contain">{children}</div>
      </SidebarInset>
    </SidebarProvider>
  )
}
