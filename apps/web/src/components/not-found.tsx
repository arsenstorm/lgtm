import { Link, useLocation, useRouter } from "@tanstack/react-router";

import { ArrowBackIcon } from "@/components/icons";
import { PageHeading } from "@/components/page-heading";
import { Button } from "@/components/ui/button";

/** The page for a path that names nothing: an unknown route, or a detail
 *  page whose id the orchestrator no longer knows. The title sits where every
 *  list page puts its own, so it reads as a page of the app. */
export function NotFound() {
  const { pathname } = useLocation();
  const router = useRouter();

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-6 px-4 py-6 sm:px-6 lg:px-8">
      <PageHeading meta="404" title="Not found" />
      <div className="flex flex-col gap-4">
        <p className="max-w-[60ch] text-pretty text-muted-foreground text-sm">
          Nothing lives at{" "}
          <code className="break-all text-foreground text-sm">{pathname}</code>.
          It may have been deleted, or the link may be wrong.
        </p>
        <div className="flex items-center gap-2">
          <Button render={<Link to="/tasks" />} size="sm">
            Back to tasks
          </Button>
          <Button
            onClick={() => router.history.back()}
            size="sm"
            type="button"
            variant="ghost"
          >
            <ArrowBackIcon data-icon="inline-start" />
            Go back
          </Button>
        </div>
      </div>
    </div>
  );
}
