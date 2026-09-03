import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute } from "@tanstack/react-router";

import { OrchestratorError } from "@/components/orchestrator-error";
import { PageHeading } from "@/components/page-heading";
import { RunnerList } from "@/components/runner-list";
import { getRunners } from "@/lib/lgtm/server";

export const Route = createFileRoute("/runners")({
  loader: async () => ({ runners: await getRunners() }),
  component: RunnersPage,
  errorComponent: RunnersError,
});

function RunnersPage() {
  const { runners } = Route.useLoaderData();
  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <PageHeading meta={`${runners.length} online`} title="Runners" />
      <RunnerList runners={runners} />
    </div>
  );
}

function RunnersError(props: ErrorComponentProps) {
  return <OrchestratorError what="runners" {...props} />;
}
