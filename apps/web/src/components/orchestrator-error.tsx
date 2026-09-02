import type { ErrorComponentProps } from '@tanstack/react-router'

/** Route-level failure to reach the orchestrator, shared by every loader.
 *  `what` names the thing that could not load: "tasks", "runners", "this task". */
export function OrchestratorError({ what, error }: { what: string } & ErrorComponentProps) {
  return (
    <div className="m-4 flex flex-col gap-2 rounded-xl border border-red-600/25 bg-red-500/5 p-6 sm:m-6">
      <h1 className="text-base font-medium text-red-700 dark:text-red-400">
        Can&rsquo;t load {what}
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
