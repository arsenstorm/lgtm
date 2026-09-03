import { Link } from "@tanstack/react-router";

import { Orb } from "@/components/aicss/Orb";
import { Badge } from "@/components/ui/badge";
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import type { RunnerStatus } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

function gigabytes(mb: number): string {
  return mb >= 1024 ? `${Math.round(mb / 1024)} GB` : `${mb} MB`;
}

// os:* and arch:* capabilities repeat the platform line in the header, so the
// chip row keeps only the tools a task could actually require.
const PLATFORM_CAPABILITY = /^(os|arch):/;

export function RunnerList({ runners }: { runners: RunnerStatus[] }) {
  if (runners.length === 0) {
    return (
      <div className="flex min-h-64 flex-col items-center justify-center gap-2 rounded-xl border border-foreground/15 border-dashed p-8 text-center">
        <h2 className="font-medium text-base">No runners connected</h2>
        <p className="max-w-[52ch] text-pretty text-base text-muted-foreground sm:text-sm">
          A runner registers itself over WebSocket and shows up here with its
          slots and capabilities. Start one on the machine that should do the
          work:
        </p>
        <code className="mt-1 text-sm">lgtm runner --name my-laptop</code>
      </div>
    );
  }

  return (
    <div className="@container">
      <ul className="grid @3xl:grid-cols-2 gap-4" role="list">
        {runners.map((runner) => (
          <li key={runner.info.name}>
            <RunnerCard runner={runner} />
          </li>
        ))}
      </ul>
    </div>
  );
}

function RunnerCard({ runner }: { runner: RunnerStatus }) {
  const { info, running } = runner;
  const busy = running.length;
  const tools = info.capabilities.filter(
    (capability) => !PLATFORM_CAPABILITY.test(capability)
  );

  return (
    <Card className="h-full justify-between gap-4">
      <CardHeader>
        <CardTitle className="flex items-baseline gap-2">
          <span className="truncate">{info.name}</span>
          {info.ephemeral ? <Badge variant="outline">ephemeral</Badge> : null}
        </CardTitle>
        <CardDescription className="font-mono">
          {info.os} · {info.arch}
        </CardDescription>
        <CardAction>
          <SlotMeter busy={busy} slots={info.slots} />
        </CardAction>
      </CardHeader>

      <CardContent className="flex flex-col gap-4">
        <dl className="grid grid-cols-3">
          <Stat className="pr-4" label="Cores">
            <span className="tabular-nums">{info.cpu_cores}</span>
          </Stat>
          <Stat className="border-border border-l px-4" label="Memory">
            <span className="tabular-nums">{gigabytes(info.memory_mb)}</span>
          </Stat>
          <Stat className="border-border border-l pl-4" label="Executors">
            {info.executors.join(", ")}
          </Stat>
        </dl>

        <dl>
          <Stat label="Tools">
            {tools.length === 0 ? (
              <span>none declared</span>
            ) : (
              <span className="flex flex-wrap gap-1">
                {tools.map((capability) => (
                  <Badge
                    className="font-mono font-normal"
                    key={capability}
                    variant="secondary"
                  >
                    {capability}
                  </Badge>
                ))}
              </span>
            )}
          </Stat>
        </dl>
      </CardContent>

      <CardFooter className="gap-2.5">
        {busy === 0 ? (
          <>
            <Orb label="Idle" size={18} variant="G5" />
            <span className="text-muted-foreground">Idle</span>
          </>
        ) : (
          <>
            <Orb label="Working" size={18} variant="S3" />
            <span className="text-muted-foreground">
              Running{" "}
              <span className="font-medium text-foreground tabular-nums">
                {busy}
              </span>{" "}
              {busy === 1 ? "task" : "tasks"}
            </span>
            <span className="flex flex-wrap gap-x-3 gap-y-1">
              {running.map((id) => (
                <Link
                  className="font-mono tabular-nums underline-offset-4 hover:underline"
                  key={id}
                  params={{ id }}
                  to="/tasks/$id"
                >
                  {id.slice(0, 8)}
                </Link>
              ))}
            </span>
          </>
        )}
      </CardFooter>
    </Card>
  );
}

function Stat({
  label,
  className,
  children,
}: {
  children: React.ReactNode;
  className?: string;
  label: string;
}) {
  return (
    <div className={cn("flex min-w-0 flex-col gap-1", className)}>
      <dt className="font-medium text-sm">{label}</dt>
      <dd className="text-muted-foreground text-sm [&_a]:text-foreground">
        {children}
      </dd>
    </div>
  );
}

function SlotMeter({ busy, slots }: { busy: number; slots: number }) {
  return (
    <div className="flex flex-col items-end gap-1.5">
      <div className="text-muted-foreground text-sm tabular-nums">
        <span className="font-medium text-foreground">{busy}</span> of {slots}{" "}
        slots
      </div>
      <div
        aria-label={`${busy} of ${slots} slots busy`}
        className="flex flex-wrap justify-end gap-1"
        role="img"
      >
        {Array.from({ length: slots }, (_, slot) => (
          <span
            className={cn(
              "h-1.5 w-6 rounded-full",
              slot < busy ? "bg-foreground" : "bg-foreground/15"
            )}
            key={slot}
          />
        ))}
      </div>
    </div>
  );
}
