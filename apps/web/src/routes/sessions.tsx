import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute } from "@tanstack/react-router";

import { projectName } from "@/components/app-sidebar";
import { OrchestratorError } from "@/components/orchestrator-error";
import { PageHeading } from "@/components/page-heading";
import { TimeAgo } from "@/components/time-ago";
import { getSessions } from "@/lib/lgtm/server";
import type { Session } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/sessions")({
  loader: async () => ({ sessions: await getSessions() }),
  component: SessionsPage,
  errorComponent: SessionsError,
});

function SessionsPage() {
  const { sessions } = Route.useLoaderData();
  const ordered = [...sessions].sort(
    (a, b) =>
      Number(a.archived) - Number(b.archived) || b.created_at - a.created_at
  );
  const live = sessions.filter((session) => !session.archived).length;

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <PageHeading meta={`${live} active`} title="Sessions" />

      {ordered.length === 0 ? (
        <p className="text-muted-foreground text-sm">No sessions yet.</p>
      ) : (
        <ul className="-mx-2 divide-y divide-foreground/5" role="list">
          {ordered.map((session) => (
            <li key={session.id}>
              <SessionRow session={session} />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function SessionRow({ session }: { session: Session }) {
  return (
    <div
      className={cn(
        "flex flex-wrap items-center gap-x-3 gap-y-1 px-2 py-2.5 text-sm sm:flex-nowrap sm:gap-x-4",
        session.archived && "text-muted-foreground"
      )}
    >
      <p className="order-last min-w-0 basis-full truncate sm:order-none sm:flex-1 sm:basis-auto">
        {session.title === "" ? (
          <span className="text-muted-foreground">(no messages yet)</span>
        ) : (
          session.title
        )}
      </p>

      <span className="w-32 shrink-0 truncate text-muted-foreground">
        {projectName(session.repository)}
      </span>

      <span className="w-40 shrink-0 truncate font-mono text-muted-foreground">
        {session.base_branch}
      </span>

      <TimeAgo
        at={session.created_at}
        className="grow truncate text-end text-muted-foreground tabular-nums sm:w-16 sm:grow-0"
      />
    </div>
  );
}

function SessionsError(props: ErrorComponentProps) {
  return <OrchestratorError what="sessions" {...props} />;
}
