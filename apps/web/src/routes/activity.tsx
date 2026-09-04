import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute, Link } from "@tanstack/react-router";

import { projectName } from "@/components/app-sidebar";
import { OrchestratorError } from "@/components/orchestrator-error";
import { PageHeading } from "@/components/page-heading";
import { TimeAgo } from "@/components/time-ago";
import { getActivity } from "@/lib/lgtm/server";
import type { ActivityEntry } from "@/lib/lgtm/types";

export const Route = createFileRoute("/activity")({
  loader: async () => ({ activity: await getActivity() }),
  component: ActivityPage,
  errorComponent: ActivityError,
});

function ActivityPage() {
  const { activity } = Route.useLoaderData();
  const feed = [...activity].sort((a, b) => b.at - a.at);

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <PageHeading meta={feed.length} title="Activity" />

      {feed.length === 0 ? (
        <p className="text-muted-foreground text-sm">No activity yet.</p>
      ) : (
        <ul className="-mx-2 divide-y divide-foreground/5" role="list">
          {feed.map((entry) => (
            <li key={`${entry.at}-${entry.task}-${entry.event}`}>
              <ActivityRow entry={entry} />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function ActivityRow({ entry }: { entry: ActivityEntry }) {
  return (
    <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1 px-2 py-2.5 text-sm sm:flex-nowrap">
      <TimeAgo
        at={entry.at}
        className="w-16 shrink-0 truncate text-muted-foreground tabular-nums"
      />

      <span className="w-40 shrink-0 truncate font-medium">{entry.event}</span>

      <Link
        className="w-16 shrink-0 truncate font-mono text-muted-foreground tabular-nums underline-offset-4 hover:underline"
        params={{ id: entry.task }}
        to="/tasks/$id"
      >
        {entry.task.slice(0, 8)}
      </Link>

      <span className="w-32 shrink-0 truncate text-muted-foreground">
        {projectName(entry.repository)}
      </span>

      {entry.owner && (
        <span className="max-w-32 shrink-0 truncate text-muted-foreground">
          {entry.owner}
        </span>
      )}

      {entry.detail && (
        <p className="min-w-0 flex-1 truncate text-muted-foreground">
          {entry.detail}
        </p>
      )}
    </div>
  );
}

function ActivityError(props: ErrorComponentProps) {
  return <OrchestratorError what="activity" {...props} />;
}
