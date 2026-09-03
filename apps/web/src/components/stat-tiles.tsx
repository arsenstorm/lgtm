import type { Stats } from "@/lib/lgtm/types";

import { shortSpan } from "./task-list";

const USD = new Intl.NumberFormat("en-US", {
  currency: "USD",
  style: "currency",
});

function usd(amount: number): string {
  return USD.format(amount);
}

export function StatTiles({ stats }: { stats: Stats }) {
  const tiles: { label: string; value: string; hint: string }[] = [
    {
      hint: `last ${shortSpan(Date.now() - stats.since)}`,
      label: "Tasks",
      value: String(stats.tasks),
    },
    {
      hint: `${stats.queued} queued`,
      label: "Running",
      value: String(stats.running),
    },
    {
      hint: stats.awaiting_review > 0 ? "needs a human" : "nothing waiting",
      label: "Awaiting review",
      value: String(stats.awaiting_review),
    },
    {
      hint: `${stats.approved} approved`,
      label: "Merged",
      value: String(stats.merged),
    },
    {
      hint: `${stats.cancelled} cancelled`,
      label: "Failed",
      value: String(stats.failed),
    },
    {
      hint:
        stats.budget_daily_usd === null
          ? `${usd(stats.spent_today)} today`
          : `${usd(stats.spent_today)} of ${usd(stats.budget_daily_usd)} today`,
      label: "Spend",
      value: usd(stats.cost_usd),
    },
  ];

  return (
    <div className="@container">
      <dl className="grid @2xl:grid-cols-3 @5xl:grid-cols-6 grid-cols-2 gap-x-6 gap-y-5">
        {tiles.map((tile) => (
          <div
            className="flex flex-col gap-0.5 border-foreground/10 border-t pt-3"
            key={tile.label}
          >
            <dt className="truncate text-muted-foreground text-sm">
              {tile.label}
            </dt>
            <dd className="font-medium text-2xl tabular-nums tracking-tight sm:text-xl">
              {tile.value}
            </dd>
            <dd className="truncate text-muted-foreground/80 text-sm sm:text-xs">
              {tile.hint}
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}
