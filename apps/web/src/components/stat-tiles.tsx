import type { Stats } from '@/lib/lgtm/types'

import { shortSpan } from './task-list'

const USD = new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' })

function usd(amount: number): string {
  return USD.format(amount)
}

export function StatTiles({ stats }: { stats: Stats }) {
  const tiles: { label: string; value: string; hint: string }[] = [
    {
      label: 'Tasks',
      value: String(stats.tasks),
      hint: `last ${shortSpan(Date.now() - stats.since)}`,
    },
    {
      label: 'Running',
      value: String(stats.running),
      hint: `${stats.queued} queued`,
    },
    {
      label: 'Awaiting review',
      value: String(stats.awaiting_review),
      hint: stats.awaiting_review > 0 ? 'needs a human' : 'nothing waiting',
    },
    {
      label: 'Merged',
      value: String(stats.merged),
      hint: `${stats.approved} approved`,
    },
    {
      label: 'Failed',
      value: String(stats.failed),
      hint: `${stats.cancelled} cancelled`,
    },
    {
      label: 'Spend',
      value: usd(stats.cost_usd),
      hint:
        stats.budget_daily_usd === null
          ? `${usd(stats.spent_today)} today`
          : `${usd(stats.spent_today)} of ${usd(stats.budget_daily_usd)} today`,
    },
  ]

  return (
    <div className="@container">
      <dl className="grid grid-cols-2 gap-x-6 gap-y-5 @2xl:grid-cols-3 @5xl:grid-cols-6">
        {tiles.map((tile) => (
          <div key={tile.label} className="flex flex-col gap-0.5 border-t border-foreground/10 pt-3">
            <dt className="truncate text-sm text-muted-foreground">{tile.label}</dt>
            <dd className="text-2xl font-medium tracking-tight tabular-nums sm:text-xl">
              {tile.value}
            </dd>
            <dd className="truncate text-sm text-muted-foreground/80 sm:text-xs">{tile.hint}</dd>
          </div>
        ))}
      </dl>
    </div>
  )
}
