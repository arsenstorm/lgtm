import { useMatchRoute } from '@tanstack/react-router'

import { Separator } from '@/components/ui/separator'
import { SidebarTrigger } from '@/components/ui/sidebar'

/** The header names where you are; the pages themselves lead with content. */
function useTitle(): string {
  const matchRoute = useMatchRoute()
  const task = matchRoute({ to: '/tasks/$id' })
  if (task) return `Task ${task.id}`
  if (matchRoute({ to: '/runners' })) return 'Runners'
  return 'Tasks'
}

export function SiteHeader() {
  const title = useTitle()

  return (
    <header className="flex h-(--header-height) shrink-0 items-center gap-2 border-b">
      <div className="flex w-full items-center gap-1 px-4 lg:gap-2 lg:px-6">
        <SidebarTrigger className="-ml-1" />
        <Separator orientation="vertical" className="mx-2 data-[orientation=vertical]:h-4" />
        <h1 className="text-base font-medium">{title}</h1>
      </div>
    </header>
  )
}
