import { Link, useLocation } from "@tanstack/react-router";

import { Button } from "@/components/ui/button";

/** The page for a path that names nothing: an unknown route, or a detail
 *  page whose id the orchestrator no longer knows. */
export function NotFound() {
  const { pathname } = useLocation();

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-5xl flex-col px-4 py-6 sm:px-6 lg:px-8">
      <div className="flex min-h-64 flex-col items-center justify-center gap-2 rounded-xl border border-foreground/15 border-dashed p-8 text-center">
        <h1 className="font-medium text-base">Page not found</h1>
        <p className="max-w-[52ch] text-pretty text-base text-muted-foreground sm:text-sm">
          Nothing lives at <code className="break-all text-sm">{pathname}</code>
          . It may have been deleted, or the link may be wrong.
        </p>
        <Button className="mt-2" render={<Link to="/tasks" />} size="sm">
          Back to tasks
        </Button>
      </div>
    </div>
  );
}
