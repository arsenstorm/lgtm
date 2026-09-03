import { Link } from "@tanstack/react-router";

import { Badge } from "@/components/ui/badge";
import {
  Card,
  CardAction,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import type { RunnerStatus } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

function gigabytes(mb: number): string {
  return mb >= 1024 ? `${Math.round(mb / 1024)} GB` : `${mb} MB`;
}

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

  return (
    <Card className="h-full">
      <CardHeader>
        <CardTitle className="truncate">{info.name}</CardTitle>
        <div className="text-muted-foreground text-sm">
          <span className="font-mono">{info.os}</span> ·{" "}
          <span className="font-mono">{info.arch}</span>
          {info.ephemeral ? " · ephemeral" : null}
        </div>
        <CardAction>
          <SlotMeter busy={busy} slots={info.slots} />
        </CardAction>
      </CardHeader>

      <CardContent className="flex flex-col gap-4">
        <dl className="grid grid-cols-2 gap-x-6 gap-y-3">
          <Field label="Executors">
            <div className="flex flex-wrap gap-1">
              {info.executors.map((executor) => (
                <Badge key={executor} variant="outline">
                  {executor}
                </Badge>
              ))}
            </div>
          </Field>
          <Field label="Hardware">
            <span className="tabular-nums">
              {info.cpu_cores} cores · {gigabytes(info.memory_mb)}
            </span>
          </Field>
        </dl>

        <dl className="flex flex-col gap-3">
          <Field label="Capabilities">
            {info.capabilities.length === 0 ? (
              <span className="text-muted-foreground">none declared</span>
            ) : (
              <div className="flex flex-wrap gap-1">
                {info.capabilities.map((capability) => (
                  <Badge key={capability} variant="secondary">
                    {capability}
                  </Badge>
                ))}
              </div>
            )}
          </Field>
          <Field label="Running now">
            {busy === 0 ? (
              <span className="text-muted-foreground">idle</span>
            ) : (
              <div className="flex flex-wrap gap-x-3 gap-y-1">
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
              </div>
            )}
          </Field>
        </dl>
      </CardContent>
    </Card>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <dt className="text-muted-foreground text-sm">{label}</dt>
      <dd className="text-base sm:text-sm">{children}</dd>
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
