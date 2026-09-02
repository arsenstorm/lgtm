import { createFileRoute } from '@tanstack/react-router'
import type { ErrorComponentProps } from '@tanstack/react-router'

import { RunnerList } from '@/components/runner-list'
import { getRunners } from '@/lib/lgtm/server'

export const Route = createFileRoute('/runners')({
  loader: async () => ({ runners: await getRunners() }),
  component: RunnersPage,
  errorComponent: RunnersError,
})

function RunnersPage() {
  const { runners } = Route.useLoaderData()
  const busy = runners.reduce((total, runner) => total + runner.running.length, 0)
  const slots = runners.reduce((total, runner) => total + runner.info.slots, 0)

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-7xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <div className="flex items-baseline gap-3">
        <h1 className="text-xl font-medium tracking-tight">Runners</h1>
        <span className="text-sm tabular-nums text-muted-foreground">
          {runners.length === 0 ? '0' : `${busy} of ${slots} slots busy`}
        </span>
      </div>
      <RunnerList runners={runners} />
    </div>
  )
}

function RunnersError({ error }: ErrorComponentProps) {
  return (
    <div className="m-4 flex flex-col gap-2 rounded-xl border border-red-600/25 bg-red-500/5 p-6 sm:m-6">
      <h1 className="text-base font-medium text-red-700 dark:text-red-400">
        Can&rsquo;t load runners
      </h1>
      <p className="max-w-[68ch] text-base text-pretty text-muted-foreground sm:text-sm">
        The orchestrator did not answer. Check that <code className="text-sm">lgtm serve</code> is
        running and that <code className="text-sm">LGTM_ORCHESTRATOR</code> and{' '}
        <code className="text-sm">LGTM_TOKEN</code> in <code className="text-sm">.dev.vars</code>{' '}
        point at it.
      </p>
      <p className="font-mono text-sm break-words text-muted-foreground">{error.message}</p>
    </div>
  )
}
