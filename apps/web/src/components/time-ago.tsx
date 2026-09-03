import { Tooltip as TooltipPrimitive } from "@base-ui/react/tooltip";
import { useSyncExternalStore } from "react";

import { cn } from "@/lib/utils";

/**
 * The dub.co timestamp register: "Just now" under a second, then Ns/Nm/Nh ago,
 * then "May 30" within the year and "Dec 12, 2024" beyond it — with the exact
 * UTC and local wall-clock times one hover away.
 */

// One module-wide ticker instead of an interval per row: a task list renders
// dozens of these and they only need to agree, not to be independently fresh.
const TICK_MS = 30_000;
const listeners = new Set<() => void>();
let timer: ReturnType<typeof setInterval> | null = null;

function subscribe(listener: () => void) {
  listeners.add(listener);
  timer ??= setInterval(() => {
    for (const l of listeners) {
      l();
    }
  }, TICK_MS);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && timer) {
      clearInterval(timer);
      timer = null;
    }
  };
}

// Coarse snapshot so re-renders happen once per tick, not once per Date.now().
const now = () => Math.floor(Date.now() / TICK_MS) * TICK_MS;

// en-US regardless of viewer locale: the short forms are typography, and
// "May 30" / "Dec 12, 2024" is the shape the whole register is built around.
const MONTH_DAY = new Intl.DateTimeFormat("en-US", {
  day: "numeric",
  month: "short",
});
const MONTH_DAY_YEAR = new Intl.DateTimeFormat("en-US", {
  day: "numeric",
  month: "short",
  year: "numeric",
});

function short(at: number, reference: number): string {
  // A clock skewed slightly ahead must not render "in 3s".
  const seconds = Math.max(0, Math.floor((reference - at) / 1000));
  if (seconds < 1) {
    return "Just now";
  }
  if (seconds < 60) {
    return `${seconds}s ago`;
  }
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m ago`;
  }
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return `${hours}h ago`;
  }
  const date = new Date(at);
  return date.getFullYear() === new Date(reference).getFullYear()
    ? MONTH_DAY.format(date)
    : MONTH_DAY_YEAR.format(date);
}

/** "2 days, 22 hours, 35 minutes ago" — the largest three non-zero units. */
function long(at: number, reference: number): string {
  const seconds = Math.max(0, Math.floor((reference - at) / 1000));
  if (seconds < 60) {
    return seconds < 1 ? "just now" : plural(seconds, "second") + " ago";
  }
  const units: Array<[number, string]> = [
    [Math.floor(seconds / 86_400), "day"],
    [Math.floor((seconds % 86_400) / 3600), "hour"],
    [Math.floor((seconds % 3600) / 60), "minute"],
  ];
  const parts = units
    .filter(([n]) => n > 0)
    .map(([n, unit]) => plural(n, unit));
  return `${parts.join(", ")} ago`;
}

function plural(n: number, unit: string): string {
  return `${n} ${unit}${n === 1 ? "" : "s"}`;
}

function wallClock(at: number, timeZone?: string) {
  const date = new Date(at);
  return {
    date: new Intl.DateTimeFormat("en-US", {
      day: "numeric",
      month: "short",
      timeZone,
      year: "numeric",
    }).format(date),
    time: new Intl.DateTimeFormat("en-US", {
      hour: "2-digit",
      hour12: true,
      minute: "2-digit",
      second: "2-digit",
      timeZone,
    }).format(date),
  };
}

function localZoneName(at: number): string {
  const part = new Intl.DateTimeFormat("en-US", { timeZoneName: "short" })
    .formatToParts(new Date(at))
    .find((p) => p.type === "timeZoneName");
  return part ? part.value : "Local";
}

function ZoneRow({
  zone,
  at,
  timeZone,
}: {
  zone: string;
  at: number;
  timeZone?: string;
}) {
  const { date, time } = wallClock(at, timeZone);
  return (
    <>
      <span className="justify-self-start rounded bg-muted px-1 py-px font-mono text-[10px] text-muted-foreground">
        {zone}
      </span>
      <span>{date}</span>
      <span className="text-end font-mono tabular-nums">{time}</span>
    </>
  );
}

export function TimeAgo({ at, className }: { at: number; className?: string }) {
  const reference = useSyncExternalStore(subscribe, now, now);

  return (
    // Built on the Base UI parts, not ui/tooltip: the house tooltip is an
    // inverted one-line chip with an arrow, and a three-row card in inverse
    // video reads as a glare. This is a popover-palette card, arrowless, and
    // quick — a timestamp lookup is a glance, not a reveal.
    // Base UI keys open delay off the Provider, not the Root; a provider per
    // instance is just context and costs nothing.
    <TooltipPrimitive.Provider delay={100}>
      <TooltipPrimitive.Root>
        <TooltipPrimitive.Trigger
          render={
            <time
              className={cn("whitespace-nowrap", className)}
              dateTime={new Date(at).toISOString()}
              // The server's tick and the client's tick straddle real seconds.
              suppressHydrationWarning
            />
          }
        >
          {short(at, reference)}
        </TooltipPrimitive.Trigger>
        <TooltipPrimitive.Portal>
          <TooltipPrimitive.Positioner
            className="isolate z-50"
            side="top"
            sideOffset={6}
          >
            <TooltipPrimitive.Popup className="data-closed:fade-out-0 data-closed:zoom-out-95 data-open:fade-in-0 data-open:zoom-in-95 z-50 rounded-lg bg-popover px-3 py-2 text-popover-foreground text-xs border border-border shadow-md data-closed:animate-out data-open:animate-in">
              <p
                className="mb-1.5 text-muted-foreground"
                suppressHydrationWarning
              >
                {long(at, reference)}
              </p>
              <div className="grid grid-cols-[auto_1fr_auto] items-center gap-x-2 gap-y-1">
                <ZoneRow at={at} timeZone="UTC" zone="UTC" />
                {/* Rendering the local row on the server with the server's zone is
                  harmless: the tooltip cannot open before hydration. */}
                <ZoneRow at={at} zone={localZoneName(at)} />
              </div>
            </TooltipPrimitive.Popup>
          </TooltipPrimitive.Positioner>
        </TooltipPrimitive.Portal>
      </TooltipPrimitive.Root>
    </TooltipPrimitive.Provider>
  );
}
